# SHOAL

Open-source, local, terminal-first AI orchestration tool.

This document is the build specification. It is self-contained and decisive. Every choice here is locked unless explicitly marked as a v2 deferral or an open item. Use it as working context for implementation.

---

## 1. What Shoal is

Shoal is a terminal harness that the user talks to directly. The user prompts Shoal with what they want done. Shoal plans the work, then distributes it across the AI coding CLIs installed on the machine, across model APIs, or across any mix of both, and drives the work to completion through an iterative loop with verification.

The user never opens Claude Code, Grok Build, Codex or OpenCode themselves. They live in Shoal. Shoal reaches down to those tools as workers.

The orchestration idea is Fugu's (a conductor composing a pool of models into ad-hoc teams). The execution model is loop engineering (a durable generator-evaluator loop with state on disk). Shoal unifies them: the conductor composes a per-task team, the loop drives long-running work and verification, and the filesystem is the shared substrate that lets heterogeneous workers collaborate.

## 2. Who it is for and why

- Developers paying for one or more coding-agent subscriptions with no API budget. They get multi-model orchestration out of capacity they already bought.
- Developers who prefer raw API access and want a clean local orchestrator over it.
- Anyone mixing both, for example a cheap API model as evaluator and a subscription harness as generator.

The point: the user already pours money into these subscriptions. Shoal extracts orchestration value from that spend without charging again and without metering, except where the user deliberately uses a paid API worker.

## 3. Locked principles

- Local only. Runs on the user's machine. No hosted component, ever.
- Open source. No pricing, no tiers, no accounts.
- No telemetry, no data fetching. The only network calls are the worker calls and the conductor backend the user configured.
- Shoal holds no credentials. Each worker is already installed and logged in by the user the normal way. Shoal invokes tools the user already set up and reads back what they produce.
- Terminal-first. No app, no web UI. A CLI that controls other CLIs.
- Routing is always narrated, never hidden. This is the deliberate opposite of Fugu's opaque orchestration.
- Ship fast. The competitive edge is execution velocity: stand on harnesses that already exist rather than training a model.

## 4. Non-goals

- Not a SaaS and not a hosted service.
- Not a subscription multiplexer. Never resells, pools or proxies one user's subscription for anyone else.
- Not a trained orchestrator in v1. The conductor is prompted. A learned router comes later, trained offline on logged trajectories, never on a free inference tier.
- Not an IDE driver. No GUI screen automation. A harness must expose a headless surface to be supported.
- Not trying to beat Fugu on raw answer quality. The pitch is Fugu-class orchestration at zero marginal cost, locally.

## 5. System architecture

```
                          user
                           |  prompt, approvals, steering, @-mentions
                           v
+----------------------------------------------------------+
|                 SHOAL HARNESS INTERFACE                  |
|     REPL / TUI  +  one-shot (shoal -p)  +  slash cmds    |
|     @-file picker  +  live plan tree + stream + steer    |
+----------------------------------------------------------+
        |                    ^                    ^
   goal + state         live events       approvals / human eval
        v                    |                    |
+----------------------------------------------------------+
|  CONDUCTOR (prompted, cheap decoupled backend)           |
|  goal + blackboard digest + worker registry -> JSON DAG  |
|  re-plans on failure, escalation or new info             |
+----------------------------------------------------------+
        |  emits / amends plan.json
        v
+----------------------------------------------------------+
|  LOOP ENGINE                                             |
|  scheduler | context assembler | dispatcher |            |
|  evaluator gate + loop-back | stuck-detector |           |
|  checkpoint manager | replanner hook | persistence       |
+----------------------------------------------------------+
        |  reads/writes                 |  dispatch
        v                               v
+--------------------+        +---------------------------+
|  BLACKBOARD        |        |  WORKER ADAPTERS          |
|  .shoal/ + repo    |<------>|  harness CLIs  |  APIs    |
|  single source     |        |  claude/grok/  |  anthropic|
|  of truth          |        |  opencode/codex|  openai...|
+--------------------+        +---------------------------+
```

Flow in one sentence: the user prompts Shoal, the conductor turns the prompt plus current state into a JSON DAG, the loop engine schedules that DAG across workers through the blackboard, evaluators (AI, tool or human) gate each result, and the conductor re-plans when something fails, until the goal is met or the budget runs out.

