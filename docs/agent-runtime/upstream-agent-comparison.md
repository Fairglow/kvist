# What Kvist can learn from other coding-agent runtimes

Research date: 2026-10-02. Status: advisory research, not an architecture
decision, security audit, compliance report, or approved implementation plan.

The inspected baseline below is historical. Subsequent implementation and
qualification are documented in the
[security-first runner guide](runner-hardening.md), without changing the
upstream pins or treating these research recommendations as acceptance.

## Executive recommendation

**Keep Kvist's authority and execution boundaries; borrow better agent
mechanisms behind them.** Replacing `agent_runner` with an entire upstream
agent would exchange a relatively small, inspectable mechanism for an
application with its own context, configuration, tool execution, persistence,
and approval semantics. That is not the same as reusing a model adapter,
transcript validator, edit parser, or protocol implementation.

The most valuable near-term improvements are not more autonomy:

1. Make context budgeting accurate enough to avoid oversized requests, and
   preserve complete tool-call/result groups during compaction.
2. Add surgical, conflict-aware editing and bounded file/search tools.
3. Make recording, terminal outcomes, cancellation, and interrupted-effect
   recovery explicit before adding live session resume.
4. Offer a headless, structured interface over the same loop as the TUI.
5. Add provider and tool interoperability only through existing authority
   seams, not by importing upstream execution defaults.

Goose is particularly relevant as a Rust agent-engine reference. OpenCode is
particularly relevant for coding tools, context management, and client/engine
separation. **Codex should be a third primary reference**, rather than a
footnote: its Rust implementation includes real operating-system sandboxing,
structured non-interactive events, patch tooling, and durable history.
OpenHands and Aider offer more focused lessons. Rig's newly released sans-I/O
run machinery deserves a bounded reevaluation, not automatic adoption.

The premise that we cannot use any of these directly is too absolute. Some
small packages can be reused largely unchanged, and complete agents can be
optional opaque backends under an independently enforced boundary. Neither
option makes their internal state or permission decisions authoritative for
Kvist. Network-denied execution also makes a stock external agent substantially
harder to integrate than simply launching it inside Bubblewrap.

## Scope, evidence, and limitations

This comparison examines source and public documentation, not a benchmark of
coding quality or a penetration test. "Observed" means visible in the inspected
implementation; it does not mean that a behavior was exercised on this machine
or that all configurations are safe. Recommendations are deliberately separate
from current capabilities.

The checked-out Kvist baseline is
`18b9e59f69ccc96319e59f4d2e9903abd541c4ec`. Its worktree was clean when
investigation began. Upstream source links below use immutable revisions:

| Project | Inspected revision | Relevant implementation and license |
| --- | --- | --- |
| Goose, `aaif-goose/goose` | `920313e4ee26418258b07de124f23d10d9368b2a` | Rust engine/CLI; Apache-2.0 |
| OpenCode, `anomalyco/opencode` | `a79ecfe109294909a239c98fb89f02979d5aa10b` | TypeScript/Bun agent and terminal client; root license MIT |
| Codex, `openai/codex` | `6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0` | Rust CLI/core; Apache-2.0 |
| Aider, `Aider-AI/aider` | `5dc9490bb35f9729ef2c95d00a19ccd30c26339c` | Python; Apache-2.0 |
| OpenHands Agent Canvas | `a8c05584ec6bb063a0857460b9cbff48e136919f` | Application/client; MIT |
| OpenHands Software Agent SDK | `0a9abc87641ad7ffe02e2dadf5e2cb3976b35217` | Python engine/server, TypeScript client; MIT |
| Rig | `d6e4beb3673ae27751120a6a9f4753d292dc7277` | Rust; MIT |
| Rig released `v0.43.0` | `654567eb64274fca00cab86cdd32c86b9913769e` | Released run machinery also checked independently |
| ACP Rust SDK | `7ae02c50a37d79077ff331667315171731f114c1` | `agent-client-protocol` 2.2.0; Apache-2.0 |
| MCP Rust SDK | `ae2f9c9b45a2c98d24ee345406e79f507c9f9282` | `rmcp` 3.5.0; licensing transition discussed below |

These are source snapshots, not recommended dependency versions. Main-branch
manifests are not evidence of a published package; version-specific docs.rs
pages were separately checked for Rig, ACP, and MCP. The direct crates.io API
returned HTTP 403 during this investigation. Live documentation can change,
so source citations take precedence for snapshot-specific claims.

This is project-level comparative research, not an authoring-agent context
bundle. Reading all three Kvist peer implementations for this report does not
authorize propagating peer internals into a future component task.

### Reading map

