use clap::Parser;
use rehearse::{operation, pipeline, ConsoleProgress, ConsoleProgressOptions, Plan};
use std::error::Error;
use std::fmt;
use std::thread;
use std::time::Duration;

const DEFAULT_SEED: u64 = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RolloutError(String);

impl fmt::Display for RolloutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for RolloutError {}

#[derive(Clone)]
struct RolloutServices {
    target: String,
}

#[derive(Debug, Clone, Copy)]
struct StepDelay {
    millis: u64,
}

impl StepDelay {
    fn sample(rng: &mut DemoRng) -> Self {
        Self {
            millis: rng.range_inclusive(1_000),
        }
    }
}

#[derive(Clone)]
struct Conditions {
    seed: u64,
    error_rate_per_10k: u32,
    traffic_rps: u32,
    incident_open: bool,
    approval_signal: u8,
    regions: u8,
}

#[derive(Clone)]
struct EnvironmentSnapshot {
    target: String,
    conditions: Conditions,
    pressure: u8,
}

#[derive(Clone)]
struct RiskAssessment {
    environment: EnvironmentSnapshot,
    score: u8,
    reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RolloutStrategy {
    Pause,
    Canary,
    Regional,
    Full,
}

impl fmt::Display for RolloutStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pause => f.write_str("pause"),
            Self::Canary => f.write_str("canary"),
            Self::Regional => f.write_str("regional"),
            Self::Full => f.write_str("full"),
        }
    }
}

#[derive(Clone)]
struct RolloutDecision {
    assessment: RiskAssessment,
    strategy: RolloutStrategy,
    percent: u8,
}

#[derive(Clone)]
struct Approval {
    decision: RolloutDecision,
    approved: bool,
    reason: String,
}

#[derive(Clone)]
struct RolloutReceipt {
    approval: Approval,
    rollout_id: String,
    applied: bool,
}

#[derive(Clone)]
struct Verification {
    receipt: RolloutReceipt,
    healthy: bool,
    observed_error_rate_per_10k: u32,
}

#[derive(Clone)]
struct RolloutSummary {
    target: String,
    seed: u64,
    strategy: RolloutStrategy,
    percent: u8,
    applied: bool,
    healthy: bool,
    risk_score: u8,
    risk_reason: String,
    observed_error_rate_per_10k: u32,
    message: String,
}

#[operation(impact = pure)]
async fn sample_conditions(delay: StepDelay, seed: u64) -> Result<Conditions, RolloutError> {
    sleep(delay);

    let mut rng = DemoRng::new(seed ^ 0xa11c_e5eed);
    Ok(Conditions {
        seed,
        error_rate_per_10k: 10 + rng.range_inclusive(180) as u32,
        traffic_rps: 400 + rng.range_inclusive(5_600) as u32,
        incident_open: rng.chance(18),
        approval_signal: rng.range_inclusive(100) as u8,
        regions: 1 + rng.range_inclusive(5) as u8,
    })
}

#[operation(impact = read)]
async fn inspect_environment(
    #[context] services: &RolloutServices,
    delay: StepDelay,
    conditions: Conditions,
) -> Result<EnvironmentSnapshot, RolloutError> {
    sleep(delay);

    let pressure = ((conditions.traffic_rps / 850)
        + (conditions.error_rate_per_10k / 45)
        + u32::from(conditions.incident_open) * 4)
        .min(10) as u8;

    Ok(EnvironmentSnapshot {
        target: services.target.clone(),
        conditions,
        pressure,
    })
}

#[operation(impact = pure)]
async fn score_risk(
    delay: StepDelay,
    environment: EnvironmentSnapshot,
) -> Result<RiskAssessment, RolloutError> {
    sleep(delay);

    let conditions = &environment.conditions;
    let mut score = (conditions.error_rate_per_10k / 3)
        + (conditions.traffic_rps / 180)
        + u32::from(environment.pressure) * 3
        + u32::from(conditions.regions) * 2;

    let mut reasons = Vec::new();
    if conditions.incident_open {
        score += 28;
        reasons.push("incident open");
    }
    if conditions.approval_signal < 35 {
        score += 12;
        reasons.push("weak approval signal");
    }
    if conditions.traffic_rps > 4_000 {
        reasons.push("high traffic");
    }
    if conditions.error_rate_per_10k > 120 {
        reasons.push("elevated errors");
    }
    if reasons.is_empty() {
        reasons.push("conditions normal");
    }

    Ok(RiskAssessment {
        environment,
        score: score.min(100) as u8,
        reason: reasons.join(", "),
    })
}

