//! `rinne-core` — the loop engine, scheduler, context assembler, blackboard,
//! and DAG types.
//!
//! Phase 0 establishes only the shared error/result types and the crate
//! skeleton. The blackboard, DAG types, scheduler, dispatcher, and context
//! assembler land in Phase 3 (`CONTEXT.md` §12, `PHASE.md` P3).

pub mod assembler;
pub mod blackboard;
pub mod dag;
pub mod engine;
pub mod error;
pub mod evaluator;
pub mod pool;
pub mod priors;
pub mod ratchet;
pub mod registry;
pub mod replanner;
pub mod state;
pub mod worker;

pub use blackboard::Blackboard;
pub use dag::{Node, OnFail, Plan};
pub use engine::{
    Engine, EngineEvent, EngineOptions, EngineSink, HumanDecision, ResumeInput, RunReport,
    StopReason,
};
pub use error::{Result, RinneError};
pub use registry::WorkerRegistry;
pub use replanner::Replanner;
pub use state::{NodeStatus, State};
pub use worker::{
    AuthMode, Capability, Constraints, ContextPacket, EventSink, ExecStatus, ExecuteRequest,
    ExecuteResult, InlinedFile, LatencyProfile, QuotaModel, Role, Transport, Usage, Worker,
    WorkerDescriptor, WorkerEvent, WorkerFamily,
};

/// The on-disk blackboard directory name, relative to the working repo.
///
/// Single source of truth for a run (`CONTEXT.md` §12). Defined here so every
/// crate refers to the same constant.
pub const BLACKBOARD_DIR: &str = ".rinne";