- [Kvist's current baseline](#1-what-kvist-actually-has-today).
- [Detailed Goose and OpenCode comparison](#2-goose-and-opencode-in-depth).
- [Codex, Aider, OpenHands, and Rig](#3-additional-references-worth-studying).
- [Whole-package reuse and licensing](#4-what-can-be-adopted-wholesale).
- [External integration boundaries](#5-how-external-agent-integration-could-actually-work).
- [Delivery priorities](#6-recommended-delivery-sequence).
- [Conformance and evaluation](#7-conformance-and-evaluation-before-adoption).

## 1. What Kvist actually has today

### Three different boundaries, not one agent runtime

The current code has three relevant components:

| Component | Current responsibility | Important distinction |
| --- | --- | --- |
| [`agent_runtime`](../../agent_runtime/CONTRACT.md) | Canonical model messages and tool intents, direct local transport, subprocess supervision, profiles, reusable trajectory and loop-detection utilities | A transport proposes tools; it does not authorize them |
| [`agent_runner`](../../agent_runner/CONTRACT.md) | Interactive TUI, conversation loop, four model-facing tools, context compaction, history, recorder and executor seams | A standalone interactive coding shell, not the whole Kvist task lifecycle |
| [`sandbox_runner`](../../sandbox_runner/CONTRACT.md) | Independent closed-request validation and Linux Bubblewrap enforcement | Enforces approved requests; does not approve tasks or promote results |

The engine's task authoring path is also materially different from the
interactive runner. Its
[`authoring` broker](../../engine/src/authoring/mod.rs) has a closed vocabulary,
protects the five component artifacts, confines writes to `src` and `tests`,
and already provides exact-single-occurrence `edit_file` semantics.
The [sandboxed applier](../../engine/src/authoring/apply.rs) revalidates staged
intents and content identities. The interactive runner instead exposes a
general shell and grants the working directory read-write.

**Do not assume that launching the interactive runner in a component directory
automatically enforces the engine's protected-artifact policy.** A writable
component directory is a wider grant than writable implementation roots.
This distinction is central to any upstream-agent integration too.

### Strengths worth preserving

The [loop](../../agent_runner/src/session.rs) accepts independent
`ModelTransport`, `ToolExecutor`, `EventSink`, and `Recorder` collaborators.
Effects are sequential. The model does not directly own a filesystem handle or
subprocess launcher through its message types. This is a useful foundation
for adapters and deterministic tests, not something to discard lightly.

The default
[executor](../../agent_runner/src/executor.rs) builds authoring requests with
network denied and delegates process execution to the installed runner.
Toolchain advertisement checks what reaches the sandbox rather than blindly
trusting the host `PATH`. Transient model failures have bounded retries and
growing attempt deadlines; cancellation and output limits already exist.
The TUI has model/effort selection, live progress, history, replay, Markdown
rendering, and responsive wrapping. The
[loop integration tests](../../agent_runner/tests/loop_integration.rs) use a
scripted transport and recording executor.

These are substantive strengths. Upstream agents mainly offer richer
mechanisms and a larger corpus of handled edge cases, not a replacement for
Kvist's human authority, recursive artifacts, or independent compliance model.

### Current gaps that change the comparison

**Host execution is implemented.** The
[CLI](../../agent_runner/src/cli.rs) accepts `--allow-host-execution` and
`--host-turns`; the [TUI](../../agent_runner/src/tui/mod.rs) selects
[`HostExecutor`](../../agent_runner/src/host.rs) explicitly. This is not an
automatic sandbox failure fallback, but it is unsandboxed host authority.
A one-turn cap is not a one-tool cap and does not confine a shell script's
effects. Protected Kvist task execution should never select this mode.

**Context bounding is heuristic and best effort, not a hard request guarantee.**
In [context management](../../agent_runner/src/context.rs), text is estimated
at four characters per token and each tool definition contributes a fixed
eight tokens, irrespective of schema and description size. Session accounting
uses the conversation without the separately injected system prompt.
Segmentation is by user message, so one autonomous prompt can accumulate a
large tool loop that remains one indivisible recent segment.

Keeping the newest segment cannot bound an individually oversized segment.
There is a concrete scale mismatch: the loop allows a 64 KiB stdout preview
and a separate stderr preview, while its default window is 8,192 tokens.
For ASCII text, even one full stdout preview is approximately 16,384 tokens
under its own estimator, before instructions, schemas, or output headroom.
This is a source-derived boundary example, not a model benchmark.

**A session log is not yet a recoverable, trusted execution journal.**
The [recorder](../../agent_runner/src/session_log.rs) writes structured
reasoning and tool data, but does not capture the complete user/assistant
conversation needed to reconstruct a live session. `Recorder` methods return
no result; individual recording writes discard errors. `ToolDispatch` is
written through `tool_result` after execution, rather than as an acknowledged
pre-effect intent. `action_hash` is a tool/call-ID label, and `state_mutated`
is derived from output presence, not observed filesystem changes.

Full tool argument values and bounded output are recorded; bounding is not
redaction. The human transcript also shortens reasoning. Consequently,
"full transcript," "redacted," and "canonical compliance evidence" should not
be inferred from module comments alone. The TUI can continue without logging
after an explicit diagnostic, and `--no-logs` exists.

**Read-only replay is different from resume.**
[History](../../agent_runner/src/history.rs) lists and displays saved text;
it does not restore an executable session. A model or effort switch starts a
fresh worker and resets conversation context
([session builder](../../agent_runner/src/tui/mod.rs)).
That conservative behavior is preferable to silently reusing incompatible
provider history, but the user-facing transition could become more explicit.

**Bounds are mostly per turn or per tool, not a complete run budget.**
The loop caps model turns at 50. Growing per-attempt deadlines and retry
backoff do not constitute one total prompt deadline or token/cost budget.
The engine already has a shared `TurnBudget` in
[`engine/src/agent.rs`](../../engine/src/agent.rs), and `agent_runtime`
already has action/observation
[loop-detection helpers](../../agent_runtime/src/loop_detection.rs).
The interactive loop does not currently wire those helpers in. Reuse local
prior art before adding another dependency.

The component [queue](../../agent_runner/TODOS.yaml) already records
documentation drift and pending independent compliance work. The older
[architecture guide](architecture.md) and
[runtime-selection guide](runtime-selection.md) describe some mechanisms as
planned and retain superseded Rig transport material.
[ADR 0009](../decisions/0009-brokered-model-transport-and-isolated-effects.md)
is explicit that the direct transport is the sole selected transport.
This report does not reconcile those artifacts or certify the runner.

## 2. Goose and OpenCode in depth

### Correcting the language premise

Goose has a Rust workspace containing its engine, CLI, providers, generic
agent machinery, and related types.[G1] Its CLI is a readline-style REPL using
`rustyline` and terminal-formatting libraries, not a ratatui full-screen UI.
The desktop shell is a separate application surface. Rust implementation makes
some code more accessible to Kvist, but does not by itself make its authority
or dependencies suitable.

The inspected **`anomalyco/opencode` agent is TypeScript running on Bun**.
Its terminal client uses OpenTUI/Solid rather than a Rust agent engine or
ratatui.[O1] This assessment applies to the linked repository and pinned tree;
it should not be conflated with other projects or older implementations using
the OpenCode name. Native dependencies of a JavaScript application also do not
make its agent loop a reusable Rust library.

Consequently, Goose offers more potential source-level Rust reuse. OpenCode
offers algorithms, tool contracts, and UX patterns that mostly need adaptation
or reimplementation. Neither is a reason to replace Kvist's existing ratatui
presentation stack.

### At-a-glance comparison

This is a comparison of architectural fit, not a coding-performance ranking.

| Concern | Kvist runner today | Goose reference | OpenCode reference | Best lesson for Kvist |
| --- | --- | --- | --- | --- |
| Loop | Small sequential loop with injected transport/executor | Explicit operations, conversation effects, reloadable sessions | TypeScript session processor composed with the Effect library | Explicit transitions and reconstruction, without surrendering effects |
| Edits | Whole-file write plus shell; engine separately has exact edit | Exact single-match replacement with helpful failure previews | Rich replacement strategies; newer exact-edit path with stale-content checks | Start strict, add actionable feedback and conflict checks |
| Context | Deterministic abbreviated history; rough token estimates | Token-accounting helpers and model-driven compaction | Deterministic pruning plus model-driven summarization | Separate request budgeting, output reduction, and semantic summarization |
| Models | Two qualified local transports | Broader provider implementation and Ollama quirks | Broad provider normalization and retry handling | Import tested compatibility knowledge, not every provider dependency |
| History | Optional journals and read-only replay | Store-backed conversation transitions and reconstruction tests | Session history, snapshots, revert, nested sessions | Durable facts first; restart/fork are distinct from effect replay |
| Permission UX | Upfront config, no action prompts; optional host mode | Stored decisions and model-assisted read-only judgment | Declarative allow/deny/ask rules | Deterministic capabilities; deny or stop instead of popping up |
| Tools/services | Four tools, sandboxed processes by default | Developer extension, MCP, recipes/skills, ACP | Editing/search ecosystem, LSP, MCP, subagents | Expand only behind explicit grants and resource bounds |
| Frontends | Interactive TUI; listing/import are non-interactive | REPL, headless JSON/NDJSON run output, ACP | Terminal client and separate server/API packages | One engine/event contract with optional frontends, not a mandatory daemon |
| Authority/evidence | Engine owns task policy and independent review | Upstream session and tool semantics | Upstream session and tool semantics | Neither upstream can replace Kvist's acceptance/compliance authority |

### Goose: explicit operations and conversation effects

Goose's `goose-agent` machinery separates step selection from applying
conversation effects. `StateMachine::step` walks operations/inference steps;
the outer driver loads the stored session, performs a step, and applies its
result before advancing.[G2] Effects include appending messages, replacing the
model-visible conversation, and updating tool-request metadata.

The concrete handler persists conversation changes before emitting frontend
events.[G3] It can preserve historical messages while changing their visibility,
rather than deleting history when context is compacted. That is a useful
precedent for treating the model's current context as a view over a fuller
record.

**Adaptation:** keep Kvist's loop small, but make states such as awaiting model,
validating proposals, dispatching a tool, awaiting observation, and terminal
explicit. Prefer fallible persistence before publishing a durable fact to a
frontend. Do not import Goose's SQLite session store as authoritative Kvist
workflow state.

Crucially, **conversation persistence is not an operating-system effect
transaction**. Persisting a message before a UI notification does not establish
that every shell/file action has a durable pre-dispatch record or that an
interrupted action is safe to replay. Kvist still needs its own intent,
dispatch, result, and unknown-effect recovery protocol.
In the inspected tool operation, dispatch is awaited inside the operation
before the returned conversation effects are applied.[G14] That specific
execution/result-recording gap must not be described as exactly-once recovery.

The generic crate's separation is worth studying, but its presence in a Rust
workspace is not proof that it is a stable, independently consumable package.
Evaluate the actual package graph and adapter cost. A pattern port may be
simpler than translating all of its message/session types into Kvist types.

### Goose: provider behavior and complete-accounting primitives

Goose's token counter has APIs accepting the system prompt, messages, and
tools, plus a wrapper that includes attached resources.[G5] That shape is
better than counting tool definitions as a constant number of tokens. However,
the inspected context-management code also contains message-only estimates
with empty system/tools arguments.[G6] Do not infer a universal complete-request
guarantee from the availability of a good helper.

Its tool-accounting implementation is still an estimate, not a tokenizer
oracle for every provider/model/chat template. Kvist should count the actual
request representation or use a conservative model-qualified estimator,
including full schema material and output reserve. An OpenAI-oriented token
heuristic should not be assumed exact for an Ollama-served model.

Goose also demonstrates concrete local-model compatibility work:

- Its Ollama provider distinguishes generation/request time from streaming
  stall detection and emits actionable stalled-stream diagnostics.[G7]
  Kvist already has a cadence watchdog: compare edge cases rather than adding
  another overlapping timer.
- It documents and handles XML-like tool-call output from some Ollama models
  that fail to use native structured calls under certain tool configurations.[G8]
  This is a valuable conformance fixture, not a reason to make arbitrary
  answer text executable. Any fallback parser needs an explicit qualified
  model/profile, bounded syntax, normal schema validation, and the same broker.
- Its retry layer distinguishes typed transient errors from permanent ones.[G9]
  This fits Kvist better than broadly treating any failure string as retryable.

Provider breadth can save compatibility research even when none of the provider
module is imported. Preserve Kvist's numeric-loopback, proxy/redirect, payload,
and unsupported-capability rules. Remote credentials and TLS/provider expansion
need a separate approved design.

### Goose: surgical editing, subprocess lifecycle, and tests

Goose's developer editor accepts an exact single match. No match produces
context/helpful suggestions; multiple matches produce an ambiguity error with
locations rather than choosing an arbitrary occurrence.[G4] This is a good
fit for Kvist's initial editor: suggest likely context without silently using
fuzzy matching to mutate a different region.

Its subprocess support illustrates Linux parent-death signals, process-group
handling, and a parent-PID race check.[G10] These are useful cleanup cases for
Kvist's supervisor and future MCP servers. They are not confinement on their
own. If adopted, place platform-specific mechanics behind existing process
interfaces and test normal exit, cancellation, parent death, descendants, and
blocked output separately.

The most valuable test technique is **reconstructing the agent after every
applied step**. Goose's lifecycle tests check that tool calls/results remain
ordered through reconstruction and that different sessions keep their working
directory, tool state, and usage isolated.[G11]
Kvist already has scripted transport tests; add reconstruction and persistence
failure injection rather than replacing them with a new harness.

A concrete frontend pattern is `goose run --output-format stream-json`,
which emits typed NDJSON message, notification, error, and completion events.
`--output-format json` instead produces one final object with messages and
metadata.[G15] These are real non-interactive surfaces, distinct from the ACP
server and chat gateway. Kvist can borrow the text/final-JSON/event-stream
distinction while defining its own versioned, bounded schema and more explicit
tool/attempt/terminal identities.

A fresh persisted session is still not a clean-slate compliance reviewer:
review scope and allowed evidence must be composed separately. Similarly,
reconstruction tests do not prove crash safety at every point inside a tool
effect.

### OpenCode: richer editing and language feedback

OpenCode's edit implementation has multiple replacement strategies: exact
text, line/whitespace normalization, indentation handling, anchored/contextual
matching, and related fallbacks. It includes ambiguity handling and a guard
against disproportionately large matches, plus per-path serialization.[O2]
A distinct patch tool offers another edit representation.[O3]

The lesson is **not** "copy all fuzzy matchers." Exact byte matching and a
content precondition are easier to review. A more forgiving matcher can change
indentation-sensitive code or select the wrong repeated block. Kvist should
first expose the engine's existing exact replacement semantics through an
appropriate standalone mechanism, not make `agent_runner` depend on engine
internals. Later matcher improvements require explicit semantics and focused
tests.

There is also a **newer, exact-edit implementation in `packages/core`**.
It captures source bytes and writes through `writeIfUnchanged`; the file-
mutation helper compares expected bytes under a per-target keyed lock and
returns a typed stale-content error when they differ.[O10], [O11]
This is a useful reference for conflict-aware mutation, not just a proposed
Kvist-original feature.

The two edit paths coexist in this snapshot. This investigation did not
establish which path every live session dispatches, so the newer safeguard is
an observed implementation capability, not a blanket claim about the default
CLI. A process-local lock also does not coordinate arbitrary external writers,
and comparing bytes inside an edit operation is not proof that the model's
earlier context was fresh. Kvist should define precisely which preimage is
bound to its authorization and what races the effect backend actually prevents.

OpenCode integrates language-server diagnostics with its coding tools.[O4]
Immediate diagnostics can shorten the model's repair cycle substantially
without another broad repository exploration. They remain advisory feedback;
they do not replace approved offline test verification.

Language servers are subprocesses, but that does **not** require unsandboxed
host mode. Kvist can launch an explicitly approved, preinstalled server in a
restricted execution environment with bounded lifecycle and allowed source
roots. The hard questions are build/toolchain availability, read scope,
network denial, state directories, output limits, and process cleanup. Cross-
workspace LSP discovery must not expose peer internals implicitly.

Similarly, bounded `read_file` ranges and first-class glob/search tools reduce
the need for the model to generate ad hoc shell scripts. Prefer stable ordering,
line/byte limits, pagination, and explicit truncation over dumping whole files
and hoping compaction catches up.

### OpenCode: layered context reduction and retry experience

OpenCode separates deterministic older-tool-output pruning from a
model-generated compaction summary. Its pruning logic protects recent tool
material and exempts selected tool categories such as skills.[O5]
This layered approach is more useful than choosing between "never summarize"
and "ask a model to summarize everything."
The pruning path marks older tool parts compacted; summary serialization then
represents them as cleared content. Separately, a 2,000-character limit applies
when rendering tool output into the summarizer's prompt. That constant is
**not** an unconditional capture limit or ordinary-request output bound.

Kvist should distinguish three independent operations:

1. Bound or paginate each tool result at capture.
2. Reduce older, already-observed outputs while preserving valid call/result
   structure and authoritative instructions.
3. Optionally summarize completed work, retaining provenance and a clearly
   non-authoritative summary.

A skill exemption is not a protected-file authorization boundary. Context
retention policy also cannot allow an unbounded exemption to overflow a request:
if required material cannot fit, fail explicitly or request a narrower task.
Copying upstream token thresholds designed for larger hosted models would be
especially inappropriate for Kvist's default 8,192-token window.

OpenCode's retry implementation handles broad provider errors and
`Retry-After`-style hints.[O6] That is useful future remote-provider experience,
but Kvist's two local transports can retain a narrower typed error classifier.
Retries should have one shared prompt budget and distinct attempt events so
partial visible text is not mistaken for the accepted final answer. A model
retry and a retry of an effectful tool are different operations.

### OpenCode: permissions, snapshots, and optional frontend architecture

Its permission system supports declarative `allow`, `deny`, and `ask`
decisions.[O7] A Kvist adapter can use deterministic allow/deny decisions
without adopting per-action dialogs. Each tool request should still be
checked against the approved capability grant; "no prompts" does not mean
"no per-call authorization."
Its inspected evaluator uses the **last matching rule in declaration order**,
not most-specific/longest-pattern matching, and falls back to `ask` when no
rule matches. Reusing that pattern requires an explicit policy for unmatched
requests: Kvist should deny or stop clearly rather than unexpectedly prompt.
Test rule order and overrides; do not assume a catch-all added last leaves
earlier narrow rules effective.

Goose's model-assisted read-only classification and security heuristics
illustrate a different tradeoff.[G12], [G13] Kvist should not use a model's
"read-only" or "safe" label as authority. Such analysis could at most be
nonbinding advisory input or defense in depth; deterministic authorization
and independently enforced execution remain necessary.

OpenCode's snapshot/revert machinery uses a separate Git store and invokes
Git to capture and restore changes.[O8] Undo is valuable UX, but an upstream
revert is not Kvist's conflict-checked promotion or acceptance. It can interfere
with edits made by a person or another process if applied without exact
workspace/preimage checks.

The fact that snapshotting launches Git does not make the idea incompatible
with Kvist: approved subprocesses can run in the sandbox. The adaptation work
is to keep snapshot metadata/evidence protected, bind scope and preimages,
declare partial effects, and never revert unrelated work or silently alter the
user's index. Start with inspectable proposed diffs before automatic undo.

OpenCode's terminal and server package split demonstrates how richer
frontends can consume a shared engine/API.[O1] Kvist can borrow that
separation without adopting a mandatory HTTP server or Bun application stack.
The first step is a headless interface over the existing loop and canonical
events. A web/editor frontend, if later justified, can remain optional.

### Recipes, skills, and subagents: useful only with smaller authority

Goose's recipe/skill operations and OpenCode's nested task sessions provide
reusable ideas for role-specific instructions, structured results, and bounded
delegation.[G11], [O9] They should not become a parallel task authority alongside
`TODOS.yaml`.

A Kvist recipe could be a reviewed adapter configuration for an existing task
purpose. A skill is additional untrusted context unless explicitly approved.
Do not automatically discover broad repository instructions or import public
skills into a protected task. Requested dependencies, architecture changes,
and plan updates remain proposals for the human/engine, not executable
instructions merely because an upstream recipe expresses them.

For future subagents, construct a fresh local context with a grant no broader
than the parent's approved scope, new budgets, and explicit lineage. An
in-process child session is not an independent OS boundary, and a separate
session ID is not evidence of source-blind review. Avoid concurrent effectful
children until isolated workspaces and conflict-aware promotion exist.

### Comparative judgment

**Goose is the stronger Rust loop, provider, and reconstruction reference.
OpenCode is the stronger rich-tool and frontend/workflow reference in this
investigation.** These judgments describe inspected mechanisms, not model
quality, overall reliability, or security rankings.

The designs converge on explicit conversation state, targeted tools,
history reduction, reusable provider boundaries, and richer sessions.
Kvist's distinctive job is to make those mechanisms subordinate to precise
context, authority, artifact, and independent-review rules. Borrowing the
mechanisms does not require copying either application's defaults.

## 3. Additional references worth studying

### Codex: the strongest additional Rust reference

Codex is more directly comparable to Kvist than a general agent framework.
Its Rust tree contains separable patch, execution-policy, Linux sandbox,
protocol, history, rollout, and CLI packages.[C1]

Unlike an application-level tool permission system, its Linux helper implements
actual operating-system restrictions: the inspected source describes
`no_new_privs`, seccomp, and Bubblewrap, with Landlock-related support and
platform-specific handling.[C2] Kvist should study those failure and lifecycle
cases, but not replace its independently installed runner with Codex's
application-coupled permission model. This also refutes the blanket assumption
that all mature coding agents lack meaningful sandboxing.

The most attractive ideas are:

- **Typed headless events.** `codex exec --json` has thread, turn, and item
  identities and separate started, updated, completed, failed, and error
  events. Command execution and file changes are distinct item types.
  A comparable Kvist API would be more useful than parsing TUI output.[C3]
- **Persistent history lifecycle.** The rollout recorder supports create,
  resume, fork provenance, queued writes, and acknowledged flush/shutdown
  operations. Borrow explicit ownership and acknowledgment, not its database
  or entire persisted format. A writer flush is not automatically an
  exactly-once effect guarantee or power-loss-safe transaction.[C4]
- **Context replacement as a lifecycle.** Compaction keeps checkpoint metadata
  separate from replacement history and deliberately reinjects initial
  context. Another path starts a fresh token-budget window without a
  summarizing model call. This supports a useful Kvist design: original
  artifacts remain authoritative, and summaries are explicitly lossy views.[C5]
- **Portable patch fixtures.** Input tree, patch, and expected tree scenarios
  cover create, delete, move, missing context, line endings, overwrite, and
  partial failure. Adopt the test organization and selected licensed fixtures
  alongside Kvist-specific expectations.[C6]

`codex-apply-patch` is not a tiny, drop-in safe editor at this revision.
Its manifest depends on other Codex workspace crates, Tokio, and Bash
tree-sitter parsing. The library's default options follow symlinks and
normalize updated line endings to LF. Matching deliberately becomes more
permissive through whitespace and Unicode normalization; fixtures explicitly
cover overwrite and failure after partial success.[C6]

**Recommendation:** evaluate the parser/format and fixtures separately from the
effectful executor. Kvist should enforce destination grants, expected
preimage digests, uniqueness, line-ending behavior, no-follow semantics, and
declared partial-effect behavior itself. Importing the complete package would
need a dependency and contract justification, not merely an Apache license.

Likewise, `codex-execpolicy` is a reusable-looking package, but it brings a
Starlark rule system and prefix matching.[C7] Kvist does not need that much
policy language to authorize four tools. Learn from typed decisions and
explainable matching before considering the package.

### Aider: token-efficient edits and bounded code maps

Aider's SEARCH/REPLACE editing avoids regenerating entire files. It feeds
specific failed-match feedback back to the model and distinguishes edits
already applied from those requiring repair.[A1] This is immediately relevant
to local models with modest context windows.

However, the inspected implementation can try other files already in chat
after a match fails in the requested file, and includes whitespace-tolerant
matching. Those are convenience choices, not suitable default semantics for
Kvist's strict resource binding. A failed destination match should remain a
failure; it should not become authority to modify a different file.

Its repository map extracts symbols with tree-sitter and ranks definitions
and references in a graph under a token budget.[A2] That is a good idea with
a different scope: **build a component map, not a whole-repository map.**
Include local implementation and permitted contracts only. Do not expand to
peer implementations because a ranking algorithm considers them relevant.
Generated maps should be bounded, revision-bound, non-authoritative context.

Reuse the edit protocol ideas and evaluation cases; do not embed the Python
application or import its entire repository-discovery behavior into the Rust
core. Automatic commits and broad file discovery would also need separate
Kvist authorization.

### OpenHands: execution backends, typed observations, and valid context views

The current `OpenHands/OpenHands` repository is Agent Canvas, a client/control
application. The runtime lessons belong primarily to the separately inspected
Software Agent SDK. Its Python engine and Agent Server expose local and remote
conversations and ephemeral workspaces; it also supplies a TypeScript client.
Do not confuse the old monolithic runtime layout with today's boundaries.[H1]

Especially useful ideas are:

- **Typed actions versus observations.** A proposed action and an observed
  result are different records, which maps naturally to Kvist's broker and
  effect boundary.
- **Context-view invariants.** The SDK protects action/result pairing and
  complete tool loops when manipulating history. Provider reasoning blocks
  may impose additional atomicity constraints. Port the invariants and tests,
  not provider-specific Python classes.[H2]
- **Stuck-loop detection.** Repeated action/observation cycles, repeated
  errors, alternating patterns, and unproductive monologues are classified
  separately. Compare this with Kvist's existing detectors before extending
  them. A detector should stop or propose recovery, never silently increase
  authority.[H3]
- **File-backed event identity and branching.** The event store has
  action-history IDs, parent links, and writer locking, with explicit NFS
  caveats. These are useful design inputs for filesystem-native evidence
  without requiring an opaque database.[H4]

An OpenHands local workspace is not isolation. Its current quickstart warns
that unsandboxed Agent Server execution has full filesystem access. Docker
examples also grant access to selected project directories; their mount and
network configuration must be assessed, not accepted because the word
"sandbox" appears.[H1]

Do not import its always-on orchestration, cloud integration, public skill
loading, or automation services as core Kvist requirements. An optional opaque
workspace backend is possible later, but a REST server, Python environment,
and container lifecycle are a large price for mechanisms Kvist can implement
locally with narrower authority.

### Rig: a changed opportunity, not a reversed decision

The earlier Kvist guidance treated `rig-run` as unavailable in a pinned
release. The useful current surface is **`rig_agent::run` in released
`rig-agent` 0.43.0**, not a presumed standalone `rig-run` package.[R1]

It is a serializable sans-I/O state machine. Its driver receives
`CallModel`, `CallTools`, and `Done`; the run owns no model, executable tools,
memory backend, or hooks. This is a substantially better fit for Kvist than
embedding a complete autonomous agent that dispatches tools internally.
The released source and version-specific docs.rs page both expose this seam.

A private adapter could drive model steps through `DirectModelTransport`,
execute tools sequentially through Kvist's executor, and reconstruct state
from Kvist-owned history. The library also has transcript validation and
explicit feedback for invalid calls and skipped siblings.[R2]

There are important limits:

- The package still depends on `rig-core` types and other generic machinery.
  A sans-I/O module is not automatically a separately lightweight crate.
- Serialized pending calls can be obtained again after resume. Kvist must
  reconcile them with its effect journal before dispatch; serialization alone
  does not prevent duplicate writes.
- Tool-name repair, recovery, raw provider payloads, and retries require
  deliberate adaptation to Kvist's closed schemas and redaction policy.
- The transcript validator treats duplicate call IDs as one pending ID.
  Kvist should reject duplicate IDs explicitly rather than assuming the
  library enforces every desired invariant.

**Recommendation:** a small, effect-free conformance experiment is now
reasonable. Keep Rig types private, disable unnecessary features, retain
direct transport, and compare maintenance savings against conversion code and
dependency growth. ADR 0009's rejection of Rig transport remains in force;
reevaluating pure run mechanics is a separate decision.

## 4. What can be adopted wholesale?

"Wholesale" should refer to a bounded implementation unit, not upstream
application authority.

| Candidate | Reuse category | Conditions and judgment |
| --- | --- | --- |
| `agent-client-protocol` Rust SDK | Use package largely unchanged behind an adapter | Strong candidate for structured external-agent/editor communication; choose tested protocol/version and deny unapproved client bridges |
| `rmcp` Rust SDK | Use package largely unchanged behind a broker | Strong candidate when MCP is needed; select minimal client/transport features and retain Kvist-owned launch, bounds, credentials, and tool policy |
| Existing Kvist `agent_runtime` model, supervision, and loop-detection mechanisms | Reuse existing local code | First choice where sufficient; several improvements need wiring or contract refinement, not a replacement framework |
| Rig's released sans-I/O run machinery | Conditional dependency experiment | Potentially reusable without upstream tool execution; conversion, persistence, and recovery semantics need conformance evidence |
| Codex patch parser and fixtures | Extract/adapt or evaluate private dependency | Promising Rust reuse; effectful package defaults and workspace dependencies prevent an unconditional recommendation |
| Goose engine/provider/extension components | Selective adaptation or opaque backend | Rust is helpful, but engine types and effectful extension lifecycle are not a safe drop-in authority layer |
| OpenCode tool/context algorithms | Port patterns and selected licensed tests | High-value behavior; TypeScript application modules are not Rust library dependencies |
| A complete Goose/OpenCode/Codex/OpenHands process | Optional opaque execution backend | Only inside tested outer confinement with solved model transport and protected artifacts; not the default high-assurance native loop |

### Protocol SDKs are more reusable than whole agents

The ACP Rust package's manifest declares edition 2024, Rust 1.88, and
Apache-2.0. Its published 2.2.0 documentation distinguishes stable protocol v1
from draft v2 features. Fork, compaction, plan operations, and model-provider
surfaces have explicit unstable feature gates.[P1] Kvist already performs
limited ACP model discovery in `agent_runtime`; extending that into full
execution is a new capability, not something discovery already provides.

Use ACP for lifecycle and presentation interoperability. A client-provided
filesystem or terminal callback must not execute on the host just because an
agent requested it. Route it through approved Kvist authority, or reject it.
An agent may also have internal tools that never call the client; ACP therefore
does not imply complete tool mediation.

The MCP Rust package exposes client/server and multiple independent transport
features. Defaults include server/macros functionality; a Kvist MCP client
should not enable everything by habit.[P2] In particular, a child-process
transport can spawn a process, and a network transport can access an endpoint.
Kvist should control those effects and then supply approved transport I/O to
the SDK where practical.

MCP tool annotations and reported roots are not proof of safety or confinement.
Imported tools require an approved descriptor, schemas, effect classification,
resource binding, timeout/output limits, and retry semantics. MCP sampling,
elicitation, resources, prompts, and server-initiated requests need individual
policy decisions; supporting `tools/call` is not approval of the whole protocol.

### Licensing and dependency cost

Kvist's [dependency policy](../../deny.toml) already allows MIT and Apache-2.0
alongside AGPL-3.0-or-later, and rejects unapproved Git sources. These upstream
licenses generally permit reuse with their applicable notices and obligations;
they do not justify silently changing Kvist's license or removing attribution.
Optional commercial distribution must preserve upstream rights and notices.
File-level exceptions, bundled assets, third-party components, patents, and
trademarks need separate assessment when actual adoption is proposed.

The current MCP SDK is a useful warning against trusting metadata alone:
its manifest declares Apache-2.0, but its actual `LICENSE` describes a
MIT-to-Apache transition with some original MIT contributions retained and
documentation under CC-BY-4.0.[P2] Record the exact package's applicable
license material rather than reducing that to a simplistic root badge.

Prefer an exact published version and minimal features. Keep adapted code's
provenance and licensed fixtures; vendor through Kvist's approved host
acquisition process before offline validation. Avoid dependencies on moving
Git branches, upstream application databases, browser stacks, or optional
network libraries for a mechanism that only needs deterministic local logic.
This is technical due diligence, not legal advice.

## 5. How external-agent integration could actually work

There are three different integration depths:

| Integration | What Kvist can know/control | Suitable use |
| --- | --- | --- |
| Pattern or utility reuse inside the native loop | Context selection, tool intent, authorization, effects, events, recovery | Default path for component tasks and evidence-sensitive work |
| External agent with structured ACP/JSON events inside outer confinement | Whole-process authority, lifecycle, emitted events, final diff; internal tool coverage remains incomplete | Optional coding/advisory backend with explicit assurance limits |
| Complete upstream engine embedded in the host | Only what the adapter actually intercepts; plugins/internal tools may cause host effects | Not recommended without a demonstrated complete authority seam |

The simplest external integration is not currently usable unchanged:
`--unshare-net` gives the process a separate loopback namespace, so a stock
agent cannot reach the host's Ollama or llama-server at `127.0.0.1`.
Giving the entire process host networking or mounting provider credentials
would undo the transport/effect separation already chosen in ADR 0009.

A future implementation could use one of these deliberately designed paths:

- Run the opaque agent in confinement and expose only a host-owned,
  application-aware model channel compatible with its provider adapter.
  Protect real credentials outside the agent/tool environment, bind model and
  budgets on the host, and enforce a narrow model operation schema.
- Keep an effect-free upstream planning/loop mechanism on the host and send
  every permitted action through Kvist's broker/executor. This works only
  when there is a demonstrably complete interception seam; "configure it to
  use our tools" is not proof that no internal tool remains.

These are designs, not current supported commands. A read/write MCP server or
ACP callback alone is insufficient while the agent retains an independent
shell or filesystem tool. A model channel is also a deliberate data-egress
capability even if ordinary authoring network access remains denied.
Prompt contents may include component data; users must approve the applicable
provider and confidentiality policy.

Qualification must bind the exact executable/version, extensions, configuration,
environment, context/mount plan, model transport, and policy. Preinstall tools
and dependencies; do not let startup bootstrap packages through unreviewed
network operations. Disable unrelated plugins, discovery, broad repository
instructions, telemetry, and automatic VCS actions where applicable. Unknown
required capabilities or unexpected bridge requests fail explicitly.

For high-assurance tasks, the outer writable workspace must exclude intent,
queue, approval, canonical evidence, and peer implementation. A whole-run
diff can be treated as a proposed result, with independent verification and
promotion; it is not evidence that every internal action was policy-mediated.
Unknown effects after a crash must remain fenced rather than automatically
resumed.

## 6. Recommended delivery sequence

This is an advisory ordering, not newly accepted tasks. Each implementation
requires approved local requirements, contract, design, and an atomic queue
with tests before code, then independent security and compliance review.

| Priority | Change | Main inspiration | Observable acceptance target |
| --- | --- | --- | --- |
| P0 | Clarify interactive versus protected task authority | Kvist's own broker and upstream execution boundaries | Protected task mode cannot select host execution or grant writes to protected artifacts/evidence |
| P0 | Hard request-budget preflight and valid compaction | OpenCode, Goose, OpenHands, Codex | Count complete serialized instructions/history/tools with explicit response reserve; oversized newest segment produces bounded recovery or typed failure, not an oversized send |
| P0 | Reliable event/recording lifecycle | Codex/OpenHands; Kvist engine evidence | Record failure is observable; required evidence acknowledgment precedes effects; cancellation/failure/exhaustion have distinct terminal outcomes |
| P1 | Surgical edit and bounded read/search tools | OpenCode, Aider, Codex; existing Kvist `edit_file` | Reject stale/ambiguous preimages; preserve unrelated bytes; paginate reads/search; changes occur only through approved executor |
| P1 | Headless structured execution over the same loop | Codex/OpenCode/Goose | No terminal required; stable event IDs/schema, separate diagnostics, explicit exit status and final outcome |
| P1 | Shared prompt budgets and stuck-loop integration | Existing `TurnBudget`/detectors; OpenHands | One deadline covers requests/backoff/tools as declared; repeated unchanged actions stop with a durable reason |
| P2 | Qualified provider capabilities and interoperability | Goose/OpenCode; official ACP/MCP SDKs | Advertised, tested, and policy-enabled capabilities remain distinct; unsupported combinations fail before effects |
| P2 | Explicit checkpoints, fork, and resume | Goose/OpenCode/Codex/OpenHands | Resume reconciles policy, workspace, model, and outstanding effects; unknown results cannot be replayed automatically |
| P3 | Sandbox-aware language diagnostics and structured recipes | OpenCode/Goose | Services have explicit resource/context grants; plans remain non-authoritative proposals and cannot certify compliance |

Context improvements should precede model-generated summarization. First
account for the entire request and reserve, shrink tool outputs, preserve
complete call/result groups, and fail clearly when immutable instructions or
the newest segment cannot fit. Optional summaries should preserve active
goals, approved constraints, changed files, failed verification, and pending
decisions, with provenance and an explicit lossy label. They must not become
an alternative requirements document.

Editing improvements should begin with the repository's existing exact edit
semantics, generalized behind the standalone executor without importing engine
types into `agent_runner`. A preimage digest or equivalent conflict condition
must be checked where the write occurs, not only on the host before dispatch.
Multi-file atomicity should be promised only if implemented; otherwise return
precise per-file outcomes and retain partial-effect evidence.

For persistence, distinguish interactive best-effort history from mandatory
task evidence. Keep sensitive diagnostic attachments non-authoritative and
bounded; derive canonical redacted events separately. User/assistant
messages, tool identities, policy/context revisions, and checkpoint lineage
are needed for honest resume. Reasoning text is optional provider output,
not proof of what the agent "really thought" or why an effect was authorized.

Do not prioritize parallel effectful tools, autonomous subagents, broad
repository memory, browser automation, arbitrary plugins, automatic commits,
or a web daemon. Those multiply authority and recovery questions before
solving the current reliability gaps. Future reviewer contexts must be built
fresh from allowed evidence, not inherited from a parent's implementation
conversation, even if an upstream subagent API makes inheritance convenient.

## 7. Conformance and evaluation before adoption

Evaluate a candidate against **the exact Kvist grant and workflow**, not just
whether it can edit a file or stream text:

| Area | Minimum discriminating cases |
| --- | --- |
| Context | Complete schema/system-prompt accounting; large latest prompt/tool output; non-ASCII text; compaction after a multi-call turn; no orphan, duplicated, or fabricated tool results |
| Editing | Zero/one/multiple matches; stale digest; overlapping edits; CRLF and no final newline; symlinks and traversal; move/delete/overwrite; documented partial failure |
| Streaming/retry | Fragmented tool JSON; truncated completion; unknown finish reason; disconnect after visible text; distinct attempt IDs; cancellation during backoff; shared budget exhaustion |
| Recording/recovery | Disk full/permission failure; crash before dispatch, after dispatch, and after effect before result; truncated log tail; changed policy/workspace on resume; no automatic replay of unknown effects |
| Enforcement | Missing/replaced runner; namespace unavailable; protected-file writes; leaked environment/credentials; unexpected network, MCP launch, or ACP callback; descendant cleanup |
| Interoperability | Advertised versus enabled capabilities; version mismatch; unknown fields/required features; tool-list change; rejected tool feedback without grant expansion |
| Presentation/headless | Same canonical outcomes with and without TUI; clear scope indicator; nonzero failed-run status; bounded replay; model switch and history boundaries |
| Workflow | Agent success cannot accept intent, mutate queues, mint receipts, approve promotion, or certify its own compliance |

Use fake transports/executors for deterministic semantics, recorded provider
fixtures for conversion, and native Linux tests for real confinement.
Run controlled live trials only as a separate opt-in qualification step.
Measure task success, unintended changed bytes, verification results, token
consumption, context overflow, latency, cancellation cleanup, and recovery
correctness on the same approved component tasks and model versions.
Do not infer that one agent is "better" from stars, feature count, or
benchmarks obtained with broader context/network privileges.

## Bottom line

Kvist should build less generic agent machinery, but not outsource its
distinctive authority model. The useful strategy is **small trusted core,
rich replaceable mechanisms, explicit evidence, and qualified opaque backends
where their limits are acceptable**. The best first investment is in context,
editing, and durable lifecycle correctness; the best first wholesale reuse is
likely protocol plumbing rather than an entire agent.

## Source register

Local links point into the inspected Kvist checkout. External links below are
immutable source references unless explicitly marked as published or live
documentation.

[C1]: https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/Cargo.toml
[C2]: https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/linux-sandbox/src/lib.rs#L1-L38
[C3]: https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/exec/src/exec_events.rs#L8-L133
[C4]: https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/rollout/src/recorder.rs
[C5]: https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/core/src/compact.rs#L59-L111
[C6]: https://github.com/openai/codex/tree/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/apply-patch
[C7]: https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/execpolicy/Cargo.toml
[A1]: https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/coders/editblock_coder.py#L15-L155
[A2]: https://github.com/Aider-AI/aider/blob/5dc9490bb35f9729ef2c95d00a19ccd30c26339c/aider/repomap.py
[H1]: https://github.com/OpenHands/OpenHands/blob/a8c05584ec6bb063a0857460b9cbff48e136919f/README.md#L63-L148
[H2]: https://github.com/OpenHands/software-agent-sdk/tree/0a9abc87641ad7ffe02e2dadf5e2cb3976b35217/openhands-sdk/openhands/sdk/context/view/properties
[H3]: https://github.com/OpenHands/software-agent-sdk/blob/0a9abc87641ad7ffe02e2dadf5e2cb3976b35217/openhands-sdk/openhands/sdk/conversation/stuck_detector.py#L18-L84
[H4]: https://github.com/OpenHands/software-agent-sdk/blob/0a9abc87641ad7ffe02e2dadf5e2cb3976b35217/openhands-sdk/openhands/sdk/conversation/event_store.py#L34-L130
[R1]: https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-agent/src/run/mod.rs#L1-L5
[R2]: https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-core/src/transcript.rs#L45-L108
[P1]: https://github.com/agentclientprotocol/rust-sdk/blob/7ae02c50a37d79077ff331667315171731f114c1/src/agent-client-protocol/Cargo.toml
[P2]: https://github.com/modelcontextprotocol/rust-sdk/tree/ae2f9c9b45a2c98d24ee345406e79f507c9f9282
[G1]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/Cargo.toml
[G2]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-agent/src/machine.rs#L79-L170
[G3]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/agents/state_machine/session.rs#L40-L140
[G4]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/agents/platform_extensions/developer/edit.rs#L157-L191
[G5]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-provider-types/src/token_counter.rs
[G6]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/context_mgmt/mod.rs
[G7]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-providers/src/ollama.rs#L577-L617
[G8]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-provider-types/src/formats/ollama.rs
[G9]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-provider-types/src/retry.rs
[G10]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/subprocess.rs#L12-L80
[G11]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/agents/state_machine/tests/reconstruction_isolation_lifecycle.rs
[G12]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/permission/permission_judge.rs
[G13]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/security/mod.rs#L25-L95
[G14]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/agents/state_machine/ops_toolcalling.rs#L906-L1024
[G15]: https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-cli/src/session/mod.rs#L146-L171
[O1]: https://github.com/anomalyco/opencode/tree/a79ecfe109294909a239c98fb89f02979d5aa10b/packages
[O2]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/opencode/src/tool/edit.ts
[O3]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/opencode/src/tool/apply_patch.ts
[O4]: https://github.com/anomalyco/opencode/tree/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/opencode/src/lsp
[O5]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/opencode/src/session/compaction.ts
[O6]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/opencode/src/session/retry.ts
[O7]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/opencode/src/permission/index.ts
[O8]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/opencode/src/snapshot/index.ts
[O9]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/opencode/src/tool/task.ts
[O10]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/core/src/tool/edit.ts#L161-L196
[O11]: https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/core/src/file-mutation.ts#L148-L158

Additional package and source evidence:

- [Goose CLI manifest](https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-cli/Cargo.toml).
- [Goose typed agent events](https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-agent/src/events.rs).
- [Goose headless output-format options](https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose-cli/src/cli.rs#L301-L321).
- [Goose compaction operation](https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/crates/goose/src/agents/state_machine/ops_compaction.rs#L173-L253).
- [Goose root license](https://github.com/aaif-goose/goose/blob/920313e4ee26418258b07de124f23d10d9368b2a/LICENSE).
- [OpenCode root license](https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/LICENSE).
- [OpenCode newer built-in tool registry and parity caveats](https://github.com/anomalyco/opencode/blob/a79ecfe109294909a239c98fb89f02979d5aa10b/packages/core/src/tool/builtins.ts#L26-L37).
- [Rig released package/run documentation](https://docs.rs/rig-agent/0.43.0/rig_agent/run/index.html).
- [Rig release manifest](https://github.com/0xPlaygrounds/rig/blob/654567eb64274fca00cab86cdd32c86b9913769e/crates/rig-agent/Cargo.toml).
- [ACP published package documentation](https://docs.rs/agent-client-protocol/2.2.0/agent_client_protocol/).
- [ACP workspace/MSRV/license](https://github.com/agentclientprotocol/rust-sdk/blob/7ae02c50a37d79077ff331667315171731f114c1/Cargo.toml).
- [MCP published package documentation](https://docs.rs/rmcp/3.5.0/rmcp/).
- [MCP feature manifest](https://github.com/modelcontextprotocol/rust-sdk/blob/ae2f9c9b45a2c98d24ee345406e79f507c9f9282/crates/rmcp/Cargo.toml).
- [MCP actual license material](https://github.com/modelcontextprotocol/rust-sdk/blob/ae2f9c9b45a2c98d24ee345406e79f507c9f9282/LICENSE).
- [Codex patch defaults](https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/apply-patch/src/lib.rs#L62-L86).
- [Codex permissive matching](https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/apply-patch/src/seek_sequence.rs#L1-L114).
- [Codex token-budget compaction lifecycle](https://github.com/openai/codex/blob/6ece7bfc21bc28c1ae4e7d7aec8ca828b2b6e3c0/codex-rs/core/src/compact_token_budget.rs).
- [Codex non-interactive documentation, live](https://developers.openai.com/codex/noninteractive).
- [OpenHands SDK repository boundaries](https://github.com/OpenHands/software-agent-sdk/blob/0a9abc87641ad7ffe02e2dadf5e2cb3976b35217/README.md#L30-L87).
