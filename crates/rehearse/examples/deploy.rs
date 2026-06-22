use rehearse::{operation, pipeline, Plan};
use serde_json::Value as JsonValue;
use std::env;
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

const TOKEN_VAR: &str = "CARGO_REGISTRY_TOKEN";
const DEFAULT_ENV_FILE: &str = ".env.local";
const CRATES_IO_API: &str = "https://crates.io/api/v1/crates";
const USER_AGENT: &str = "rehearse-publish-example (https://github.com/OpenResult/rehearse)";
const MACROS_CRATE: &str = "rehearse-macros";
const RUNTIME_CRATE: &str = "rehearse";
const INDEX_POLL_ATTEMPTS: usize = 30;
const INDEX_POLL_DELAY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishError(String);

impl PublishError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for PublishError {}

#[derive(Clone)]
struct Workspace {
    root: PathBuf,
}

impl Workspace {
    fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_owned()
        } else {
            self.root.join(path)
        }
    }
}

#[derive(Clone)]
struct PublishInput {
    env_file: PathBuf,
    allow_dirty_package_checks: bool,
}

#[derive(Clone)]
struct PublishSecrets {
    token: Option<String>,
}

impl PublishSecrets {
    fn token(&self) -> Result<&str, PublishError> {
        self.token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| {
                PublishError::new(format!(
                    "missing {TOKEN_VAR}; add `export {TOKEN_VAR}=...` to {DEFAULT_ENV_FILE}"
                ))
            })
    }
}

#[derive(Clone)]
struct PackageCheckOptions {
    allow_dirty_package_checks: bool,
}

#[derive(Clone)]
struct CrateTarget {
    name: String,
    version: String,
    publish: PublishSetting,
}

#[derive(Clone)]
enum PublishSetting {
    Enabled,
    Disabled,
    RegistryAllowList(Vec<String>),
}

#[derive(Clone)]
struct ReleaseState {
    macros: CrateTarget,
    runtime: CrateTarget,
}

impl ReleaseState {
    fn version(&self) -> &str {
        &self.runtime.version
    }
}

#[derive(Clone)]
struct PublishedCrate {
    state: ReleaseState,
    name: String,
    version: String,
}

#[derive(Clone)]
struct PublishOutcome {
    version: String,
}

#[operation(impact = read)]
async fn load_publish_secrets(
    #[context] workspace: &Workspace,
    env_file: PathBuf,
) -> Result<PublishSecrets, PublishError> {
    let mut token = env::var(TOKEN_VAR)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let path = workspace.resolve(&env_file);

    if path.exists() {
        let contents = fs::read_to_string(&path).map_err(|error| {
            PublishError::new(format!("failed to read {}: {error}", path.display()))
        })?;

        for (line_index, line) in contents.lines().enumerate() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let assignment = trimmed.strip_prefix("export ").unwrap_or(trimmed);
            let Some((key, value)) = assignment.split_once('=') else {
                continue;
            };

            if key.trim() == TOKEN_VAR {
                token = Some(parse_env_value(value.trim(), line_index + 1)?);
            }
        }
    }

    Ok(PublishSecrets { token })
}

#[operation(impact = pure)]
async fn package_check_options(
    allow_dirty_package_checks: bool,
) -> Result<PackageCheckOptions, PublishError> {
    Ok(PackageCheckOptions {
        allow_dirty_package_checks,
    })
}

#[operation(impact = read)]
async fn inspect_workspace_metadata(
    #[context] workspace: &Workspace,
) -> Result<ReleaseState, PublishError> {
    let output = run_command(
        workspace,
        "cargo",
        &["metadata", "--no-deps", "--format-version", "1"],
        None,
    )?;
    let json = serde_json::from_str::<JsonValue>(&output)
        .map_err(|error| PublishError::new(format!("failed to parse cargo metadata: {error}")))?;
    let packages = json
        .get("packages")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| PublishError::new("cargo metadata did not contain a packages array"))?;
    let macros = find_package(packages, MACROS_CRATE)?;
    let runtime = find_package(packages, RUNTIME_CRATE)?;

    if macros.version != runtime.version {
        return Err(PublishError::new(format!(
            "{MACROS_CRATE} is version {} but {RUNTIME_CRATE} is version {}",
            macros.version, runtime.version
        )));
    }

    Ok(ReleaseState { macros, runtime })
}

