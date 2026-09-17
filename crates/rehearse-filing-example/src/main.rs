mod filing;

use clap::{ArgGroup, Parser};
use filing::{filing_plan, seed_demo, Office};
use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(about = "Sort an office inbox by filename; defaults to a safe dry-run")]
#[command(group(ArgGroup::new("mode").args(["describe", "dry_run", "execute", "seed_demo"])))]
struct Args {
    /// Office directory containing inbox/ and the filing folders.
    #[arg(default_value = "target/filing-office")]
    directory: PathBuf,
    /// Show the static plan without accessing the office directory.
    #[arg(long)]
    describe: bool,
    /// Assess documents and destinations without changing files (the default).
    #[arg(long)]
    dry_run: bool,
    /// Create filing folders and move documents.
    #[arg(long)]
    execute: bool,
    /// Create a new office directory with sample documents, then exit.
    #[arg(long)]
    seed_demo: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Filing failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Args) -> Result<(), Box<dyn Error>> {
    if args.seed_demo {
        seed_demo(&args.directory)?;
        println!(
            "Sample inbox created at {}",
            args.directory.join("inbox").display()
        );
        return Ok(());
    }

    let plan = filing_plan();
    if args.describe {
        println!("{}", plan.describe());
        return Ok(());
    }

    let office = Office {
        root: args.directory,
    };
    if args.execute {
        println!("Filing documents in {}", office.root.display());
        let summary = plan.execute(&office).await?;
        println!(
            "{} documents filed and verified; {} require manual review.",
            summary.filed, summary.pending_review
        );
    } else {
        println!("Rehearsing filing in {}", office.root.display());
        let report = plan.dry_run(&office).await;
        println!("\n{report}");
        report.require_no_failures()?;
        println!("Pass --execute to file these documents.");
    }
    Ok(())
}
