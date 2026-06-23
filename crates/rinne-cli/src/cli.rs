//! Command-line surface for `rinne` (`CONTEXT.md` §17).
//!
//! Phase 0 defines the full command tree with `clap` derive. Every command is
//! stubbed; the handlers land in their respective phases.

use clap::{Parser, Subcommand};

/// Rinne — local, terminal-first AI orchestration.
///
/// With no subcommand, `rinne` opens the interactive REPL/TUI. With `-p`, it
/// runs one shot headless with structured output.
#[derive(Debug, Parser)]
#[command(name = "rinne", version, about, long_about = None)]
pub struct Cli {
    /// Run one shot headless on the given task and emit structured output,
    /// instead of opening the interactive TUI (`shoal -p` in the spec).
    #[arg(short = 'p', long = "prompt", value_name = "TASK", global = true)]
    pub prompt: Option<String>,

    /// Increase log verbosity (repeatable). Logs go to a file in `.rinne/`,
    /// never to the TUI.
    #[arg(short = 'v', long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Option<Command>,
}

/// The `rinne` subcommands (`CONTEXT.md` §17).
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Detect and report backends, auth mode, and quota.
    Doctor,

    /// Run a native login or set a key for a backend, then re-check.
    Connect {
        /// The backend to connect (e.g. `claude`, `codex`, `openai`).
        backend: String,
    },

    /// Show the state of the current run (DAG, progress).
    Status,

    /// Resume an interrupted or parked run.
    Resume,

    /// Edit backends, conductor backend, and preferences.
    Config,

    /// View trajectory logs (local only).
    Logs,
}
