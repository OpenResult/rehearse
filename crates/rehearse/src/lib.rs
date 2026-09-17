//! Build typed operation plans, then describe, dry-run, or execute them.
//!
//! `rehearse` records declared operation impact. It does not infer effects from
//! arbitrary Rust code. A pipeline builds a static ordered [`Plan`], and the
//! runner decides whether each operation is only described, safely rehearsed, or
//! fully executed.
//!
//! The default macro frontend is available through `#[operation]` and
//! `#[pipeline]`. The manual [`PlanBuilder`] API remains public for tests and
//! lower-level integrations.

//! Manual construction works with default features disabled:
//!
//! ```
//! use rehearse::{Impact, Operation, OperationMetadata, PlanBuilder};
//!
//! let mut builder = PlanBuilder::<(), ()>::new("example");
//! let output = builder.add(Operation::sync(
//!     OperationMetadata::new("answer", Impact::Pure),
//!     (),
//!     |_, ()| Ok(42_u32),
//! ));
//! let plan = builder.try_finish(output).expect("valid plan");
//! assert_eq!(plan.describe().len(), 1);
//! ```

#![forbid(unsafe_code)]

mod describe;
mod error;
mod impact;
mod operation;
mod policy;
mod progress;
mod report;

pub mod plan;
mod runner;

pub use describe::{
    PlanDescription, PlanDescriptionRow, PlanExecutionDescription, PlanExecutionDescriptionRow,
};
pub use error::{DryRunFailure, ExecuteError, InvariantError, PlanBuildError, ValueError};
pub use impact::Impact;
pub use operation::{BoxFuture, Operation, OperationMetadata};
pub use plan::{Input, IntoInput, NodeId, OperationInputs, Plan, PlanBuilder, Value};
pub use policy::{DryRunAction, DryRunPolicy, SafeDryRun};
pub use progress::{
    ConsoleProgress, ConsoleProgressOptions, NoopProgress, ProgressEvent, ProgressListener,
    ProgressMode, ProgressNode, ProgressOutcome, ProgressPlanOutcome,
};
pub use report::{DryRunIncomplete, DryRunReport, DryRunStatus, NodeOutcome, NodeReport};

#[cfg(feature = "macros")]
pub use rehearse_macros::{operation, pipeline, step};