#[operation(impact = pure)]
async fn choose_rollout_strategy(
    delay: StepDelay,
    assessment: RiskAssessment,
) -> Result<RolloutDecision, RolloutError> {
    sleep(delay);

    let conditions = &assessment.environment.conditions;
    let strategy = if conditions.incident_open || assessment.score >= 82 {
        RolloutStrategy::Pause
    } else if assessment.score >= 62 {
        RolloutStrategy::Canary
    } else if conditions.traffic_rps >= 3_500 || conditions.regions >= 4 {
        RolloutStrategy::Regional
    } else {
        RolloutStrategy::Full
    };
    let percent = match strategy {
        RolloutStrategy::Pause => 0,
        RolloutStrategy::Canary => 5,
        RolloutStrategy::Regional => 25,
        RolloutStrategy::Full => 100,
    };

    Ok(RolloutDecision {
        assessment,
        strategy,
        percent,
    })
}

#[operation(impact = session)]
async fn request_approval(
    delay: StepDelay,
    decision: RolloutDecision,
) -> Result<Approval, RolloutError> {
    sleep(delay);

    let conditions = &decision.assessment.environment.conditions;
    let approved = decision.strategy != RolloutStrategy::Pause
        && (decision.assessment.score < 70 || conditions.approval_signal >= 55);
    let reason = if approved {
        format!(
            "{} rollout approved with signal {}",
            decision.strategy, conditions.approval_signal
        )
    } else if decision.strategy == RolloutStrategy::Pause {
        "rollout paused by risk policy".to_owned()
    } else {
        format!(
            "approval signal {} too low for risk score {}",
            conditions.approval_signal, decision.assessment.score
        )
    };

    Ok(Approval {
        decision,
        approved,
        reason,
    })
}

#[operation(impact = write)]
async fn apply_rollout(
    delay: StepDelay,
    approval: Approval,
) -> Result<RolloutReceipt, RolloutError> {
    sleep(delay);

    let applied = approval.approved && approval.decision.percent > 0;
    let rollout_id = if applied {
        let seed = approval.decision.assessment.environment.conditions.seed;
        format!(
            "rollout-{:016x}",
            rollout_token(seed, approval.decision.percent)
        )
    } else {
        "not-applied".to_owned()
    };

    Ok(RolloutReceipt {
        approval,
        rollout_id,
        applied,
    })
}

#[operation(impact = read)]
async fn verify_rollout(
    delay: StepDelay,
    receipt: RolloutReceipt,
) -> Result<Verification, RolloutError> {
    sleep(delay);

    let conditions = &receipt.approval.decision.assessment.environment.conditions;
    let observed_error_rate_per_10k = if receipt.applied {
        conditions.error_rate_per_10k + u32::from(receipt.approval.decision.percent) / 2
    } else {
        conditions.error_rate_per_10k
    };
    let healthy = !receipt.applied
        || (observed_error_rate_per_10k < 170 && receipt.approval.decision.assessment.score < 86);

    Ok(Verification {
        receipt,
        healthy,
        observed_error_rate_per_10k,
    })
}

#[operation(impact = pure)]
async fn summarize_outcome(
    delay: StepDelay,
    verification: Verification,
) -> Result<RolloutSummary, RolloutError> {
    sleep(delay);

    let decision = &verification.receipt.approval.decision;
    let assessment = &decision.assessment;
    let conditions = &assessment.environment.conditions;
    let message = if verification.receipt.applied && verification.healthy {
        format!(
            "{} applied at {}% as {}",
            verification.receipt.rollout_id, decision.percent, decision.strategy
        )
    } else if verification.receipt.applied {
        format!(
            "{} applied but verification needs attention",
            verification.receipt.rollout_id
        )
    } else {
        format!(
            "rollout not applied: {}",
            verification.receipt.approval.reason
        )
    };

    Ok(RolloutSummary {
        target: assessment.environment.target.clone(),
        seed: conditions.seed,
        strategy: decision.strategy,
        percent: decision.percent,
        applied: verification.receipt.applied,
        healthy: verification.healthy,
        risk_score: assessment.score,
        risk_reason: assessment.reason.clone(),
        observed_error_rate_per_10k: verification.observed_error_rate_per_10k,
        message,
    })
}

