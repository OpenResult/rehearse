//! Runtime planning engine for `rehearse`.
//!
//! This crate currently exposes the non-macro runtime: typed operation handles,
//! ordered plans, execute mode, and dry-run mode.

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
pub use plan::{Input, NodeId, OperationInputs, Plan, PlanBuilder, Value};
pub use policy::{DryRunAction, DryRunPolicy, SafeDryRun};
pub use report::{DryRunReport, DryRunStatus, NodeOutcome, NodeReport};
