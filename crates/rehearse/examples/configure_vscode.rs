use clap::Parser;
use rehearse::{operation, pipeline, Plan};
use serde_json::{json, Map, Value};
use std::error::Error;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_SETTINGS_PATH: &str = ".vscode/settings.json";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigureError(String);

impl ConfigureError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ConfigureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ConfigureError {}

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
struct SettingsDocument {
    path: PathBuf,
    existed: bool,
    values: Map<String, Value>,
}

#[derive(Clone)]
struct SettingsUpdate {
    path: PathBuf,
    existed: bool,
    values: Map<String, Value>,
    added: Vec<String>,
}

impl SettingsUpdate {
    fn needs_write(&self) -> bool {
        !self.added.is_empty()
    }
}

#[derive(Clone)]
struct SettingsResult {
    path: PathBuf,
    existed: bool,
    written: bool,
    added: Vec<String>,
}

#[operation(impact = read)]
async fn read_settings(
    #[context] workspace: &Workspace,
    path: PathBuf,
) -> Result<SettingsDocument, ConfigureError> {
    let absolute = workspace.resolve(&path);

    if !absolute.exists() {
        return Ok(SettingsDocument {
            path,
            existed: false,
            values: Map::new(),
        });
    }

    let contents = fs::read_to_string(&absolute).map_err(|error| {
        ConfigureError::new(format!("failed to read {}: {error}", absolute.display()))
    })?;
    let value = serde_json::from_str::<Value>(&contents).map_err(|error| {
        ConfigureError::new(format!(
            "failed to parse {} as JSON: {error}",
            absolute.display()
        ))
    })?;
    let Value::Object(values) = value else {
        return Err(ConfigureError::new(format!(
            "{} must contain a JSON object",
            absolute.display()
        )));
    };

    Ok(SettingsDocument {
        path,
        existed: true,
        values,
    })
}

#[operation(impact = pure)]
async fn add_missing_rust_analyzer_settings(
    document: SettingsDocument,
) -> Result<SettingsUpdate, ConfigureError> {
    let SettingsDocument {
        path,
        existed,
        mut values,
    } = document;
    let mut added = Vec::new();

    for (key, value) in required_settings() {
        if !values.contains_key(key) {
            values.insert(key.to_owned(), value);
            added.push(key.to_owned());
        }
    }

    Ok(SettingsUpdate {
        path,
        existed,
        values,
        added,
    })
}

#[operation(impact = write)]
async fn write_settings(
    #[context] workspace: &Workspace,
    update: SettingsUpdate,
) -> Result<SettingsResult, ConfigureError> {
    let absolute = workspace.resolve(&update.path);

    if update.needs_write() {
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ConfigureError::new(format!("failed to create {}: {error}", parent.display()))
            })?;
        }

        let mut output = serde_json::to_string_pretty(&Value::Object(update.values.clone()))
            .map_err(|error| {
                ConfigureError::new(format!("failed to format VS Code settings: {error}"))
            })?;
        output.push('\n');

        fs::write(&absolute, output).map_err(|error| {
            ConfigureError::new(format!("failed to write {}: {error}", absolute.display()))
        })?;
    }

    let written = update.needs_write();

    Ok(SettingsResult {
        path: update.path,
        existed: update.existed,
        written,
        added: update.added,
    })
}

#[pipeline]
fn configure_vscode_settings(path: PathBuf) -> Plan<Workspace, SettingsResult, ConfigureError> {
    let document = step!(read_settings(path))?;
    let update = step!(add_missing_rust_analyzer_settings(document))?;
    let result = step!(write_settings(update))?;

    Ok(result)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let workspace = Workspace {
        root: std::env::current_dir()?,
    };
    let plan = configure_vscode_settings(args.path);

    println!("{}", plan.describe());

    let report = plan.dry_run(&workspace).await;
    println!("{report}");
    report.require_no_failures()?;

    if args.dry_run {
        println!("dry-run requested; settings were not written");
        return Ok(());
    }

    let result = plan.execute(&workspace).await?;
    print_result(&result);

    Ok(())
}

#[derive(Debug, Parser)]
#[command(about = "Configure VS Code rust-analyzer settings for this workspace")]
struct Args {
    /// Rehearse the write without changing the settings file.
    #[arg(long)]
    dry_run: bool,
    /// Settings file path, relative to the current workspace unless absolute.
    #[arg(value_name = "settings-path", default_value = DEFAULT_SETTINGS_PATH)]
    path: PathBuf,
}

fn required_settings() -> [(&'static str, Value); 6] {
    [
        ("rust-analyzer.procMacro.enable", json!(true)),
        ("rust-analyzer.cargo.allTargets", json!(true)),
        ("rust-analyzer.cargo.features", json!("all")),
        ("rust-analyzer.check.command", json!("clippy")),
        ("rust-analyzer.check.allTargets", json!(true)),
        ("rust-analyzer.check.features", json!("all")),
    ]
}

fn print_result(result: &SettingsResult) {
    let path = result.path.display();

    if result.written {
        println!(
            "updated {path}: added {} setting{}",
            result.added.len(),
            if result.added.len() == 1 { "" } else { "s" }
        );
        for key in &result.added {
            println!("  + {key}");
        }
    } else if result.existed {
        println!("{path} already contains the required rust-analyzer settings");
    } else {
        println!("{path} created with the required rust-analyzer settings");
    }
}
