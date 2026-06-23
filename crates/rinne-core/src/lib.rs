//! `rinne-core` — the loop engine, scheduler, context assembler, blackboard,
//! and DAG types.
//!
//! Phase 0 establishes only the shared error/result types and the crate
//! skeleton. The blackboard, DAG types, scheduler, dispatcher, and context
//! assembler land in Phase 3 (`CONTEXT.md` §12, `PHASE.md` P3).

pub mod error;
pub mod worker;

pub use error::{Result, RinneError};
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