#[pipeline]
fn conditional_rollout(seed: u64) -> Plan<RolloutServices, RolloutSummary, RolloutError> {
    let mut rng = DemoRng::new(seed);
    let sample_delay = StepDelay::sample(&mut rng);
    let inspect_delay = StepDelay::sample(&mut rng);
    let score_delay = StepDelay::sample(&mut rng);
    let strategy_delay = StepDelay::sample(&mut rng);
    let approval_delay = StepDelay::sample(&mut rng);
    let apply_delay = StepDelay::sample(&mut rng);
    let verify_delay = StepDelay::sample(&mut rng);
    let summary_delay = StepDelay::sample(&mut rng);

    let conditions = step!(sample_conditions(sample_delay, seed))?;
    let environment = step!(inspect_environment(inspect_delay, conditions))?;
    let assessment = step!(score_risk(score_delay, environment))?;
    let decision = step!(choose_rollout_strategy(strategy_delay, assessment))?;
    let approval = step!(request_approval(approval_delay, decision))?;
    let receipt = step!(apply_rollout(apply_delay, approval))?;
    let verification = step!(verify_rollout(verify_delay, receipt))?;
    let summary = step!(summarize_outcome(summary_delay, verification))?;

    Ok(summary)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    let services = RolloutServices {
        target: "checkout-service".to_owned(),
    };
    let plan = conditional_rollout(args.seed);

    if args.execute {
        println!("{}", plan.describe_execution());
        let mut progress = rollout_progress();
        let summary = plan.execute_with_listener(&services, &mut progress).await?;
        println!();
        print_summary(&summary);
    } else {
        println!("{}", plan.describe());
        let mut progress = rollout_progress();
        let report = plan.dry_run_with_listener(&services, &mut progress).await;
        println!();
        println!("{report}");
        report.require_no_failures()?;
        println!("safe dry-run complete; pass --execute to apply the rollout");
    }

    Ok(())
}

#[derive(Debug, Parser)]
#[command(about = "Rehearse or execute a simulated feature rollout")]
struct Args {
    /// Execute the rollout instead of running the safe dry-run.
    #[arg(long)]
    execute: bool,
    /// Seed controlling simulated conditions and per-step delays.
    #[arg(long, default_value_t = DEFAULT_SEED)]
    seed: u64,
}

fn rollout_progress() -> ConsoleProgress {
    ConsoleProgress::with_options(ConsoleProgressOptions {
        show_impact: false,
        ..ConsoleProgressOptions::default()
    })
}

struct DemoRng {
    state: u64,
}

impl DemoRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn range_inclusive(&mut self, max: u64) -> u64 {
        self.next_u64() % (max + 1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.range_inclusive(99) < percent
    }
}

fn sleep(delay: StepDelay) {
    if delay.millis > 0 {
        thread::sleep(Duration::from_millis(delay.millis));
    }
}

fn rollout_token(seed: u64, percent: u8) -> u64 {
    let mut rng = DemoRng::new(seed ^ u64::from(percent));
    rng.next_u64()
}

fn print_summary(summary: &RolloutSummary) {
    println!("rollout summary");
    println!("  target: {}", summary.target);
    println!("  seed: {}", summary.seed);
    println!("  strategy: {}", summary.strategy);
    println!("  percent: {}", summary.percent);
    println!("  applied: {}", summary.applied);
    println!("  healthy: {}", summary.healthy);
    println!("  risk: {} ({})", summary.risk_score, summary.risk_reason);
    println!(
        "  observed error rate: {} per 10k",
        summary.observed_error_rate_per_10k
    );
    println!("  result: {}", summary.message);
}