## 6. The Shoal Harness Interface

### Modes

- Interactive: `shoal` opens a REPL/TUI the user lives in.
- Headless: `shoal -p "task"` runs one shot with structured output, so Shoal is itself scriptable and can be a worker inside another system.

### Interactive layout

Four regions, rendered mid-run:

```
 shoal · add Redis rate limiting to the Express API and prove it works
 ───────────────────────────────────────────────────────────────────────
 PLAN                                        budget 11m left · 2/5 nodes
 ├─ ✔ n1 design limiter            api:claude-sonnet        1.2k tok
 ├─ ⠋ n2 implement                 hns:claude-code  ······· editing 4 files
 ├─ ○ n3 run tests (eval)          tool:npm-test
 ├─ ○ n4 adversarial review (eval) api:gpt-5.5
 └─ ○ n5 summarize + checkpoint    hns:opencode
 ───────────────────────────────────────────────────────────────────────
 STREAM  n2 · claude-code
   reading src/middleware/, package.json
   adding src/middleware/rateLimit.ts
   wiring limiter into app.ts
 ───────────────────────────────────────────────────────────────────────
 conductor: routed implement to claude-code (repo-aware, subscription).
            test + review split across two different models on purpose.
 ───────────────────────────────────────────────────────────────────────
 shoal› /steer keep the window at 100 req/min        [tab] approve  [esc] pause
```

1. Plan tree: the DAG with per-node status, assigned worker and live cost.
2. Stream pane: the active worker's events.
3. Conductor narration: one line explaining routing, so it is never a black box.
4. Prompt line: doubles as the steer/approve control, and hosts the `@`-picker.

### Slash commands

- `/plan` show or re-show the DAG
- `/workers` list available workers with auth mode and quota
- `/steer <text>` inject guidance into the running node
- `/approve` `/reject` at checkpoints and human-evaluator gates
- `/pause` `/resume`
- `/budget` adjust limits live
- `/route n2 grok` override a worker assignment
- `/logs` open transcripts

### @-file mentions (Power 1)

`@` in the prompt line triggers a fuzzy file picker over the repo. Supports `@path/to/file`, `@dir/` for a whole folder and `@glob` like `@src/**/*.ts`.

Mechanics:
- `@` resolves to explicit file references, not pasted file bodies.
- The interface validates each reference against the repo, expands globs, and hands the conductor a clean resolved list, never raw `@` syntax:
  ```jsonc
  { "goal": "fix the race condition", "mentioned": ["src/middleware/rateLimit.ts", "src/lib/redis.ts"] }
  ```
- Mentioned files are a pinned context anchor, distinct from what a worker discovers on its own. They flow per worker family (see the context assembler): pinned as paths for harness workers, inlined as contents for API workers.
- Guardrail: if a glob expands past a threshold (for example 400 files) the interface warns and asks the user to narrow before the conductor ever sees it, so a careless glob cannot detonate context cost.

The picker is indexed once on session start and kept current with a filesystem watcher (see tech stack).

## 7. The Conductor

The brain that plans and routes. It does no work itself. It runs prompted on a cheap decoupled backend so planning never burns the quota meant for real work.

### Backend

Configurable, all OpenAI-compatible so one client covers them:
- Cloudflare Workers AI, model `@cf/moonshotai/kimi-k2.7-code`. Free tier is 10,000 neurons per day, resets daily. Plenty for short planning calls.
- Groq, for fast planning, roughly 30 RPM and 1,000 requests/day free on larger models.
- NVIDIA NIM, trial credits only (1,000 to 5,000, 40 RPM). Evaluation, not a default.
- Local via Ollama, OpenAI-compatible, fully offline.
- Fallback: the user's cheapest installed harness as conductor when nothing else is configured.

### Inputs and output

Inputs on every invocation: the goal, a digest of the blackboard (plan, progress, repo summary, prior node outputs), the resolved `@`-mentions, the worker registry (which workers exist, capabilities, auth mode, current quota state) and constraints (budgets, preferences).

Output: a JSON DAG, or an amendment to the existing DAG.

