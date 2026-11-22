#![forbid(unsafe_code)]

use clap::{error::ErrorKind, ArgGroup, CommandFactory, Parser, ValueEnum};
use rehearse::{
    ConsoleProgress, ConsoleProgressOptions, DryRunAction, DryRunPolicy, Impact, NoopProgress,
    OperationMetadata, ProgressListener, SafeDryRun,
};
use rehearse_github::{GitHubReporter, ReportOptions, ReportOutcome};
use rehearse_workflow_example::lab::{self, Lab};
use rehearse_workflow_example::model::{LabError, Stack, Stage};
use rehearse_workflow_example::workflows::{self, Workflow};
use serde::Serialize;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Policy {
    Safe,
    NoSession,
}

impl DryRunPolicy for Policy {
    fn action(&self, metadata: &OperationMetadata) -> DryRunAction {
        if *self == Self::NoSession && metadata.impact() == Impact::Session {
            DryRunAction::Deny
        } else {
            SafeDryRun.action(metadata)
        }
    }
}

#[derive(Parser)]
#[command(about = "Explore local CI/CD fixture workflows; defaults to a safe dry-run")]
#[command(
    long_about = "Explore local CI/CD fixture workflows with rehearse. Bundles, test reports, deployments and API gateway routes are local JSON files. No compilers, Java, Maven, servers or cloud credentials are used."
)]
#[command(group(ArgGroup::new("mode").args(["describe", "mermaid", "dry_run", "execute", "init"])))]
struct Args {
    /// Owned lab directory; --init creates both service fixtures.
    #[arg(default_value = "target/workflow-lab")]
    directory: PathBuf,
    #[arg(long, value_enum, default_value = "rust")]
    stack: Stack,
    #[arg(long, value_enum, default_value = "release")]
    workflow: Workflow,
    /// Artifact revision (default: seed for promote, demo-1 for other workflows).
    #[arg(long, value_parser = revision)]
    revision: Option<String>,
    /// Initialize a new lab, refusing to overwrite an existing directory.
    #[arg(long)]
    init: bool,
    /// Show static metadata and policy decisions without accessing the lab.
    #[arg(long)]
    describe: bool,
    /// Show static control structure and value inputs without accessing the lab.
    #[arg(long)]
    mermaid: bool,
    /// Rehearse allowed operations without changing the lab (the default).
    #[arg(long)]
    dry_run: bool,
    /// Execute fixture operations in order; stop at the first error.
    #[arg(long)]
    execute: bool,
    #[arg(long, value_enum, default_value = "text")]
    format: Format,
    /// Shorthand for --format json.
    #[arg(long, conflicts_with = "format")]
    json: bool,
    /// Print ConsoleProgress events (text execution/dry-run only).
    #[arg(long, conflicts_with_all = ["init", "describe", "mermaid"])]
    progress: bool,
    /// Emit GitHub progress and save reports in a fresh directory outside the lab.
    #[arg(long, value_name = "DIRECTORY", conflicts_with_all = ["init", "describe", "mermaid", "progress"])]
    github_report: Option<PathBuf>,
    /// Custom no-session policy denies authentication, preserving other safe actions.
    #[arg(long, value_enum, default_value = "safe")]
    policy: Policy,
    /// Fail if any dry-run node is skipped, denied, blocked, or failed.
    #[arg(long, conflicts_with_all = ["init", "describe", "mermaid", "execute"])]
    require_complete: bool,
    /// Return a controlled error when this stage body runs; never alters static plans.
    #[arg(long, value_enum, conflicts_with = "init")]
    fail_at: Option<Stage>,
}

fn revision(raw: &str) -> Result<String, String> {
    lab::validate_revision(raw).map_err(|error| error.to_string())?;
    Ok(raw.to_owned())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let mut args = Args::parse();
    if args.json {
        args.format = Format::Json;
    }
    if args.progress && args.format == Format::Json {
        Args::command()
            .error(
                ErrorKind::ArgumentConflict,
                "--progress requires --format text",
            )
            .exit();
    }
    if args.execute && args.policy != Policy::Safe {
        Args::command()
            .error(
                ErrorKind::ArgumentConflict,
                "dry-run policies do not apply to --execute; omit --policy",
            )
            .exit();
    }
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Workflow failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_json(value: &impl Serialize) -> Result<(), serde_json::Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// Resolve existing parents as well as the final path, so `..` and symlinked
// aliases cannot put reporting writes back inside the managed fixture tree.
fn resolved_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut missing = Vec::new();
    let mut existing = absolute.as_path();
    loop {
        match fs::canonicalize(existing) {
            Ok(mut resolved) => {
                for component in missing.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = existing.file_name().ok_or(error)?;
                missing.push(name.to_owned());
                existing = existing.parent().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "path has no existing parent")
                })?;
            }
            Err(error) => return Err(error),
        }
    }
}