#[operation(impact = pure)]
async fn check_publish_enabled(state: ReleaseState) -> Result<ReleaseState, PublishError> {
    ensure_publish_enabled(&state.macros)?;
    ensure_publish_enabled(&state.runtime)?;
    Ok(state)
}

#[operation(impact = read)]
async fn check_version_not_published(state: ReleaseState) -> Result<ReleaseState, PublishError> {
    let mut existing = Vec::new();

    for package in [&state.macros, &state.runtime] {
        if crate_version_exists(&package.name, &package.version)? {
            existing.push(format!("{} {}", package.name, package.version));
        }
    }

    if existing.is_empty() {
        Ok(state)
    } else {
        Err(PublishError::new(format!(
            "crate version already exists on crates.io: {}",
            existing.join(", ")
        )))
    }
}

#[operation(impact = session)]
async fn run_fmt(
    #[context] workspace: &Workspace,
    state: ReleaseState,
) -> Result<ReleaseState, PublishError> {
    run_command(workspace, "cargo", &["fmt", "--all", "--check"], None)?;
    Ok(state)
}

#[operation(impact = session)]
async fn run_clippy(
    #[context] workspace: &Workspace,
    state: ReleaseState,
) -> Result<ReleaseState, PublishError> {
    run_command(
        workspace,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        None,
    )?;
    Ok(state)
}

#[operation(impact = session)]
async fn run_tests(
    #[context] workspace: &Workspace,
    state: ReleaseState,
) -> Result<ReleaseState, PublishError> {
    run_command(
        workspace,
        "cargo",
        &["test", "--workspace", "--all-features"],
        None,
    )?;
    Ok(state)
}

#[operation(impact = session)]
async fn run_local_publish_smoke(
    #[context] workspace: &Workspace,
    state: ReleaseState,
) -> Result<ReleaseState, PublishError> {
    run_command(workspace, "bash", &["scripts/publish-local.sh"], None)?;
    Ok(state)
}

#[operation(impact = session)]
async fn cargo_publish_dry_run_rehearse_macros(
    #[context] workspace: &Workspace,
    secrets: PublishSecrets,
    state: ReleaseState,
    options: PackageCheckOptions,
) -> Result<ReleaseState, PublishError> {
    cargo_publish_dry_run(
        workspace,
        &secrets,
        &state.macros.name,
        options.allow_dirty_package_checks,
    )?;
    Ok(state)
}

#[operation(impact = write)]
async fn publish_rehearse_macros(
    #[context] workspace: &Workspace,
    secrets: PublishSecrets,
    state: ReleaseState,
) -> Result<PublishedCrate, PublishError> {
    cargo_publish(workspace, &secrets, &state.macros.name)?;
    Ok(PublishedCrate {
        name: state.macros.name.clone(),
        version: state.macros.version.clone(),
        state,
    })
}

#[operation(impact = read)]
async fn wait_until_indexed(published: PublishedCrate) -> Result<ReleaseState, PublishError> {
    for attempt in 1..=INDEX_POLL_ATTEMPTS {
        if crate_version_exists(&published.name, &published.version)? {
            return Ok(published.state);
        }

        if attempt < INDEX_POLL_ATTEMPTS {
            thread::sleep(INDEX_POLL_DELAY);
        }
    }

    Err(PublishError::new(format!(
        "{} {} was not visible on crates.io after {} checks",
        published.name, published.version, INDEX_POLL_ATTEMPTS
    )))
}

#[operation(impact = session)]
async fn cargo_publish_dry_run_rehearse(
    #[context] workspace: &Workspace,
    secrets: PublishSecrets,
    state: ReleaseState,
    options: PackageCheckOptions,
) -> Result<ReleaseState, PublishError> {
    cargo_publish_dry_run(
        workspace,
        &secrets,
        &state.runtime.name,
        options.allow_dirty_package_checks,
    )?;
    Ok(state)
}

#[operation(impact = write)]
async fn publish_rehearse(
    #[context] workspace: &Workspace,
    secrets: PublishSecrets,
    state: ReleaseState,
) -> Result<PublishedCrate, PublishError> {
    cargo_publish(workspace, &secrets, &state.runtime.name)?;
    Ok(PublishedCrate {
        name: state.runtime.name.clone(),
        version: state.runtime.version.clone(),
        state,
    })
}