### Key design decision

The conductor assigns each node a role plus a capability requirement plus an optional preferred worker. It does not hard-bind a concrete worker. The scheduler resolves the concrete worker at dispatch time from live availability and quota. A node does not die because its preferred worker is rate limited.

### When it is invoked

- Initial planning.
- Re-planning when an evaluator rejects an approach or a node fails repeatedly.
- Escalation when the loop reports it is stuck.

It also decides granularity. An easy task gets one node and one worker with no orchestration. A hard task gets a multi-node graph with evaluator loops. No over-orchestration.

### Phase two

Replace the worker-selection step with a learned router trained offline on logged trajectories (chosen worker, role, eval result, cost, latency). Planning stays prompted longer because its reward signal is murkier. Routing is learned first because the reward is clean: did the chosen worker's output pass the evaluator, at what cost and latency.

## 8. The Worker

Anything that takes a subtask and does it. Two families, one contract.

### The contract

Every worker exposes:
- A capability descriptor: what it can do (`code-edit`, `repo-aware`, `web-search`, `vision`, `long-context`, `tool-run`, `code-review`, `reasoning`, `writing`), its auth mode (`subscription`, `api-key`, `free`), a quota model (token-bucket refill params), a latency profile and its transport.
- `execute(role, instruction, context_packet, workspace, constraints) -> { result, file_diff, transcript, status, usage }`.
- A lifecycle: spawn, stream events, terminate, with session continuation where the underlying tool supports it for cheap intra-worker context.

### Two families

- Harness workers wrap the native headless call that honors the existing login: `claude -p --output-format json`, `grok -p --output-format json`, `opencode run --format json`, `codex exec`, `cursor-agent -p`. The adapter normalizes output into Shoal's result schema. The worker reads the repo itself.
- API workers are direct model calls on the user's own key. Stateless, full context control.

### Behavioral split (this drives dispatch and context assembly)

- A harness worker is an autonomous agent. Give it a chunky self-contained task ("implement the limiter so these tests pass") and let it do its own file reading and editing.
- An API worker is a raw model. Give it a precise instruction with context assembled inline, get back one focused result.

The scheduler picks the family to fit the node. The context assembler shapes the packet differently for each.

### Transports (three, behind one trait)

- `subprocess-json`: `tokio::process` with piped stdio, parse structured output.
- `acp`: thin JSON-RPC 2.0 over stdio client, for workers that support ACP cleanly. Optional, not mandatory. ACP is one transport, not the point. The interface that matters is Shoal's, facing the user.
- `http`: `reqwest` streaming, for API workers and the conductor backend.

## 9. Auth model

Shoal authenticates nothing and holds no credentials. Each worker owns its own auth, set up by the user before Shoal runs. The only rule that touches Shoal: how a tool is invoked decides which auth it uses, so each adapter calls its worker the native way that honors the login the user already has.

Concretely:
- Claude: drive the real `claude` CLI, which respects the Pro/Max subscription login. Do not use the Claude ACP adapter, which forces an Anthropic API key. Footgun to guard against: if `ANTHROPIC_API_KEY` is set in the environment it overrides the subscription and bills the API account, and non-interactive `-p` uses the key when present. The adapter must surface and warn on this, never silently bill.
- Grok: `grok -p` honors the `grok login` subscription token. ACP path honors it too.
- Codex: ChatGPT-login or API key, per the user's setup.
- OpenCode: provider config the user set in OpenCode.
- API workers: always the user's own key, always metered. No subscription option exists for raw API. This is the explicit trade against harness workers.

`doctor` detects and displays each worker's auth mode (subscription, api-key, free) so the user always knows which workers are free and which are metered, and never gets a surprise invoice.

## 10. The JSON DAG

The plan, and the thing the loop executes. Nodes plus dependency edges. Generator-evaluator is not a separate structure, it is a pair of nodes with a loop-back policy. This is exactly how Fugu-style composition and the loop unify.

### Node schema and example

