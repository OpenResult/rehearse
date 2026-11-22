#![forbid(unsafe_code)]

use clap::Parser;
use rehearse_workflow_example::server::{
    router,
    state::{ServerConfig, ServerState},
};
use std::error::Error;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(about = "Serve the local rehearse fixture dashboard with production approval gates")]
struct Args {
    /// Owned fixture directory. Run state and approvals remain in memory.
    #[arg(default_value = "target/workflow-server")]
    directory: PathBuf,
    /// Loopback address only; port 0 chooses a free local port.
    #[arg(long, default_value = "127.0.0.1:3000")]
    bind: SocketAddr,
    #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(1..=3600))]
    approval_timeout_secs: u64,
    /// Small pacing delay to make fixture progress visible.
    #[arg(long, default_value_t = 350, value_parser = clap::value_parser!(u64).range(0..=2000))]
    step_delay_ms: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if !args.bind.ip().is_loopback() {
        return Err("this local example only binds to loopback addresses".into());
    }
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    let address = listener.local_addr()?;
    let state = ServerState::new(ServerConfig {
        root: args.directory,
        approval_timeout: Duration::from_secs(args.approval_timeout_secs),
        step_delay: Duration::from_millis(args.step_delay_ms),
        max_runs: 32,
        event_capacity: 256,
    })?;
    let origin = format!("http://{address}");
    println!("Rehearse workflow server: {origin}");
    println!("Local fixtures only. Runs and approvals are lost when this process stops.");
    let shutdown_state = state.clone();
    axum::serve(listener, router(state, origin))
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_state.cancel_all();
        })
        .await?;
    Ok(())
}
