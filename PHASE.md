# RINNE — Phased Build Plan

This is the execution plan for building Rinne (the product specified in `CONTEXT.md`).
It turns the locked spec into an ordered sequence of phases, each with a goal, scope,
dependencies, deliverables, and a hard exit gate. **A phase is not "done" until its exit
gate passes.** Phases are ordered so that every phase can be built and tested against work
already proven in earlier phases.

Guiding rule from the spec: build the lowest-friction connection experience first, prove the
loop end-to-end against a mock worker before touching real CLIs, and never let the interface
land before the engine it drives exists.

---

## Naming / conventions (locked for all phases)

- Binary / command: `rinne`
- Blackboard directory: `.rinne/`
- Global config: `~/.config/rinne/config.toml`
- Per-project config: `.rinne/config.toml`
- Workspace crates (per `CONTEXT.md` §15):
  - `rinne-cli` — clap, ratatui, `@`-picker, slash commands, the interface
  - `rinne-core` — loop engine, scheduler, context assembler, blackboard, DAG types
  - `rinne-conductor` — prompt assembly, plan parsing, backend client
  - `rinne-workers` — `Worker` trait, the three transports, per-CLI adapters
  - `rinne-config` — figment + serde config, doctor probe
- Language: Rust, single static binary. Async: Tokio + `tokio-util` cancellation tokens.
- Errors: `thiserror` in libs, `anyhow` at the binary boundary.
- Logging: `tracing` to files in `.rinne/`, never polluting the TUI.

---

## Phase map (dependency order)

```
P0  Scaffold ───► P1  Config + doctor ───► P2  Workers + transports ──┐
                                                                      │
                          ┌───────────────────────────────────────────┘
                          ▼
                  P3  Core loop (serial, mock worker)
                          │
                          ▼
                  P4  Conductor (prompt → DAG)
                          │
                          ▼
                  P5  Loop control (eval gate, ratchet, loop-back,
                          │            stuck-detector, human eval, replan)
                          ▼
                  P6  Interactive TUI (4-region, stream, slash, @-picker, steer)
                          │
                          ▼
                  P7  Hardening (parallel read-only nodes, timeouts,
                                  retries, headless -p, resume polish)
                          │
                          ▼
                  V2  (out of scope here — see roadmap)
```

Each arrow is a hard dependency. P2 depends on P1's config/probe surface; P3 depends on P2's
`Worker` trait + mock worker; P4–P5 layer onto a working serial loop; P6 only renders state
that P3–P5 already produce; P7 hardens everything.

---

## Phase 0 — Workspace scaffold

**Goal:** A compiling Cargo workspace with the five crates wired together and CI-able.

**Scope**
- Create the workspace `Cargo.toml` and the five member crates from §15.
- Pin the locked dependencies (`tokio`, `tokio-util`, `clap`, `ratatui`, `crossterm`,
  `nucleo`, `notify`, `rusqlite`, `serde`, `serde_json`, `reqwest`, `figment`,
  `directories`, `thiserror`, `anyhow`, `tracing`, `tracing-subscriber`).
- `rinne-cli` exposes a `clap` derive skeleton for every command in §17
  (`rinne`, `-p`, `doctor`, `connect`, `status`, `resume`, `config`, `logs`) — all stubbed.
- Shared error and result types; `tracing` subscriber writing to `.rinne/`.

**Deliverables:** workspace builds; `rinne --help` prints all subcommands; `rinne doctor`
exits 0 with a "not implemented" notice.

**Exit gate:** `cargo build --workspace` and `cargo test --workspace` pass clean (empty tests
ok). `rinne --help` lists the full §17 surface.

---

## Phase 1 — `rinne-config` + `doctor` (lowest-friction connection first)

**Goal:** The user can install Rinne, run one command, and see exactly which workers exist,
their auth mode, and whether they're free or metered — with the Claude billing footgun caught.

**Scope**
- Config model (§18) parsed with `serde`, layered with `figment`: defaults ← global
  `~/.config/rinne/config.toml` ← per-project `.rinne/config.toml` ← env vars.
  `directories` crate for per-OS paths.
- `doctor` probe:
  - Detect installed harnesses on `PATH` (`claude`, `grok`, `opencode`, `codex`,
    `cursor-agent`, `agy`, `aider`) and smoke-test their headless surface.
  - Read configured API keys from the env vars named in config.
  - Report each worker's **auth mode** (`subscription` / `api-key` / `free`) and status.
  - **Claude footgun guard:** detect `ANTHROPIC_API_KEY` in env and warn that it overrides the
    subscription and bills the API account (§9, §21). Never silently bill.
  - Cache probe results so `doctor` doesn't re-run on every invocation (§18).
- `rinne connect <backend>`: run the native login or set a key, then re-probe.

**Dependencies:** P0.