#[operation(impact = pure)]
async fn publish_complete(state: ReleaseState) -> Result<PublishOutcome, PublishError> {
    Ok(PublishOutcome {
        version: state.version().to_owned(),
    })
}

#[pipeline]
fn deploy(input: PublishInput) -> Plan<Workspace, PublishOutcome, PublishError> {
    let secrets = step!(load_publish_secrets(input.env_file))?;
    let package_options = rehearse::step!(package_check_options(input.allow_dirty_package_checks))?;
    let state = rehearse::step!(inspect_workspace_metadata())?;
    let state = rehearse::step!(check_publish_enabled(state))?;
    let state = rehearse::step!(check_version_not_published(state))?;
    let state = rehearse::step!(run_fmt(state))?;
    let state = rehearse::step!(run_clippy(state))?;
    let state = rehearse::step!(run_tests(state))?;
    let state = rehearse::step!(run_local_publish_smoke(state))?;
    let state = rehearse::step!(cargo_publish_dry_run_rehearse_macros(
        secrets,
        state,
        package_options
    ))?;
    let macros = rehearse::step!(publish_rehearse_macros(secrets, state))?;
    let state = rehearse::step!(wait_until_indexed(macros))?;
    let state = rehearse::step!(cargo_publish_dry_run_rehearse(
        secrets,
        state,
        package_options
    ))?;
    let runtime = rehearse::step!(publish_rehearse(secrets, state))?;
    let state = rehearse::step!(wait_until_indexed(runtime))?;
    let outcome = rehearse::step!(publish_complete(state))?;

    Ok(outcome)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse()?;
    let workspace = Workspace {
        root: env::current_dir()?,
    };
    let plan = deploy(PublishInput {
        env_file: args.env_file,
        allow_dirty_package_checks: !args.execute,
    });

    println!("{}", plan.describe());

    if args.execute {
        let outcome = plan.execute(&workspace).await?;
        println!(
            "published {RUNTIME_CRATE} {} and {MACROS_CRATE} {}",
            outcome.version, outcome.version
        );
    } else {
        let report = plan.dry_run(&workspace).await;
        println!("{report}");
        report.require_no_failures()?;
        println!("safe dry-run complete; pass --execute to publish for real");
    }

    Ok(())
}

struct Args {
    execute: bool,
    env_file: PathBuf,
}

impl Args {
    fn parse() -> Result<Self, PublishError> {
        let mut execute = false;
        let mut env_file = PathBuf::from(DEFAULT_ENV_FILE);
        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--execute" => execute = true,
                "--env-file" => {
                    let value = args
                        .next()
                        .ok_or_else(|| PublishError::new("--env-file requires a path"))?;
                    env_file = PathBuf::from(value);
                }
                "-h" | "--help" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ if arg.starts_with("--env-file=") => {
                    let value = arg
                        .split_once('=')
                        .map(|(_, value)| value)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| PublishError::new("--env-file requires a path"))?;
                    env_file = PathBuf::from(value);
                }
                _ => {
                    return Err(PublishError::new(format!("unsupported argument: {arg}")));
                }
            }
        }

        Ok(Self { execute, env_file })
    }
}

fn print_usage() {
    println!("Usage: cargo run -p rehearse --example deploy -- [--execute] [--env-file PATH]");
    println!();
    println!("Default mode describes the publish plan and runs safe dry-run checks.");
    println!("Use --execute only when {TOKEN_VAR} is present and you intend to publish.");
}

fn parse_env_value(raw: &str, line: usize) -> Result<String, PublishError> {
    if let Some(value) = raw.strip_prefix('"') {
        return value
            .strip_suffix('"')
            .map(|value| value.replace("\\\"", "\""))
            .ok_or_else(|| PublishError::new(format!("unterminated quoted value on line {line}")));
    }

    if let Some(value) = raw.strip_prefix('\'') {
        return value
            .strip_suffix('\'')
            .map(str::to_owned)
            .ok_or_else(|| PublishError::new(format!("unterminated quoted value on line {line}")));
    }

    Ok(raw.to_owned())
}