```jsonc
{
  "goal": "Add Redis-backed rate limiting to the Express API and prove it works",
  "blackboard": ".shoal/",
  "mentioned": ["src/middleware/rateLimit.ts", "src/lib/redis.ts"],
  "budget": { "minutes": 30, "max_total_iterations": 20 },
  "stop_when": "n4 passes and n3 passes",
  "nodes": [
    {
      "id": "n1",
      "role": "planner",
      "instruction": "Design the limiter: middleware shape, Redis keying, window strategy.",
      "needs": ["reasoning", "repo-aware"],
      "prefer": "api:claude-sonnet",
      "depends_on": [],
      "outputs": ["artifacts/design.md"],
      "budget": { "iterations": 1 }
    },
    {
      "id": "n2",
      "role": "generator",
      "instruction": "Implement the limiter per artifacts/design.md. Wire it into app.ts.",
      "needs": ["code-edit", "repo-aware"],
      "prefer": "harness:claude-code",
      "depends_on": ["n1"],
      "inputs": ["artifacts/design.md"],
      "outputs": ["diff", "artifacts/impl-notes.md"],
      "budget": { "iterations": 4 },
      "on_fail": "loop_with(n3)"
    },
    {
      "id": "n3",
      "role": "evaluator",
      "evaluator": "tool",
      "instruction": "Run the suite and lint. Report failures concretely.",
      "needs": ["tool-run"],
      "prefer": "tool:npm-test",
      "depends_on": ["n2"],
      "acceptance": { "command": "npm test && npm run lint", "must_exit": 0 },
      "test_ratchet": true,
      "on_fail": "loop_back(n2, critique=artifacts/eval-n3.md)"
    },
    {
      "id": "n4",
      "role": "evaluator",
      "evaluator": "ai",
      "instruction": "Adversarial review of the diff for correctness and edge cases. Fresh eyes.",
      "needs": ["code-review", "long-context"],
      "prefer": "api:gpt-5.5",
      "depends_on": ["n3"],
      "outputs": ["artifacts/review.md"],
      "on_fail": "loop_back(n2, critique=artifacts/review.md)"
    },
    {
      "id": "n5",
      "role": "synthesizer",
      "instruction": "Summarize what changed, update progress.md.",
      "needs": ["writing"],
      "prefer": "harness:opencode",
      "depends_on": ["n4"],
      "checkpoint": "before",
      "outputs": ["artifacts/summary.md"]
    }
  ]
}
```

### Field reference

- `id`, `role` (`planner`, `generator`, `evaluator`, `synthesizer`, `fixer`), `instruction`.
- `needs`: capability requirements the scheduler resolves to a worker.
- `prefer`: optional preferred worker, soft, overridable at dispatch.
- `depends_on`: dependency edges.
- `inputs` / `outputs`: named blackboard artifacts, plus the special `diff`.
- `budget.iterations`: per-node loop cap.
- `evaluator`: `ai`, `tool` or `human` (see Power 2).
- `acceptance`: for tool evaluators, a command and required exit code.
- `test_ratchet`: blocks any diff that weakens or deletes tests.
- `on_fail`: `loop_back(node, critique=...)`, `loop_with(node)`, `fixer`, or `replan`.
- `checkpoint`: `before` or `after`, a human gate.

## 11. Human as evaluator (Power 2)

An AI evaluator grading an AI generator shares blind spots and mental model, so the pair can agree the wall is the door and keep ramming it. The loop looks healthy from inside while making no real progress. The reliable circuit breaker is a different kind of evaluator, and the user is the highest-signal one available because they hold context Shoal does not: product intent, the thing that looked off, the unwritten constraint.

A checkpoint is a gate the user passes or fails. A human-evaluator node is a checkpoint that also captures the user's critique as a blackboard artifact, which flows into the loop-back exactly like an AI evaluator's critique. The user's words become `artifacts/eval-human.md` and the generator gets them on the next iteration. The point is that user knowledge enters the loop as actionable feedback, not a yes/no.

### Three ways it appears

1. Explicit, placed by the user or the conductor on high-stakes nodes:
   ```jsonc
   {
     "id": "n4",
     "role": "evaluator",
     "evaluator": "human",
     "instruction": "Is the limiter behaving the way you actually want before we build on it?",
     "depends_on": ["n3"],
     "on_fail": "loop_back(n2, critique=artifacts/eval-human.md)"
   }
   ```