**Deliverables:** `rinne doctor` prints a readable table of workers, auth mode, quota/status;
`rinne connect` round-trips; probe cache persists.

**Exit gate:** On a machine with at least one harness installed, `doctor` correctly classifies
its auth mode and the `ANTHROPIC_API_KEY` warning fires when that env var is set. Config
layering verified by test (project overrides global overrides default).

---

## Phase 2 — `rinne-workers`: contract, transports, first adapters, mock

**Goal:** A uniform `Worker` contract with two real transports, a handful of real adapters,
and a mock worker that the rest of the system can be tested against without burning tokens.

**Scope**
- The `Worker` trait per §8:
  - Capability descriptor (`code-edit`, `repo-aware`, `web-search`, `vision`, `long-context`,
    `tool-run`, `code-review`, `reasoning`, `writing`), auth mode, quota model
    (token-bucket refill params), latency profile, transport.
  - `execute(role, instruction, context_packet, workspace, constraints) -> { result, file_diff,
    transcript, status, usage }`.
  - Lifecycle: spawn, stream events, terminate; session continuation where supported.
- Transports behind one trait (§8, §14):
  - `subprocess-json`: `tokio::process` with piped stdio, parse structured output.
  - `http`: `reqwest` streaming, for API workers and the conductor backend.
  - (`acp` JSON-RPC transport deferred to V2 — not on the v1 critical path.)
- Adapters (the §19 first set):
  - Harness: **Claude Code** (`claude -p --output-format json`, native — never the ACP adapter,
    honors subscription), **Codex** (`codex exec`), **OpenCode** (`opencode run --format json`).
  - One **OpenAI-compatible API worker** over the shared `http` client.
- **Mock worker** (§14, §21): replays canned transcripts, deterministic, used to integration-test
  the loop without real CLIs.
- Normalize every adapter's output into Rinne's result schema.

**Dependencies:** P1 (auth mode / quota come from config + probe).

**Deliverables:** each adapter can run a trivial task and return a normalized result; the mock
worker replays a scripted transcript including a file diff and a usage report.

**Exit gate:** unit tests drive the mock worker through success, failure, and streaming-event
cases. At least one real harness adapter (Claude Code) executes a one-line task end-to-end and
returns a valid result schema with correct `usage` and auth mode honored.

---

## Phase 3 — `rinne-core` minimal: blackboard + serial loop on the mock

**Goal:** Execute a hand-written JSON DAG end-to-end, serially, against the mock worker, with
full persistence and clean resume. This is the spine; everything later hangs off it.

**Scope**
- Blackboard on disk (§12):
  ```
  .rinne/
    plan.json        the DAG
    state.db         SQLite (rusqlite): node statuses, iteration counts, budget ledger, quota buckets
    progress.md      human-readable run log
    context/         assembled context packets, cached
    artifacts/       named node outputs
    transcripts/     per-node worker transcripts
  ```
- DAG types (§10): node schema (`id`, `role`, `instruction`, `needs`, `prefer`, `depends_on`,
  `inputs`/`outputs` incl. special `diff`, `budget.iterations`, `evaluator`, `acceptance`,
  `test_ratchet`, `on_fail`, `checkpoint`) plus top-level `goal`, `budget`, `stop_when`.
- Scheduler (serial first, §19): walk DAG, find dep-satisfied nodes, resolve `needs` → concrete
  worker via live availability + quota token-buckets + user preference order. Enforce per-node
  and global budgets. Serialize any node that writes the workspace.
- Context assembler (§12, the hard component): build each node's packet from the blackboard.
  Harness worker → thin packet, **pin file paths**, let it read the repo. API worker → **read
  files and inline contents**. `@`-mentions pinned-or-inlined per family.
- Dispatcher: invoke worker via adapter, stream events, collect result, write outputs +
  transcript to blackboard, update `state.db`.
- Persistence + resume: workers are amnesiac; a killed run reconstructs from `state.db` and
  `rinne resume` continues exactly where it stopped.
- `rinne status`: render current run state (DAG + progress) to the terminal (plain, pre-TUI).

**Dependencies:** P2 (`Worker` trait + mock worker).

**Deliverables:** a checked-in sample `plan.json` runs to completion against the mock worker,
producing artifacts, transcripts, and a populated `state.db`; killing mid-run and `rinne resume`
finishes correctly.

**Exit gate:** integration test: multi-node DAG executes serially against the mock worker, all
artifacts/transcripts land, budgets are enforced, and a SIGKILL-then-resume run reaches the same
final state as an uninterrupted run. Context assembler verified to pin-for-harness vs inline-for-API.

---

## Phase 4 — `rinne-conductor`: prompt → real DAG

**Goal:** Turn a natural-language goal plus blackboard state into a valid JSON DAG, using a
cheap decoupled backend, so plans stop being hand-written.