fn find_package(packages: &[JsonValue], name: &str) -> Result<CrateTarget, PublishError> {
    let package = packages
        .iter()
        .find(|package| package.get("name").and_then(JsonValue::as_str) == Some(name))
        .ok_or_else(|| PublishError::new(format!("cargo metadata did not contain {name}")))?;
    let version = package
        .get("version")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| PublishError::new(format!("{name} package did not contain a version")))?;
    let publish = match package.get("publish").unwrap_or(&JsonValue::Null) {
        JsonValue::Null => PublishSetting::Enabled,
        JsonValue::Array(registries) if registries.is_empty() => PublishSetting::Disabled,
        JsonValue::Array(registries) => {
            let registries = registries
                .iter()
                .map(|registry| {
                    registry.as_str().map(str::to_owned).ok_or_else(|| {
                        PublishError::new(format!("{name} package has non-string publish entry"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            PublishSetting::RegistryAllowList(registries)
        }
        _ => {
            return Err(PublishError::new(format!(
                "{name} package has unsupported publish metadata"
            )));
        }
    };

    Ok(CrateTarget {
        name: name.to_owned(),
        version: version.to_owned(),
        publish,
    })
}

fn ensure_publish_enabled(package: &CrateTarget) -> Result<(), PublishError> {
    match &package.publish {
        PublishSetting::Enabled => Ok(()),
        PublishSetting::RegistryAllowList(registries)
            if registries.iter().any(|registry| registry == "crates-io") =>
        {
            Ok(())
        }
        PublishSetting::RegistryAllowList(registries) => Err(PublishError::new(format!(
            "{} publish allow-list does not include crates-io: {}",
            package.name,
            registries.join(", ")
        ))),
        PublishSetting::Disabled => Err(PublishError::new(format!(
            "{} is not publish-enabled; remove `publish = false`",
            package.name
        ))),
    }
}

fn crate_version_exists(name: &str, version: &str) -> Result<bool, PublishError> {
    let status = crates_io_status(name, version)?;

    match status {
        200 => Ok(true),
        404 => Ok(false),
        other => Err(PublishError::new(format!(
            "unexpected crates.io status for {name} {version}: {other}"
        ))),
    }
}

fn crates_io_status(name: &str, version: &str) -> Result<u16, PublishError> {
    let url = format!("{CRATES_IO_API}/{name}/{version}");
    let output = Command::new("curl")
        .args(["-sS", "-o", "/dev/null", "-w", "%{http_code}"])
        .arg("-H")
        .arg(format!("User-Agent: {USER_AGENT}"))
        .arg(&url)
        .output()
        .map_err(|error| PublishError::new(format!("failed to run curl: {error}")))?;

    if !output.status.success() {
        return Err(PublishError::new(format!(
            "curl failed while checking {url}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse::<u16>().map_err(|error| {
        PublishError::new(format!(
            "curl returned invalid HTTP status for {url}: {error}"
        ))
    })
}

fn cargo_publish_dry_run(
    workspace: &Workspace,
    secrets: &PublishSecrets,
    package: &str,
    allow_dirty: bool,
) -> Result<(), PublishError> {
    let mut args = vec!["publish", "--dry-run", "-p", package];

    if allow_dirty {
        args.push("--allow-dirty");
    }

    run_command(workspace, "cargo", &args, secrets.token.as_deref())?;
    Ok(())
}

fn cargo_publish(
    workspace: &Workspace,
    secrets: &PublishSecrets,
    package: &str,
) -> Result<(), PublishError> {
    run_command(
        workspace,
        "cargo",
        &["publish", "-p", package],
        Some(secrets.token()?),
    )?;
    Ok(())
}

fn run_command(
    workspace: &Workspace,
    program: &str,
    args: &[&str],
    token: Option<&str>,
) -> Result<String, PublishError> {
    let mut command = Command::new(program);
    command.current_dir(&workspace.root).args(args);

    if let Some(token) = token {
        command.env(TOKEN_VAR, token);
    }

    let output = command.output().map_err(|error| {
        PublishError::new(format!(
            "failed to run {}: {error}",
            command_line(program, args)
        ))
    })?;

    if output.status.success() {
        return String::from_utf8(output.stdout).map_err(|error| {
            PublishError::new(format!(
                "{} produced non-UTF-8 stdout: {error}",
                command_line(program, args)
            ))
        });
    }

    Err(PublishError::new(format!(
        "command failed: {}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        command_line(program, args),
        output.status.code().map_or_else(
            || "terminated by signal".to_owned(),
            |code| code.to_string()
        ),
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn command_line(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}
