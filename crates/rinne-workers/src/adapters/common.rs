//! Shared machinery for harness adapters (`CONTEXT.md` §8).
//!
//! A harness worker is an autonomous agent: it gets a chunky self-contained
//! prompt and reads/edits the repo itself, so context is passed as a prompt plus
//! *pinned file paths*, never inlined contents (`CONTEXT.md` §8 behavioral
//! split, §12 context assembler).

use std::time::Duration;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use rinne_core::worker::{
    emit, EventSink, ExecStatus, ExecuteRequest, ExecuteResult, Usage, Worker, WorkerDescriptor,
    WorkerEvent,
};
use rinne_core::Result;

use crate::transport::subprocess::{self, LineMapper, SubprocessOutput, SubprocessSpec};

/// What an adapter extracts from a harness CLI's raw output. Parsers should be
/// defensive: on any doubt, fall back to the raw stdout as the result text so a
/// schema change in a beta CLI degrades gracefully (`CONTEXT.md` §21).
pub struct ParsedHarness {
    pub result: String,
    pub session_id: Option<String>,
    pub usage: Usage,
    /// The CLI signalled an error in its structured output, even if it exited 0.
    pub is_error: bool,
}

impl ParsedHarness {
    /// The trivial parse: use raw stdout, no session, no usage.
    pub fn raw(stdout: &str) -> Self {
        Self {
            result: stdout.trim().to_string(),
            session_id: None,
            usage: Usage::default(),
            is_error: false,
        }
    }
}

/// Build the argv for a harness invocation given the composed prompt and an
/// optional model selection.
pub type ArgsBuilder = fn(prompt: &str, model: Option<&str>) -> Vec<String>;

/// Defensive parser for harnesses that emit a single JSON result object: probe
/// common result fields, falling back to raw stdout on any surprise
/// (`CONTEXT.md` §21). Suitable for not-yet-pinned beta CLIs.
pub fn parse_generic_json(out: &SubprocessOutput) -> ParsedHarness {
    let pick = |v: &serde_json::Value| {
        v.get("result")
            .or_else(|| v.get("text"))
            .or_else(|| v.get("message"))
            .or_else(|| v.get("content"))
            .or_else(|| v.get("response"))
            .and_then(|x| x.as_str())
            .map(String::from)
    };
    for line in out.stdout.lines().rev() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(result) = pick(&v) {
                let session_id = v
                    .get("session_id")
                    .or_else(|| v.get("sessionId"))
                    .and_then(|x| x.as_str())
                    .map(String::from);
                let is_error = v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false);
                return ParsedHarness {
                    result,
                    session_id,
                    usage: Usage::default(),
                    is_error,
                };
            }
        }
    }
    ParsedHarness::raw(&out.stdout)
}

/// The trivial parser: the captured stdout is the result (for CLIs that print
/// plain text, e.g. Aider).
pub fn parse_raw(out: &SubprocessOutput) -> ParsedHarness {
    ParsedHarness::raw(&out.stdout)
}
/// Parse a harness CLI's captured output into normalized fields.
pub type OutputParser = fn(out: &SubprocessOutput) -> ParsedHarness;

/// A generic harness worker driven over the `subprocess-json` transport. Each
/// concrete CLI supplies its program name, argv builder, output parser, and an
/// optional line mapper for richer streaming.
pub struct HarnessAdapter {
    pub descriptor: WorkerDescriptor,
    pub program: String,
    pub build_args: ArgsBuilder,
    pub parse: OutputParser,
    pub line_mapper: LineMapper,
    /// Whether the prompt is piped via stdin (vs. passed as an argument).
    pub prompt_via_stdin: bool,
    pub default_timeout: Duration,
}

#[async_trait]
impl Worker for HarnessAdapter {
    fn descriptor(&self) -> &WorkerDescriptor {
        &self.descriptor
    }

    async fn execute(
        &self,
        request: ExecuteRequest,
        events: EventSink,
        cancel: CancellationToken,
    ) -> Result<ExecuteResult> {
        let prompt = compose_prompt(&request);
        let timeout = request
            .constraints
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(self.default_timeout);

        let model = request.constraints.model.as_deref();
        let (args, stdin) = if self.prompt_via_stdin {
            ((self.build_args)("", model), Some(prompt))
        } else {
            ((self.build_args)(&prompt, model), None)
        };

        // Retry transient failures (spawn errors, timeouts) once before giving
        // up — beta CLIs are flaky (`CONTEXT.md` §21). A cancelled run is not
        // retried.
        const MAX_ATTEMPTS: u32 = 2;
        let mut attempt = 0;
        let out = loop {
            attempt += 1;
            let spec = SubprocessSpec {
                program: self.program.clone(),
                args: args.clone(),
                workspace: request.workspace.clone(),
                stdin: stdin.clone(),
                timeout: Some(timeout),
            };
            match subprocess::run(spec, &events, &cancel, self.line_mapper).await {
                Ok(out) => {
                    let timed_out = matches!(out.status, ExecStatus::TimedOut);
                    if timed_out && attempt < MAX_ATTEMPTS && !cancel.is_cancelled() {
                        emit(&events, WorkerEvent::Message(format!(
                            "{} timed out — retrying ({attempt}/{MAX_ATTEMPTS})",
                            self.program
                        )));
                        continue;
                    }
                    break out;
                }
                Err(e) => {
                    if attempt < MAX_ATTEMPTS && !cancel.is_cancelled() {
                        emit(&events, WorkerEvent::Message(format!(
                            "{} failed to start ({e}) — retrying ({attempt}/{MAX_ATTEMPTS})",
                            self.program
                        )));
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                    return Err(e);
                }
            }
        };
        let parsed = (self.parse)(&out);

        // Reconcile the transport-level status with the parsed error flag.
        let status = match out.status {
            ExecStatus::Success if parsed.is_error => {
                ExecStatus::Failed("worker reported an error".into())
            }
            other => other,
        };

        let mut usage = parsed.usage;
        if usage.wall_ms == 0 {
            usage.wall_ms = out.wall_ms;
        }

        Ok(ExecuteResult {
            result: parsed.result,
            // Diff capture from the workspace is handled by the dispatcher in
            // Phase 3 (git-aware); adapters leave it None unless the CLI emits
            // one directly.
            file_diff: None,
            transcript: if out.stderr.is_empty() {
                out.stdout
            } else {
                format!("{}\n--- stderr ---\n{}", out.stdout, out.stderr)
            },
            status,
            usage,
            session_id: parsed.session_id,
        })
    }
}

/// Compose a harness prompt from the request: the instruction, any critique fed
/// back on loop-back, ambient steering, and the pinned file paths the worker
/// should read itself.
pub fn compose_prompt(request: &ExecuteRequest) -> String {
    let mut out = String::new();
    out.push_str(&request.instruction);

    if !request.context.prior_context.is_empty() {
        out.push_str("\n\n## Context\n");
        out.push_str(&request.context.prior_context);
    }

    if let Some(critique) = &request.context.critique {
        out.push_str("\n\n## Address this feedback from the previous attempt\n");
        out.push_str(critique);
    }

    if let Some(steer) = &request.constraints.steer {
        out.push_str("\n\n## Steering\n");
        out.push_str(steer);
    }

    if !request.context.pinned_paths.is_empty() {
        out.push_str("\n\n## Relevant files (read these)\n");
        for p in &request.context.pinned_paths {
            out.push_str("- ");
            out.push_str(&p.display().to_string());
            out.push('\n');
        }
    }

    out
}