**Scope**
- One OpenAI-compatible chat client over `reqwest` with configurable base URL covering every
  conductor backend (§7, §14): Cloudflare Workers AI, Groq, NVIDIA NIM, local Ollama, and the
  "cheapest installed harness as conductor" fallback.
- Prompt assembly (§7 inputs): goal, blackboard digest (plan, progress, repo summary, prior node
  outputs), resolved `@`-mentions, worker registry (capabilities, auth mode, live quota),
  constraints (budgets, preferences).
- Plan parsing: **JSONC-tolerant only at the conductor boundary** (model may emit comments /
  trailing commas), sanitize there, keep everything internal strict (§14). Validate against the
  DAG schema; reject and retry on malformed output.
- Key design decision honored (§7): conductor assigns role + capability `needs` + optional
  `prefer`; it does **not** hard-bind a worker. Scheduler still resolves concrete worker at
  dispatch.
- Granularity control: easy task → single node, no orchestration; hard task → multi-node graph.
- Graceful fallback to the next configured conductor backend when a free tier runs dry mid-run
  (§21).

**Dependencies:** P3 (the DAG the conductor emits must be the exact shape P3 executes).

**Deliverables:** `rinne -p "<task>"` (still plain output) produces a valid `plan.json` from a
real prompt and runs it through the P3 loop against the mock worker.

**Exit gate:** for a set of representative prompts (one trivial, one multi-step), the conductor
emits schema-valid DAGs that the scheduler accepts and executes; malformed-model-output is
sanitized or rejected-and-retried, never crashes the run; backend fallback exercised by test.

---

## Phase 5 — Loop control: evaluators, ratchet, loop-back, stuck-detector, human eval, replan

**Goal:** The full generator–evaluator loop with verification, the human-as-evaluator circuit
breaker, and conductor re-planning. This is what makes Rinne more than a dispatcher.

**Scope (§10, §11, §12)**
- Evaluator gate + loop controller:
  - `evaluator: tool` — run `acceptance.command`, require `must_exit` code (tests, lint,
    typecheck, build). Tool-grounded grading preferred.
  - `evaluator: ai` — adversarial review producing a critique artifact.
  - On pass: close node, advance. On fail: write critique artifact, fire `on_fail`
    (`loop_back(node, critique=...)`, `loop_with(node)`, `fixer`, `replan`) up to
    `budget.iterations`.
- **Test ratchet** (`test_ratchet: true`): refuse any diff that weakens or deletes tests.
- **Stuck-detector** (§11 trip conditions): N loops with no meaningful diff change / same failure;
  iteration or budget threshold with node still open; oscillating verdict (fix A breaks B); replanner
  returning a near-identical plan. On trip → inject a human-evaluator node, pause, surface stuck
  state + transcript + diff with a **sharp targeted question**.
- **Human as evaluator** (§11), all three forms:
  1. Explicit `evaluator: human` checkpoint node; user's words become `artifacts/eval-human.md`
     and flow into loop-back like an AI critique.
  2. Auto-triggered by the stuck-detector.
  3. Ambient: `/steer` / `/pause` captured as feedback to the active node.
  - Two rules: escalate sparingly with a sharp question; **async by default** — a human node parks
    the branch, independent branches keep going, `rinne resume` picks it up. A long unattended run
    degrades to "parked waiting on you," never "burned the budget ramming a wall."
- Checkpoint manager: `checkpoint: before|after` nodes pause for human approve/reject.
- Replanner hook: on repeated failure / wrong-approach verdict / genuinely new info, call the
  conductor to amend the DAG rather than grinding the same node.

**Dependencies:** P4 (replanner needs the conductor) + P3 (loop spine).

**Deliverables:** a DAG with a generator + tool-evaluator + AI-evaluator runs loop-backs to a
passing state against the mock worker; a scripted "wall-ramming" transcript trips the
stuck-detector and parks on a human-evaluator node; `rinne resume` injects the human critique and
the loop proceeds.

**Exit gate:** integration tests against the mock worker cover: (a) tool-eval fail → loop_back →
pass; (b) test ratchet blocks a test-deleting diff; (c) stuck-detector trips on the documented
signature and parks async without burning budget; (d) human critique artifact flows into the next
iteration; (e) replanner amends the DAG on a wrong-approach verdict.

---

## Phase 6 — `rinne-cli` interactive TUI

**Goal:** The harness interface the user lives in — the four-region layout, live streaming,
slash commands, `@`-picker, and steer/approve — rendering the state the engine already produces.

**Scope (§6, §14)**
- `ratatui` + `crossterm`, four regions rendered mid-run:
  1. **Plan tree** — the DAG with per-node status, assigned worker, live cost.
  2. **Stream pane** — the active worker's events.
  3. **Conductor narration** — one line explaining each routing decision (routing is always
     narrated, never hidden — the deliberate opposite of opaque orchestration, §3).
  4. **Prompt line** — doubles as steer/approve control and hosts the `@`-picker.
