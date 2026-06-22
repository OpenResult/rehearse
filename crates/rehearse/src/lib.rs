//! Build typed operation plans, then describe, dry-run, or execute them.
//!
//! `rehearse` records declared operation impact. It does not infer effects from
//! arbitrary Rust code. A pipeline builds a static ordered [`Plan`], and the
//! runner decides whether each operation is only described, safely rehearsed, or
//! fully executed.
//!
//! The default macro frontend is available through [`operation`] and
//! [`pipeline`]. The manual [`PlanBuilder`] API remains public for tests and
//! lower-level integrations.

#![forbid(unsafe_code)]

mod describe;
mod error;
mod impact;
mod operation;
mod policy;
mod report;

#[doc(hidden)]
pub mod __private;
pub mod plan;
mod runner;

pub use describe::{
    PlanDescription, PlanDescriptionRow, PlanExecutionDescription, PlanExecutionDescriptionRow,
};
pub use error::{DryRunFailure, ExecuteError};
pub use impact::Impact;
pub use operation::{BoxFuture, Operation, OperationMetadata};
pub use plan::{Input, IntoInput, NodeId, OperationInputs, Plan, PlanBuilder, Value};
pub use policy::{DryRunAction, DryRunPolicy, SafeDryRun};
pub use report::{DryRunReport, DryRunStatus, NodeOutcome, NodeReport};

#[cfg(feature = "macros")]
pub use rehearse_macros::{operation, pipeline, step};