2. Triggered automatically by the stuck-detector. This is what solves the error-loop problem. Trip conditions:
   - a generator-evaluator pair loops N times with no meaningful diff change, the same failure repeating
   - iteration or budget crossing a threshold with the node still open
   - an evaluator verdict oscillating (fix A breaks B, fix B breaks A)
   - the replanner returning a near-identical plan, meaning it is out of ideas

   On trip, the run pauses, surfaces the stuck state with the relevant transcript and diff, and asks the targeted question the AI could not answer:
   ```
    ⚠ stuck · n2↔n3 looped 3× · same failure: "window resets early under burst"
    ───────────────────────────────────────────────────────────────────────
    Tried: TTL on first hit, sliding log, fixed window. All fail the burst test.
    The agents keep assuming a single Redis node.

    Your call:
      /steer <detail>     give the missing constraint
      /approve            accept current state, move on
      /reject restart     throw out this approach, replan from scratch
   ```
   The user's one line ("it's a Redis cluster, keys aren't co-located, use a hash tag") lands as the critique artifact, the generator picks it up, and the wall turns out to have been a door.

3. Ambient. At any moment the user can `/steer` or `/pause`, and the input is captured as feedback to the active node. Human evaluation is an interrupt, never blocked on a scheduled checkpoint.

### Two rules so it stays useful, not nagging

- Escalate sparingly with a sharp question. The user is expensive and high-signal, so Shoal pulls them in only when genuinely stuck or at a real fork, never to rubber-stamp routine steps, and always with the specific question and context attached, never "how's this looking."
- Async by default. If the user is away, a human-evaluator node parks the branch, independent branches keep going, and the run waits at that node rather than failing. `shoal resume` picks it up. A long unattended run degrades to "parked waiting on you," never to "burned the budget ramming a wall."

## 12. The Loop Engine

### Blackboard (single source of truth on disk)

```
.shoal/
  plan.json          the DAG (document, conductor emits and amends)
  state.db           SQLite: node statuses, iteration counts, budget ledger, quota buckets
  progress.md        human-readable run log
  context/           assembled context packets, cached
  artifacts/         named node outputs (design.md, review.md, eval-human.md, ...)
  transcripts/       per-node worker transcripts
<repo>               the actual work product
```

### Components

- Scheduler: walks the DAG, finds nodes whose deps are satisfied, resolves each node's `needs` to a concrete worker using live availability, quota token-buckets and the user's preference order, then dispatches. Runs independent branches concurrently. Enforces per-node and global budgets.
- Context assembler: builds each node's context packet from the blackboard. This is the hardest component, because no two workers share a context window. For a harness worker it writes a thin packet and pins file paths, letting the worker read the repo itself. For an API worker it reads the files and inlines contents, because the model sees only what is sent. `@`-mentions are pinned or inlined here per family. Get this right or workers talk past each other.
- Dispatcher: invokes the worker through its adapter, streams events to the interface, collects the result, writes outputs and transcript to the blackboard, updates state.
- Evaluator gate and loop controller: on an evaluator node it grades, tool-grounded where possible (run tests, lint, typecheck, build). On pass the node closes and the graph advances. On fail it writes a critique artifact and triggers `on_fail`, up to the node's iteration budget. The test ratchet refuses any diff that weakens or deletes tests.
- Stuck-detector: watches for the wall-ramming signature (see Power 2) and escalates to a human-evaluator node instead of burning budget.
- Replanner hook: on repeated failure, a wrong-approach verdict, or genuinely new information, calls the conductor to amend the DAG rather than grinding the same node.
- Checkpoint manager: nodes flagged `checkpoint` pause and surface to the user for approval before or after.
- Persistence and resume: everything lives in `.shoal/`, workers are amnesiac, so a killed run reconstructs from `state.db` and `shoal resume` continues exactly where it stopped.

### Run lifecycle