- Slash commands (§6): `/plan`, `/workers`, `/steer <text>`, `/approve`, `/reject`, `/pause`,
  `/resume`, `/budget`, `/route n2 grok`, `/logs`.
- **`@`-file picker** (Power 1, §6): `nucleo` fuzzy matcher over a file index rendered as a
  ratatui overlay. Index on session start, keep current with `notify`. Supports `@path`, `@dir/`,
  `@glob`. Resolve to explicit file references (paths, not pasted bodies); validate against repo;
  expand globs; hand the conductor a clean resolved list, never raw `@` syntax. **Guardrail:** a
  glob expanding past threshold (e.g. 400 files) warns and asks the user to narrow *before* the
  conductor sees it.
- Wire live events from the dispatcher/loop to the panes via `tokio-util` cancellation tokens for
  `/pause`, budget kills, stuck-detector aborts.

**Dependencies:** P5 (the TUI surfaces evaluator gates, checkpoints, stuck pauses, narration —
all of which exist only after P5).

**Deliverables:** `rinne` opens the live REPL/TUI; a real run renders the plan tree updating,
streams worker events, narrates routing, accepts `/steer` and `/approve`, and drives the
`@`-picker with the glob guardrail.

**Exit gate:** a full interactive run against the mock (and one real harness) shows: live node
status transitions, streaming output, a narration line per routing decision, a working `@`-picker
with glob warning, and steer/approve/pause/resume all affecting the running loop.

---

## Phase 7 — Hardening: parallelism, timeouts, retries, headless, resume polish

**Goal:** Make Rinne robust enough to trust on long real tasks across flaky beta CLIs.

**Scope (§14, §19, §21)**
- Parallelize read-only nodes (e.g. the two evaluators); keep serialized any node that writes the
  workspace. (Git-worktree isolation for parallel writers stays V2.)
- Harden adapters: defensive timeouts (Cursor `-p` is known to hang), retries, version pinning,
  graceful degradation on beta instability (Grok Build, Cursor paths).
- Headless mode polish: `rinne -p "<task>"` emits clean structured output so Rinne is itself
  scriptable and can be a worker inside another system (§6).
- Resume/persistence polish across all the new node types; budget ledger and quota-bucket accuracy
  under concurrency; conductor backend fallback under mid-run free-tier exhaustion.
- `rinne logs` query surface over `state.db` for trajectory inspection (local only).

**Dependencies:** P6 (hardening the full interactive system).

**Deliverables:** read-only nodes run concurrently; a hung/slow CLI is timed out and retried or
routed around; `-p` structured output is stable; resume works across every node type.

**Exit gate:** a long multi-node run with at least two real harnesses completes with concurrent
read-only evaluators, survives an injected adapter hang (timeout + reroute), and resumes cleanly
after a kill. `rinne -p` output validates against the documented structured schema.

---

## Out of scope for this plan (V2+ per §20)

Tracked here only so we don't accidentally pull them into v1:

- More adapters: Cursor CLI, Grok Build, Aider, Antigravity.
- `acp` JSON-RPC transport for workers that support it cleanly.
- Git-worktree isolation for parallel *writing* branches.
- Learned router trained offline on logged trajectories (routing learned before planning,
  because the reward signal is clean).
- Richer DAG (parallel branches).
- V3: pluggable conductor prompts / role packs, community adapter ecosystem, optional opt-in
  shared trajectory corpus.

---

## Cross-cutting invariants (hold in every phase)

These come straight from §3 "Locked principles" and §21 "Risks" and must never regress:

- **Local only, no telemetry, no data fetching.** Only network calls are configured worker calls
  and the conductor backend.
- **Rinne holds no credentials.** Each adapter invokes its worker the native way that honors the
  user's existing login; isolate auth handling per adapter to adapt to shifting vendor ToS.
- **Routing is always narrated**, never hidden.
- **No silent billing** — the Claude `ANTHROPIC_API_KEY` footgun is surfaced and warned at every
  relevant point (`doctor` and the adapter).
- **The blackboard is the single source of truth**; workers are amnesiac; any run is reconstructible
  from `.rinne/`.
- **No over-orchestration** — easy tasks get one node and one worker.
- **Test against the mock worker first** — never gate a phase on a flaky real beta CLI.

---

## Suggested verification spine (carried forward each phase)

- Keep a small library of canned mock-worker transcripts (success, fail-then-fix, wall-ramming,
  test-deleting diff) and grow it as features land.
- Every phase from P3 on adds at least one integration test that runs a DAG end-to-end against the
  mock worker and asserts on blackboard state.
- `doctor`, config layering, and the Claude footgun warning get regression tests from P1 onward.
