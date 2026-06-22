//! Runtime planning engine for `rehearse`.
//!
//! This crate exposes typed operation handles, ordered plans, execute mode,
//! dry-run mode, and the optional macro frontend.

mod describe;
mod error;
mod impact;
mod operation;
mod policy;
mod report;

#[doc(hidden)]
pub mod __private;
pub mod plan;
pub mod runner;

pub use describe::{PlanDescription, PlanDescriptionRow};
pub use error::{DryRunFailure, ExecuteError};
pub use impact::Impact;
pub use operation::{BoxFuture, Operation, OperationMetadata};
pub use plan::{Input, IntoInput, NodeId, OperationInputs, Plan, PlanBuilder, Value};
pub use policy::{DryRunAction, DryRunPolicy, SafeDryRun};
pub use report::{DryRunReport, DryRunStatus, NodeOutcome, NodeReport};

#[cfg(feature = "macros")]
pub use rehearse_macros::{operation, pipeline, step};