```
conductor -> plan.json
  -> [optional checkpoint: approve plan]
  loop while not stop_when and budget remains:
    ready = nodes with deps satisfied
    for each ready node (parallel where independent, serialized where it writes the workspace):
        worker  = scheduler.resolve(node.needs, quota, prefer)
        packet  = context_assembler.build(node)
        result  = dispatcher.run(worker, node, packet)   -> stream to UI
        write result -> blackboard
        if node.role == evaluator:
            pass -> advance
            fail -> on_fail (loop_back / fixer / replan)
        if stuck-detector trips -> inject human-evaluator node -> pause
        if node.checkpoint -> pause for human
        if wrong-approach or stuck -> replanner -> amend DAG
  -> summarize: diff, artifacts, what changed
```

## 13. Routing and scheduling

The routing objective is quota and rate limits, not dollars. Model each backend as a token bucket with a refill rate (a subscription's rate-limit window, an API tier's RPM, a free tier's daily reset). The scheduler spreads load to avoid hitting limits, respects user preference (prefer subscription over API, or pin a backend to a role) and lets the conductor pick "single worker, no team" for easy nodes so simple tasks are not over-orchestrated.

## 14. Tech stack (locked)

- Language: Rust. Single static binary, no runtime to install.
- Async runtime: Tokio, plus `tokio-util` for cancellation tokens (used by `/pause`, budget kills, stuck-detector aborts).
- Worker transport: `tokio::process` for subprocess workers, a hand-written thin JSON-RPC 2.0 over stdio client for ACP workers (a couple hundred lines, full control over session lifecycle, no heavy framework), `reqwest` streaming for HTTP and API workers.
- CLI: `clap` with derive.
- TUI: `ratatui` with `crossterm` backend. Four-region layout, live plan tree, streaming pane, prompt line.
- @-picker: `nucleo` fuzzy matcher over a file index, rendered as a `ratatui` overlay. File list indexed on session start, kept current with `notify`.
- Persistence: plain files for human-facing artifacts and `plan.json`, SQLite via `rusqlite` for machine state (node statuses, iteration counts, budget ledger, quota buckets). Atomic updates, clean resume, query surface for `shoal logs`.
- Serialization: `serde` plus `serde_json` everywhere. JSONC-tolerant parsing only at the conductor boundary (model may emit comments or trailing commas), sanitize there, keep everything internal strict.
- Model clients: one OpenAI-compatible chat client over `reqwest` with a configurable base URL, covering every conductor backend and every OpenAI-format API worker. Thin specific clients only for Anthropic and Google native shapes. No LLM-abstraction crate.
- Config: `~/.config/shoal/config.toml` parsed with `serde`, layered with `figment` over env vars and a per-project `.shoal/config.toml`. `directories` crate for per-OS paths.
- Errors: `thiserror` in library crates, `anyhow` at the binary boundary.
- Logging: `tracing` plus `tracing-subscriber`, file output into `.shoal/`, never polluting the TUI.
- Testing: a mock worker that replays canned transcripts. Integration-test the loop engine against it to exercise loop-backs, stuck-detection and resume without burning tokens or depending on a flaky beta CLI.

Resolved decision points:
- File picker indexes on session start and watches with `notify`.
- Parallelism in v1: serialize any node that writes the workspace, parallelize read-only nodes (for example the two evaluators). Git-worktree isolation per branch is v2.
- Stay framework-free. Shoal is the orchestrator, so no agent-framework or LLM-orchestration crate.

## 15. Workspace layout

```
shoal/
  crates/
    shoal-cli/         clap, ratatui, the @ picker, slash commands, the interface
    shoal-core/        loop engine, scheduler, context assembler, blackboard, DAG types
    shoal-conductor/   prompt assembly, plan parsing, backend client
    shoal-workers/     Worker trait, the three transports, per-CLI adapters
    shoal-config/      figment + serde config, doctor probe
  Cargo.toml           workspace
```

## 16. Harness support matrix

| Harness | Headless surface | Auth path | v1 priority |
|---|---|---|---|
| OpenCode | `opencode run --format json`, `serve`, `acp`, SDK | provider config | First |
| Claude Code | `claude -p --output-format json` (native, honors subscription; not the ACP adapter) | subscription login | First |
| Codex CLI | `codex exec` | ChatGPT login or API key | First |
| Cursor CLI | `cursor-agent -p --output-format json --force` | subscription | Second, add timeouts (known `-p` hangs) |
| Grok Build | `grok -p --output-format json`, ACP | `grok login` subscription or `XAI_API_KEY` | Second, early beta, high-tier gated |
| Aider | git-native, scriptable | provider keys | Second |
| Antigravity | `agy --print` / `--continue` (replaces Gemini CLI for free users from 2026-06-18) | Google OAuth | Second |

API workers: Anthropic and Google via thin native clients, everything else via the one OpenAI-compatible client (OpenAI, OpenRouter, local Ollama).

## 17. CLI surface

```
shoal                        # interactive REPL/TUI
shoal -p "<task>"            # one-shot headless, structured output
shoal doctor                 # detect and report backends, auth mode, quota
shoal connect <backend>      # run native login or set a key, then re-check
shoal status                 # state of the current run (DAG, progress)
shoal resume                 # resume an interrupted or parked run
shoal config                 # edit backends, conductor backend, preferences
shoal logs                   # view trajectory logs (local)
```

## 18. Config example

```toml
[conductor]
backend = "cloudflare"            # cloudflare | groq | nvidia | local | harness
model   = "@cf/moonshotai/kimi-k2.7-code"

[loop]
max_iterations_per_node = 8
global_budget_minutes   = 120
test_ratchet            = true
stuck_loop_threshold    = 3       # identical-failure loops before human escalation

[preferences]
prefer = "harness"                # harness | api | balanced
roles  = { evaluator = "api:gpt-5.5", generator = "harness:claude-code" }

[backends.harness]
enabled = ["claude-code", "codex", "opencode"]

[backends.api.anthropic]
key_env = "ANTHROPIC_API_KEY"

[backends.api.openai]
key_env = "OPENAI_API_KEY"
```

Probe results are cached so `doctor` does not rerun on every invocation.

## 19. Build order

1. `shoal-config` plus `doctor`: probe PATH, smoke-test installed harnesses, read configured API keys, report auth mode and status. Lowest-friction connection experience first.
2. `shoal-workers`: the `Worker` trait, the `subprocess-json` and `http` transports, adapters for Claude Code, Codex, OpenCode and one OpenAI-compatible API worker. Plus the mock worker for tests.
3. `shoal-core` minimal: blackboard, DAG types, scheduler (serial first), dispatcher, context assembler, persistence and resume. Run a hand-written DAG end to end against the mock worker.
4. `shoal-conductor`: prompt assembly, the OpenAI-compatible backend client, plan parsing. Generate a real DAG from a prompt.
5. Loop control: evaluator gate, tool evaluators, test ratchet, loop-back, human-evaluator nodes, stuck-detector, replanner hook, checkpoints.
6. `shoal-cli` interactive: ratatui four-region layout, streaming, slash commands, `@`-picker, steer/approve.
7. Parallelize read-only nodes. Harden adapters with timeouts and retries.

## 20. Roadmap beyond v1

- v2: more adapters (Cursor, Grok Build, Aider, Antigravity), ACP transport for workers that support it cleanly, git-worktree isolation for parallel writing branches, learned router trained offline on logged trajectories, richer DAG (parallel branches).
- v3: pluggable conductor prompts and role packs, community adapter ecosystem, optional opt-in shared trajectory corpus for a better default router.

## 21. Risks and constraints

- No shared context window across workers. The blackboard plus per-family context assembly is the mitigation. Context packet assembly is the hard engineering problem, not the adapters.
- Harness latency. Driving CLIs headlessly is slower than a raw API call (process spawn, model think time). Right for long tasks, wrong for low-latency interactive use. Set expectations.
- Beta instability. Grok Build and some Cursor CLI paths change and break. Adapters need defensive timeouts, retries and version pinning.
- Auth footguns, Claude especially. A stray `ANTHROPIC_API_KEY` silently moves a Max user onto metered API billing. `doctor` and the adapter must detect and warn.
- Per-vendor ToS. Local, user-driven, user-installed, bring-your-own-login use is the defensible posture. No hosting, no pooling, no embedding tokens. Vendor policies on subscription use in third-party tools shift, so isolate auth handling per adapter to adapt fast.
- Conductor free-tier ceilings. 10K neurons/day or trial credits can run dry mid-run. Need graceful fallback to the next configured conductor backend.