fn check_reporting_path(path: &Path, lab: &Path) -> io::Result<()> {
    let report = resolved_path(path)?;
    let fixture = resolved_path(lab)?;
    if report.starts_with(&fixture) || fixture.starts_with(&report) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reporting paths must be outside and separate from the fixture lab",
        ));
    }
    Ok(())
}

async fn run(args: Args) -> Result<(), Box<dyn Error>> {
    if args.init {
        lab::initialize(&args.directory)?;
        if args.format == Format::Json {
            print_json(&serde_json::json!({
                "initialized": args.directory,
                "stacks": ["rust", "spring-boot"],
                "backend": "local-fixtures"
            }))?;
        } else {
            println!(
                "Initialized Rust and Spring Boot fixtures at {}",
                args.directory.display()
            );
        }
        return Ok(());
    }

    let plan = workflows::plan(args.workflow, args.fail_at);
    // Static interpretations precede context construction and all lab access.
    if args.describe {
        let description = plan.describe_with_policy(&args.policy);
        if args.format == Format::Json {
            print_json(&plan.describe_graph())?;
        } else {
            println!("{description}");
        }
        return Ok(());
    }
    if args.mermaid {
        let graph = plan.describe_graph().to_mermaid();
        if args.format == Format::Json {
            print_json(&serde_json::json!({"plan": plan.name(), "mermaid": graph}))?;
        } else {
            print!("{graph}");
        }
        return Ok(());
    }

    let revision = args.revision.unwrap_or_else(|| {
        if args.workflow == Workflow::Promote {
            "seed"
        } else {
            "demo-1"
        }
        .to_owned()
    });
    let lab = Lab {
        root: args.directory,
        stack: args.stack,
        revision,
        fail_at: args.fail_at,
    };
    let mut reporter = if let Some(directory) = &args.github_report {
        check_reporting_path(directory, &lab.root)?;
        let mut options = ReportOptions::new(directory)
            .with_metadata("stack", lab.stack.as_str())
            .with_metadata("workflow", plan.name())
            .with_metadata("artifact_revision", &lab.revision);
        if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
            let path = PathBuf::from(path);
            check_reporting_path(&path, &lab.root)?;
            options = options.with_summary_path(path);
        }
        for name in [
            "GITHUB_SHA",
            "GITHUB_RUN_ID",
            "GITHUB_RUN_ATTEMPT",
            "GITHUB_JOB",
        ] {
            if let Ok(value) = std::env::var(name) {
                options = options.with_metadata(name, value);
            }
        }
        Some(GitHubReporter::with_writer(
            plan.describe_graph(),
            options,
            io::stderr(),
        )?)
    } else {
        None
    };
    let mut fallback: Box<dyn ProgressListener<LabError>> = if args.progress {
        Box::new(ConsoleProgress::with_options(ConsoleProgressOptions {
            show_elapsed: false,
            ..ConsoleProgressOptions::default()
        }))
    } else {
        Box::new(NoopProgress)
    };
    let listener: &mut dyn ProgressListener<LabError> = match reporter.as_mut() {
        Some(reporter) => reporter,
        None => &mut *fallback,
    };

    let mut outcome = ReportOutcome::Succeeded;
    let workflow_result: Result<(), Box<dyn Error>> = async {
        if args.execute {
            if args.format == Format::Text {
                println!(
                    "Local fixture workflow for {} in {}",
                    lab.stack,
                    lab.root.display()
                );
                println!("{}", plan.describe_execution());
            }
            match plan.execute_with_listener(&lab, listener).await {
                Ok(summary) => {
                    if args.format == Format::Json {
                        print_json(&summary)?;
                    } else {
                        println!("{}", summary.message);
                    }
                }
                Err(error) => {
                    if args.format == Format::Json {
                        if let Err(report_error) = print_json(&error) {
                            eprintln!("Could not print workflow error JSON: {report_error}");
                        }
                    }
                    return Err(Box::new(error) as Box<dyn Error>);
                }
            }
        } else {
            let report = plan
                .dry_run_with_policy_and_listener(&lab, &args.policy, listener)
                .await;
            if report.status() != rehearse::DryRunStatus::Complete {
                outcome = ReportOutcome::Incomplete;
            }
            if args.format == Format::Json {
                print_json(&report)?;
            } else {
                println!(
                    "Local fixture dry-run for {} in {}\n",
                    lab.stack,
                    lab.root.display()
                );
                println!("{report}");
            }
            if args.require_complete {
                report.require_complete()?;
            } else {
                report.require_no_failures()?;
            }
        }
        Ok(())
    }
    .await;
    if workflow_result.is_err() {
        outcome = ReportOutcome::Failed;
    }
    let reporting_result = reporter
        .map(|reporter| reporter.finish(outcome))
        .transpose();
    match (workflow_result, reporting_result) {
        (Err(error), reporting_result) => {
            if let Err(report_error) = reporting_result {
                eprintln!("GitHub reporting also failed: {report_error}");
            }
            Err(error)
        }
        (Ok(()), Err(report_error)) => Err(Box::new(report_error)),
        (Ok(()), Ok(_)) => Ok(()),
    }
}
