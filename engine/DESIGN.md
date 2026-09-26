<!-- kvist-design-version: 1 -->

# Kvist Engine Design

## Design overview

The root crate separates CLI parsing and dispatch from testable library
modules. Domain modules own configuration, filesystem safety, artifacts,
component documents, discovery, project state, queues, task commands, sandbox
protocol, VCS inspection, agent integration, conversion, and presentation.
`main.rs` only converts the library result into process output and exit status.

Kvist depends on the standalone `agent-runtime` crate for reusable provider and
process mechanisms while retaining task policy, authority, context, and
evidence.

## Internal structure

| Area              | Modules                                                                                                            | Responsibility                                                                                                                      |
| ----------------- | ------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| CLI boundary      | `cli`, `main`, `error`                                                                                             | Typed grammar, dispatch, JSON/text output, domain errors                                                                            |
| Durable artifacts | `artifacts`, `component_documents`, `file_io`, `filesystem`                                                        | Templates, Markdown validation, bounded safe reads, atomic writes                                                                   |
| Project model     | `config`, `discovery`, `project_state`, `tree`, `status`, `vcs`                                                    | Configuration, recursive layout, state classification, deterministic reports                                                        |
| Workflow          | `task_queue`, `task_commands`, `sandbox`                                                                           | Queue schema, lifecycle, locks, policy approval, runner protocol, evidence                                                          |
| Agent integration | `agent`, `prompt_input`, `wizard`                                                                                  | Role selection, prompt sources, standalone runtime integration                                                                      |
| Interactive shell | `shell` (`completion`, `journal`, `locks`, `pager`, `prompt_editor`, `runs`, `state`, `status`, `stream`, `style`) | Reedline host, clap-derived completion tree, dynamic state snapshot, builtins, journal, lock inspection, paging, streaming, theming |
| Onboarding        | `init`, `convert`, `import`, `reverse_discovery`                                                                   | New projects and explicit source-derived drafts                                                                                     |

`engine/`, `agent_runtime/`, and `sandbox_runner/` are top-level peer
components under the root Rust workspace manifest (`/Cargo.toml`). Each owns
its own requirements, contract, design, queue, records, tests, and manifest.
`kvist.engine` depends on `agent-runtime` as a Rust library, while
`sandbox-runner` is an independent execution boundary that communicates with
the engine only through the versioned protocol. The one-way migration from the
original `src/` layout is recorded in
[`../docs/decisions/0003-align-rust-workspace-with-components.md`](../docs/decisions/0003-align-rust-workspace-with-components.md).

## Interactions and state

Project inspection classifies root artifacts before loading configuration.
Only a current root permits bounded component discovery. Each discovered
component is then validated in stable artifact order; queue revisions are
compared with local intent documents and the immediate parent contract.

Task mutation follows:

1. normalize the component-root-relative target;
2. inspect project, component, and VCS state;
3. obtain a user-owned exclusive lock;
4. re-read and validate the queue;
5. validate the requested transition;
6. append `prepared` evidence;
7. atomically replace and synchronize the queue;
8. append `committed` evidence; and
9. release the lock.

Task execution adds policy approval, runner and enforcement-backend identity and
capability checks, explicit authoring implementation and test roots with
read-only intent, root, and immediate-parent contract mounts, agent execution,
output redaction, and implementation test verification.

The target dogfooding path splits this into recovery-safe supervised authoring,
optional mediated dependency acquisition, isolated verification, and explicit
human finalization. Each phase receives a separately canonicalized grant set.

## Algorithms and decisions

Component Markdown validation is intentionally structural rather than a
general Markdown parser. Each document has an exact version marker and ordered,
unique, nonempty required level-two sections; all other content remains
human-authored and preserved.

Discovery sorts directory entries and component paths, applies hard resource
bounds, skips known generated/repository directories, rejects link-like paths,
and recognizes descendants only through required artifact names.

Queue validation parses a version probe before strict typed YAML, rejects
unknown fields and invalid graph/state combinations, then emits canonical YAML
with explicit field order. SHA-256 hashes exact UTF-8 bytes, so VCS-visible
document changes are attributable without hidden normalization.

Explicit prompt execution selects a configured model within the requested role,
then renders typed reasoning effort only through the runtime's declared
placeholder. Text mode uses live bounded supervision and returns no wrapper
message. JSON mode uses bounded capture and emits one escaped `content` value,
replacing invalid UTF-8 sequences with U+FFFD so provider output cannot corrupt
the command's structured response.

Agent setup delegates provider collection and mandatory qualification to the
runtime with a typed force option. The setup invocation supplies authority for
the runtime's bounded provider discovery and fixed-prompt qualification only.
The runtime returns numbered provider model choices with a final custom-entry
escape hatch before constructing the command profile. A successful
qualification continues to role selection and atomic configuration
persistence; a failed qualification exits before those steps unless `--force`
was supplied.
Kvist treats the catalog and selection as child-owned interaction and consumes
only the validated resulting profile, preserving its command bytes during role
materialization.
Forced persistence retains a visible warning, while cancellation remains
terminal. No separate test prompt, host-authority question, or
save-after-failure question is part of the root wizard state machine.
For global JSON presentation, the wizard receives standard error as its
interaction writer and qualification uses bounded capture, leaving standard
output exclusively for the dispatcher's single result object.
Selecting an already saved reusable runtime profile skips provider collection
and qualification and proceeds directly to role binding.

Agent health check parses the selected scope's configuration document (and the
other scope, whose providers lose on name conflict) with the same bounded
document rules and lists the target scope's profiles in deterministic name
order. Each profile's test command is its explicit `command` when present,
otherwise the merged provider's synthesized template. Testing reuses
`agent_runtime::verify_profile` with the fixed setup prompt, so supervision,
capture bounds, and diagnostics match setup qualification exactly; no new
execution path is introduced. The interactive loop reuses the wizard's
cancellation-aware input handling, and per-failure removal reuses the standard
profile removal, so cancellation semantics and configuration edits are the
same as the dedicated remove command.

External agent turn execution selects a configured model within the role, then
resolves the command's numeric loopback gateway endpoint. A command that does
not target a numeric loopback gateway is refused with `AgentCommandNotModelGateway`
before any transport work, so the model turn always runs on the host against a
loopback gateway and no agent command is ever spawned outside the effect
sandbox. The endpoint is liveness-probed with a bounded TCP connect before
turning; the probe never loads or selects a model. When the gateway accepts, the
turn is dispatched through the runtime transport and advertises the closed
authoring tool set. The broker reduces the turn's untrusted tool intents to
capability-bound effects under a deny-by-default policy; a dropped intent fails
the turn. Each authorized effect is applied by the engine itself inside the
effect sandbox against a read-only staged-intent mount, so the host never writes
component state for an effect. A turn succeeds only when it produced a usable
result, no intent was dropped, and every authorized effect applied.

The model phase runs under one shared wall-clock budget equal to the configured
profile timeout, covering the liveness probe, every turn attempt, and every retry
backoff; the per-attempt transport deadline is the remaining budget, so the model
phase never exceeds the configured timeout by more than scheduling slack. Only
transient availability failures — a socket connection refused, timed out,
interrupted, or reset error, a slot-allocation timeout, or the overall transport
timeout — are retried, up to three attempts with a short fixed backoff, to ride
out a cold-starting gateway that has not finished spawning the model onto its
ephemeral port. Non-retryable failures (a non-success HTTP status, a malformed
or oversized response, or cancellation) are returned immediately. A streamed
attempt that has already emitted text is never retried. An exhausted retry or a
failed probe yields a single clear, actionable gateway-unreachable error instead
of an opaque transport failure.

Significant artifact separation rationale is retained in
`docs/decisions/0001-separate-component-intent.md`.

### Planned dogfooding execution boundary

The engine replaces the unreleased sandbox protocol's original shape while
retaining protocol version 1. A request is a closed typed value containing the
phase, working directory, argv, environment, network capability, resource
limits, and mount grants. Each mount identifies its canonical source, fixed
sandbox destination, access, purpose, and approval-bound identity. Before the
request is serialized the engine resolves `argv[0]` to an exact executable: a
bare program name is resolved only against a `PATH` explicitly present in the
request environment (no ambient host fallback), an absolute program path is used
directly, the target must be a regular non-symlink executable, and it is
canonicalized and content-hashed. That canonical path replaces `argv[0]`. All
grant source paths are canonicalized so the producer never emits a non-canonical
path the runner would reject. Resource limits use bounded defaults and options
that never exceed the runner's explicit safe maxima; a converted limit that
overflows or exceeds a maximum fails closed rather than saturating. Unknown
fields, purposes, overlaps, aliases, links, special files, and paths outside
approved roots fail before the runner is probed.
Before request serialization or runner execution, the producer also enforces
the runner's lexical argv and environment bounds: 1–1024 argv entries, and at
most 4096 bytes with no NUL per argv entry, environment name, or environment
value; environment has at most 256 entries and names are portable identifiers.
Configuration applies the same count and name restrictions to its environment
allowlist.

The Bubblewrap runner is a child component but is installed as one regular
executable outside the worktree. Engine approval binds the descriptor-launched
runner bytes, Bubblewrap path and digest, kernel capability result, typed
policy, toolchain, command, sources, and grant plan. The request `identities`
bind the exact command bytes, the toolchain identity derived from the exact
resolved `argv[0]` executable bytes (a narrow, internally consistent toolchain
grant for that exact executable; a full immutable toolchain-set approval is
deferred to the later runner integration), and the request policy identity,
which is the authenticated execution-approval digest rather than a locally
computed unapproved hash. The availability probe itself runs under a fixed short
deadline and combined-output cap with process-group termination, never an
unbounded capture. The runner independently parses the request and cannot import
engine types or trust engine path validation as a substitute for its own checks.
The engine drains runner stdout and stderr nonblockingly and fairly under that
single combined cap, polling direct-child status until both streams reach EOF.
The deadline and cap remain active after direct-child exit; either breach kills
the process group, reaps the direct child when necessary, and returns bounded
captured output without waiting for descendants that retained a pipe descriptor.

Authoring and verification use separate filesystem views. Authoring receives
only local component context and explicit writable implementation/test roots.
Workflow artifacts and evidence are read-only or absent. Verification can read
approved workspace metadata, provider source, toolchains, and dependency
caches needed by the build, but those files are not added to the agent prompt
or writable set. Nested child implementation paths are masked from a parent
authoring view unless separately granted.

The dependency phase plans Cargo acquisition without compiling. A future
source-aware network boundary will reauthorize every outbound URL and redirect
against configured registry index/download origins or an exact approved Git
repository and immutable revision, pin trusted resolution, and reject
private/link-local/loopback production addresses. Cargo uses a real
attempt-local `CARGO_HOME`, a distinct writable lockfile workspace, and
separate scratch. Valid content may become one new immutable generation beneath
a trusted provider-owned parent only after bounded descriptor-relative
traversal, checksum, lockfile before/after, source, and link validation.
Verification mounts a selected generation read-only as its `CARGO_HOME`, uses
separate target scratch, and disables network.

The engine implements this as typed, bounded, fallible planning in
`acquisition`: private plan fields expose only validated host-path and
sandbox-path getters. It derives the real attempt-local Cargo home,
lockfile workspace, phase-specific scratch, exact Cargo identity,
domain-separated source identities, and exact Cargo environments. Acquisition
is `cargo fetch` (without `--locked`) so its result binds the old and new
lockfile identities; verification is `cargo test --locked` with
`CARGO_NET_OFFLINE=true`. Supported package sources and bounds are strict
project configuration under `[sandbox.acquisition]`; complete built-in plus
extra name, identity, count, and origin-overlap validation occurs both during
configuration parsing and plan construction. The runner independently
revalidates protocol data. OS mounts, processes, transport enforcement, final
generation selection, and live `task run` wiring remain deferred and valid
requests fail closed.

Supervised execution records an attempt but leaves completion to a separate
human disposition bound to its ID, approved pre-state, scoped post-state, and
verification evidence. It does not retry. The first pilot may write a narrow
live path such as `tests/`; private snapshots and journaled conflict-checked
promotion are required before unattended operation.

Human acceptance creates a canonical acceptance manifest before optional VCS
work. It records accepted path operations and exact pre/post blobs, queue and
evidence outputs, the acceptance source, expected repository head and selected
backend, message bytes, signing mode, and transaction phase. Files not named by
the manifest are never candidates for the commit.

The Git implementation creates a private temporary index outside the worktree,
initializes it from the expected head, stages exact accepted paths including
deletions, and constructs one commit without modifying the user's index.
Before updating the branch it verifies the complete commit tree against the
expected head plus acceptance set and rechecks accepted paths, index overlap,
worktree identity, and head identity. The ref update uses the expected old
object as a compare-and-swap precondition. Temporary index cleanup never
removes user state.

Commit messages are derived from trusted bounded task or document metadata and
include stable acceptance and task trailers. Model output may be offered only
as an explicitly selected user override. Hooks are skipped by default because
they can execute repository code or mutate the worktree; any future hook mode
uses separately approved sandbox execution and revalidates the acceptance set.
Signing uses an explicit off, optional, or required policy. Required signing
failure leaves the accepted set pending commit.

If commit construction, signing, tree verification, or ref update fails after
acceptance, the acceptance manifest advances to a recoverable commit-pending
state. `vcs commit-accepted` revalidates and retries the same manifest; it does
not rerun review, finalization, or promotion. Multiple acceptance sets are not
combined by default. Git is promoted first, while Jujutsu returns a typed
unsupported-backend error until its operation-log and working-copy semantics
have a separate design and evidence chain.

The approved target workflow adds three planned capabilities without changing
the current CLI contract:

- advisory document-review evidence and acceptance;
- observed-intent proposal and advisory comparison; and
- contract-clause traceability verification.

These capabilities are design intent, not claims of current implementation.

### Planned multi-turn agent execution

The current tier drives a single, write-only authoring turn: the model turn runs
on the host, the broker reduces its intents to capability-bound effects, and a
dropped intent or an unapplied effect fails the turn. The target tier replaces
that single turn with a bounded multi-turn loop. This subsection is design intent
for that loop; it does not describe current implementation, and the current
behavior is documented in the dogfooding execution boundary above.

The loop runs a closed authoring tool set: `read_file`, `write_file`,
`edit_file`, `request_dependency`, and `propose_decision`. Each host turn is
classified by the broker, which routes each untrusted intent to a capability-bound
effect under a deny-by-default policy; a dropped intent fails the turn but not
the run. The state machine advances turn by turn through a running state and
terminates in one of complete, awaiting-decision, or fatal. A run reaches
complete only
when the agent reports completion, no decision worthy of intervention remains
surfaced, and every authorized effect applied by the engine. A decision worthy of
intervention terminates the run in awaiting-decision rather than continuing, and
a fatal
transport or gateway failure terminates it in fatal.

Reads execute within a bounded read scope: the whole current project plus
approved dependency source, constrained by per-file, per-turn, and directory-depth
limits. Reads are logged to the per-run trajectory with the run's redaction
values applied, so intermediate investigation is inspectable without being
persisted as an effect. Only brokered write effects are applied by the engine
inside the effect sandbox against a read-only staged-intent mount; the host never
writes component state, and only the final brokered effect of a run is persisted,
preserving durable, inspectable state over in-chat reasoning.

The write scope is the whole current component directory minus the excluded paths
— the five Kvist intent and record documents, the component's `.kvist` state and
evidence, its `.git`, and any sub-component directory — with each excluded
document mounted read-only as context. Sub-components are detected by the same
adjacency rule used for component status, so a child never inherits a writable
view of a sibling or of its parent. When an exclusion cannot be correctly
identified, or when no writable path remains, the run fails closed rather than
granting the component root.

`request_dependency` is routed to the dependency phase for evaluation rather than
executed inline. While the agent is in the acquisition phase, a request within
policy is fetched automatically so the agent continues without interruption; a
request outside policy is surfaced to the user as a decision worthy of
intervention and stops the run in awaiting-decision. Policy covers the
configured supported
registry and exact revision origins and rejects private, link-local, loopback, and
unverified production addresses.

`propose_decision` records an impactful question for the user and ends the run by
placing the component in an awaiting-decision state, where it remains for further
implementation until the decision is resolved. Only decisions that substantially
alter the implementation and are not already covered by the component's
REQUIREMENTS, CONTRACT, DESIGN, or TODOS are worthy of intervention; trivial
issues do not block and are left for the post-hoc advisory comparison of IMPL.md
with the existing intent. Human finalization gates completion and commit, but the
run itself proceeds unattended between turns; the awaiting-decision state is the
sole in-run blocking mechanism for impactful, uncovered decisions. The running
task holds this status while the component is paused; it is a task-level state
distinct from failure (`blocked`). Once the decision is accepted, `TODOS.yaml`
gains the tasks needed to implement it, `IMPL.md` becomes stale, and the component
stays awaiting-decision until an updated advisory review is performed and accepted.

### Implemented proposal and dependency-request tools (phase 4)

Phase 3 drove the bounded multi-turn loop but only brokered the two write
tools (`write_file`, `edit_file`) and terminated a run on a usable answer, a
dropped intent, or an unapplied effect. This subsection documents the
implemented tier that closes the remaining target-tier gap: the two read-only-of-
intent tools the multi-turn contract names, `propose_decision` and
`request_dependency`, wired to the `AwaitingDecision` state, plus the prompt
that advertises them. The authoritative enumeration of allowed agent actions
remains the closed [`ALLOWED_TOOLS`](crate::authoring::ALLOWED_TOOLS) set in the
broker; a change to that set is a change to that code and its tests, never to an
untrusted agent.

The broker ([`engine/src/authoring`](src/authoring/mod.rs)) routes each untrusted
intent through the single funnel [`classify_intent`](src/authoring/mod.rs) to one
of four outcomes: an authorized write effect ([`CheckedIntent`]), a surfaced
decision ([`ProposedDecision`]), an accepted dependency request
([`DependencyRequest`]), or a dropped intent ([`DroppedIntent`]). The
[`AuthoringPlan`](src/authoring/mod.rs) now carries `decisions` and
`dependency_requests` alongside `effects` and `dropped`.

`propose_decision` takes `summary`, `why`, and an optional `patch`. It never
writes a protected intent document; instead the engine records a bounded,
redacted proposal as durable, inspectable evidence under
`<component>/.kvist/authoring/proposals/<id>.json`. When a turn contains a
surfaced decision the loop stops before applying that turn's write effects, sets
`AgentRunResult.surfaced_decision`, and the run harness
([`run_task`](src/task_commands.rs)) transitions the task to `AwaitingDecision`
with a reason rather than running verification or jumping to `Completed`. Only
decisions that substantially alter the implementation and are not already covered
by the component intent are surfaced; trivial issues do not block and are left
for the post-hoc advisory comparison of `IMPL.md`. Whether an issue is trivial
or decision-worthy is the tested decision boundary; trivial issues never pause
the run.

`request_dependency` takes `name` and `origin` and evaluates the origin against
the dependency policy in [`evaluate_dependency_request`](src/authoring/mod.rs).
An in-policy request (an exact, pinned registry revision, or a public VCS origin
with an exact pinned revision; never a private, link-local, loopback, or
unverified production address, and never a wildcard or unpinned range) is
recorded and accepted so the agent continues without interruption; the existing
build/verification step acquires the exact revision. An out-of-policy request is
surfaced as a decision and stops the run in `AwaitingDecision`. A dedicated
in-run acquisition phase (fetching before the next turn) is a documented
follow-up, so a request within policy is recorded and surfaced for acquisition
rather than fetched inline in this tier.

The model is advertised exactly these tools, with schemas and the scope and
protected-document notice, by [`authoring_tool_definitions`](src/authoring/mod.rs)
and the authoring-scope directive in the task prompt. Trivial-issueness, the
decision-worthy-of-intervention test, and the dependency-origin policy are unit
tested in the broker and the run harness; the end-to-end pause-then-await is
exercised by an integration test that drives the loop with a mock gateway that
proposes a decision.

#### Whole-component write scope (phase 4b, deferred)

The target writable scope is the whole current component directory minus the
excluded paths (the five protected documents, `.kvist`, `.git`, and any
sub-component directory), with `write_file` retaining full overwrite semantics.
This tier keeps the existing `src`/`tests` scope because the write scope is
enforced by two coupled layers — the broker's `WRITABLE_ROOTS` and the sandbox's
`AUTHORING_WRITABLE_ROOTS` plus its read-write grant mount plan in
[`append_authoring_grants`](src/sandbox.rs) — and the approved-write-scope
digest in [`approved_write_scope`](src/task_commands.rs). Widening the broker
alone would authorize effects the sandbox would then refuse to apply, so the
widening is a separate phase that changes both layers and the grant enumeration
together, then re-tests the dogfood boundary suite. See
[`REQUIREMENTS.md`](REQUIREMENTS.md) for the recorded follow-up.

### Pinned Rust toolchains and host provisioning (ADR-0012)

The offline Cargo verification topology needs an immutable toolchain. The
toolchain is a pinned, host-provisioned artifact with durable state, managed
exactly like the vendored registry:

- **Pin source.** `rust-toolchain.toml` (TOML: `[toolchain] channel = "..."`,
  the rustup-native file) takes priority over the plain `rust-toolchain` file
  (a bare channel name) at the project root. Both are read with the same
  bounds as other project artifacts (bounded size, regular non-link files).
  Channel syntax is validated to rustup-supported forms: exact versions
  (`1.95.0`, `1.95`), `stable`, `beta`, `nightly`, and dated variants
  (`nightly-2026-01-01`, `stable-2026-01-01`, `beta-2026-01-01`). Anything
  else fails closed with an actionable message naming the offending pin.
- **Channel-explicit resolution.** `rustup which cargo --toolchain <channel>`
  resolves the exact cargo path for the pinned channel, independent of the
  process working directory and ambient rustup overrides; the resolved path
  then passes the existing toolchain-layout validation (`cargo_toolchain_from_
path`: `<root>/bin/cargo`, a complete rustlib layout, cargo beneath the
  root). Without a pin, the rustup default is resolved as today and recorded
  as `default` in the manifest.
- **Provisioning step (host, outside the sandbox).** `kvist toolchain ensure
[PROJECT_DIR]` resolves the effective channel, runs `rustup toolchain
install <channel>` when the channel is not present in `rustup toolchain
list` (the supported upgrade/downgrade path; bounded output capture, host
  network), re-resolves and validates the layout, and records the manifest.
  This step is the only supported toolchain change path. Builds and
  verification never invoke `rustup toolchain install`.
- **Manifest.** `.kvist/rust-toolchain.json` (schema version 1) records:
  schema version, the pinned channel (`"default"` when no pin file exists),
  the canonical toolchain root, the canonical cargo path, the cargo
  executable's SHA-256 content digest (the identity the sandbox request
  derives), and a provisioning timestamp. The manifest lives under the
  Kvist-owned, gitignored `.kvist/` directory: the durable, version-controlled
  catalogue is the pin file itself.
- **Enforcement on use.** `run_offline_cargo_verification` resolves the
  toolchain from the pin (channel-explicit). When the manifest exists, the
  resolved toolchain root and cargo digest MUST match the recorded values; a
  mismatch fails closed with an actionable message ("toolchain drifted; run
  `kvist toolchain ensure`"). When the manifest is absent, resolution proceeds
  without a recorded baseline (today's behavior), so the feature is additive.
  A pinned channel that is not installed fails closed with an actionable
  message naming `kvist toolchain ensure`.

**Authoring-phase toolchain gap.** The authoring phase cannot receive the
offline Cargo topology under the current shared runner contract: the runner's
phase-purpose validation permits only `Context`, `Authoring`, `Toolchain`,
and `Scratch` purposes in authoring, rejects `Toolchain::Cargo` outside Cargo
phases, and rejects a declared Cargo cache in authoring. The vendored
registry, sandbox cargo config, and runtime bin therefore cannot be mounted
for authoring. Two paths exist: (a) extend the runner contract (protocol
change plus conformance updates in `sandbox_runner` and the `agent_runner`
serialization) to permit the read-only Cargo purposes in authoring, or (b)
carry the vendored registry, cargo config, and runtime bin under the already
permitted `Context` purpose with a `Toolchain::System` toolchain block rooted
at the toolchain destination and a writable `Scratch` serving as
`CARGO_HOME`/`CARGO_TARGET_DIR` — no protocol change, but the request builder
must be engine-side and the resulting topology is a documented variant of the
verification topology. Path (b) is the first candidate because it reuses the
existing closed purposes and the pinned-toolchain manifest.

### Language support, per-language provisioning, and evidence

`detect_language_strategy` selects the owning language by lock file (Rust
first, then Python, JavaScript, C/Conan). `verify_task` routes Rust to the
closed offline Cargo topology; every other language runs the generic
approved-test-command path in the network-denied sandbox against host system
toolchains (the runner mounts `/usr`, `/lib`, `/lib64`, `/bin`, and `/sbin`
read-only; writable space is the `/tmp` and `/run` tmpfs; no `$HOME`).

The non-Rust vendoring strategies are implemented at the enforcement layer
(detection, lock-file digest identity, vendored-content presence, mount
planning with per-language offline configs: `pip.conf` at `/workspace/.pip`,
`.npmrc` at `/workspace/.npm`, `OFFLINE.md` at `/workspace/.conan`). The
remaining wiring, in intended order:

1. **Go (easy).** No Kvist machinery: `go mod vendor` on the host commits
   `vendor/` inside the component, and `go test -mod=vendor ./...` runs
   fully offline against the system `go`. The test-policy environment
   allowlist must include `GOCACHE` and `TMPDIR` pointing at `/tmp`.
   Evidence: a Go e2e test in the shape of
   `offline_cargo_verification_e2e` (provision on the host, run the real
   test offline in the sandbox, self-skip without the live sandbox).
2. **JavaScript (easy).** Provision `package-lock.json` entries into
   `.kvist/vendored-js` on the host (`npm pack` per locked entry, or an
   offline npm cache seed), then wire the existing `JavaScriptStrategy`
   mounts through the generic path's `read_only_mounts` parameter with the
   test policy running `npm ci --offline` (or against a committed
   `node_modules`) and the project test command.
3. **Python (important).** Provision locked wheels into
   `.kvist/vendored-python` on the host (`pip download`/`uv` fetch against
   the lock file), wire the existing `PythonStrategy` mounts, and run an
   offline `pip install --no-index --find-links` plus the project test
   command inside the sandbox. Limitations to document: interpreter version
   must match the host system `python3` (no per-project interpreter
   provisioning yet), C-extension packages must be manylinux wheels, and
   build backends with their own network needs require vendored build
   dependencies.
4. **C/C++ (important).** System `gcc`/`cc` plus system packages works today
   via the generic path; Conan support reuses the existing `CConanStrategy`
   (lock file plus presence, `--offline` install against the vendored
   package directory).

Before any non-Rust language is claimed as vendored-supported, its strategy
MUST verify per-package presence against the lock file (the current
directory-non-empty check is a placeholder) and an end-to-end integration
test MUST pass. `kvist vendor` dispatches per strategy once each language's
provisioning is implemented; until then it is Rust-only and fails with an
actionable message for other languages.

### Planned review evidence and acceptance state

The initial review subject is a digest-bound bundle containing exact local
`REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` bytes plus a canonical
projection of `TODOS.yaml` task definitions. The projection contains only each
task's `id`, `title`, `description`, `context`, `purpose`, `expected_outcome`,
`kind`, `depends_on`, and `requirements`. Canonicalization excludes all
`component` metadata plus task `status`, `timestamps`, `blocked_reason`, and
`recovery_state`, so acceptance and task execution do not invalidate the
review.

A separate review operation will construct the strict context from that bundle,
`ROOT_CONTRACT.md`, and the immediate parent `CONTRACT.md`. It will use the
existing shell-free, bounded, approved or explicitly acknowledged agent path.
It will not expose peer artifacts or parent internals. Separate authoring and
reviewing contexts are preferred, but the resulting provenance is not treated
as proof of independence or quality.

Kvist, not the model, will write a versioned receipt from execution evidence
under component-local `.kvist/reviews/`. The receipt will bind exact target
digests and scope, reviewing and authoring context identity when known,
provider/profile/model/tool identity, Kvist version, timestamp, and the digest
and path of a bounded redacted report. Human acknowledgement or a per-bundle
exception is recorded with it. An exception records actor, timestamp, reason,
and exact digests. Review defaults to required when its configuration is
absent, and generated project templates state `[review] required = true`
explicitly. Setting it to `false` is the visible persistent opt-out; absence of
a configured agent in a review-required project requires an explicit exception
rather than an implicit pass.

Receipt and report files are VCS-trackable durable state but remain outside the
five-artifact set. Discovery ignores them for component candidacy, revision
staleness, task context, and compliance evidence. Generated evidence such as
`IMPL.md`, compliance reports, review reports, status, and attempt logs is
exempt. Generated intent drafts are not exempt.

When project review is required and no exact-bundle exception exists, missing
current review evidence or acknowledgement blocks target acceptance. A visible
project opt-out disables that per-bundle gate. Review findings are retained for
the human to consider but have no blocking severity and cannot determine
compliance. `component accept` continues to be deterministic and local; it
consumes valid evidence but never spawns an agent or performs network I/O.
Project-level documents and referenced native schemas need a later
project-level acceptance state rather than being folded into component
discovery.

### Planned observed-intent workflows

The `propose intent`/`derive draft` path starts from an independently generated
`IMPL.md` and writes no-clobber draft `REQUIREMENTS.md` and `DESIGN.md`.
Because observed behavior cannot recover stakeholder intent, drafts identify
uncertainty and omitted decisions. The path never produces a normative
`CONTRACT.md`.

A separate advisory comparison reads `IMPL.md` and existing intent, emits
differences without modifying either, avoids compliance verdict vocabulary,
and is not compliance evidence. Both outputs remain subject to human review and
the normal advisory-review-or-exception acceptance gate. This path is separate
from `reverse_discovery`, which analyzes source for onboarding and may produce
a non-normative draft contract.

### Planned contract verification

Contract verification begins with stable clause locators derived from existing
heading anchors. Explicit clause IDs require a later explicit format/version
decision rather than an unversioned syntax change. Test metadata maps tests to
clauses, approved execution provides bounded results, and a traceability report
identifies clauses with passing evidence, failed evidence, or no linked test.

The report is not code coverage and tests are not proof. Independent compliance
still compares `CONTRACT.md`, `IMPL.md`, and test evidence.

### Interactive shell design

The shell hosts a reedline line editor (Emacs mode, IDE completion menu) over
the exact `clap` command surface: `shell/tree.rs` introspects the same
`clap::Command` the parser uses into a static node tree, so static completion
(verbs, flags, enum literals) cannot drift from parseable commands. Shell
builtins (`cd`, `tasks`, `run`, `help`, `last`, `history`, `journal`, `locks`,
`exit`/`quit`) are appended to the root node for completion and are dispatched
before the line is re-parsed by clap; `prompt TASK_ID` is intercepted as the
explicit authoring flow and submits only after confirmation.

Dynamic values are snapshotted into `shell/state.rs` after every command:
component paths, queue tasks, attempt journals, model profiles, and the active
Git branch. Each source degrades independently to empty, so a missing
configuration or an unparsable queue never aborts the editor loop. The
completion engine (`shell/completion.rs`) is a pure function of the line and
cursor position and is fully testable without a terminal.

The session journal is JSONL at `.kvist/session.log`, appended one line at a
time with `O_APPEND` so no session can overwrite earlier entries; the editor
history is reedline's file-backed history at `.kvist/history`. Both are local
state and are excluded from compliance evidence.

Cancellation is process-level: the shell installs one `sigaction` handler for
SIGINT/SIGTERM through the shared `agent_runtime::interrupt` registry. The
handler sets an atomic flag and forwards the signal to the currently active
process group, which the sandbox supervisor and the runtime supervisor register
around each child (both spawn with their own process group). The supervision
loop observes the flag, grants a short grace period, and escalates to
process-group termination, returning a typed cancelled result so durable task
state stays explicit.

Streaming task execution threads an optional live-stdout relay sink through
the runner supervision loop: each drained chunk is echoed to the terminal
while the bounded capture continues unchanged, so evidence and live output
cannot diverge. A deferred spinner (visible only after a short grace and only
on a real terminal) covers commands that produce no output of their own.

Task-lock inspection parses the user-state lock files, treats their contents
as untrusted bounded input, and classifies each lock live or stale by process
liveness; the prompt, banner, and status line report the two counts
separately.

Presentation lives in `shell/style.rs`: a `Theme` resolves once per session
from `NO_COLOR` (any value), `CLICOLOR`, `CLICOLOR_FORCE`, `TERM=dumb`, and
terminal detection, and every renderer is a pure function of its inputs plus
the theme, so styled and plain output are unit-testable without a terminal.
Styled table cells are padded on visible (ANSI-stripped) width so columns
stay aligned. `titled_box` sizes its box to the content, applies the
40-column floor and the terminal-width cap, and extends the box when content
is wider than the cap so nothing is ever truncated. The prompt shows the
branch, the component focus (set by `cd`), a failure marker after a failed
command (cleared by the next success; cooperative cancellations are not
failures), and the green `❯` indicator; a dimmed status badge on the right
reports the sandbox backend, lock counts, and default model. The welcome
banner is a titled box with aligned labels and a key-hint line. Ctrl+L is
bound to the editor's clear-and-redraw event. Long non-streaming output is
paged through `minus` only when the pure `pager_policy` (terminal height
minus the prompt row, 15-line fallback, `KVIST_NO_PAGER` override) says it
would scroll; a pager that cannot start falls back to direct printing so
output is never lost. Completion attaches rich descriptions to dynamic
values (task status and title, a next-ready star in run contexts, the
current component), and the walker treats a complete `--` token as the end
of flag parsing, so Tab after `--` completes positionals only. Editor launch
retries the transient Linux `ETXTBSY` ("text file busy") a bounded number of
times before reporting it.

## Failure and recovery

Readers return contextual domain errors or read-only invalid states. Writers
never repair malformed input. Same-directory temporary files prevent partial
single-file replacement, while callers acknowledge that initialization and
multi-document creation are not multi-file transactions.

Task locks live in protected user state outside the repository and sandbox.
The lock identity hashes canonical project and component paths. Drop attempts
cleanup for in-process failure, but externally retained locks and incomplete
prepared evidence require explicit recovery rather than guessing.

The target attempt journal adds a unique ID, pre-queue and intended-post-queue
digests, policy and runner identities, scoped filesystem preconditions, and
durable phase markers. Recovery finalizes only an exact known state or records
that execution provably did not begin. Every other case remains fenced and
requires a human disposition; it never performs a destructive VCS reset.
Recovery is a sequence of individually durable journal append and atomic
single-file queue replacement steps, not a multi-file transaction. A signed
recovery-prepared decision permits an interrupted invocation to complete only
the exact fenced or recovered queue state it names; any changed input remains
inspectable and refused.
Queue writers assess every task journal while holding the component lock.
Only the exact recovery operation may proceed while its named attempt is
unresolved. Once a fully authenticated recovery chain records its recovered
queue digest, it remains terminal audit history rather than requiring all
future legal queue revisions to retain that digest.

Acceptance and commit journals are separate. Acceptance does not become false
because the VCS operation failed. Recovery reports the accepted-but-uncommitted
state and exact retry command without silently committing a broadened or
changed set.

Sandbox runner launch validates file identity and uses descriptor-bound Linux
execution to reduce time-of-check/time-of-use substitution. Timeout and output
overflow terminate the runner process tree and become explicit failures.

An external model turn to a loopback gateway is liveness-probed with a bounded
TCP connect before it turns; only transient availability failures are retried
a bounded number of times, and a gateway that never accepts a connection fails
fast with a clear, actionable error rather than an opaque transport failure.

## Security and resource design

All filesystem entry points inspect metadata without following final links,
enforce regular-file and size rules, and avoid shell interpolation. VCS
inspection is read-only and preserves non-UTF-8 paths internally.

Project-controlled sandbox configuration cannot approve itself. Approval state
is authenticated in user-owned storage and bound to exact project, worktree,
root-contract, runner, Bubblewrap backend, toolchain, source policy, command,
grant plan, and resource identity. Environment inheritance is allowlisted.
Authoring and verification deny network. Dependency acquisition has a separate
source-limited capability and writes only approved attempt-local state.

Direct prompt and setup streams are bounded but are not retained as durable
evidence by the root component. Task subprocess output is bounded and literal
configured redactions apply across output chunks and streams before task
evidence is persisted. Errors avoid secrets and success-shaped fallbacks.

Planned review reports apply the same bounded-output and redaction rules. Raw
transcripts, credentials, and secrets are not retained, and untrusted model
output cannot authorize acceptance or mint canonical receipts.

## Verification strategy

Pure parsing, validation, hashing, ordering, transitions, and serialization use
unit tests. CLI grammar, filesystem layouts, initialization, conversion,
import, status, VCS, queues, locking, evidence, policy approval, process
supervision, timeout, redaction, and sandbox requests use integration tests.

Linux-only guarantees require native Linux tests. New component-document work
must cover all three templates, line-aware diagnostics, no-clobber creation,
five-artifact discovery, separate staleness causes, parent-contract
propagation, component context paths, conversion/import/reverse-discovery, and
independent compliance evidence.

Dogfooding-boundary tests must additionally use the real Bubblewrap runner to
cover malformed requests, path aliases and links, hidden workflow state,
read-only intent and child implementations, task-scoped writes, absent home
and Git state, process-tree cleanup, resource exhaustion, attempt recovery,
human finalization, source-limited Cargo acquisition, untrusted archives and
caches, offline locked verification, backend replacement, and refusal to
degrade or fall back. Accepted-change tests must cover exact path sets,
creations, deletions, renames, unrelated dirty and staged state, overlap,
concurrent head movement, detached head, isolated index preservation, commit
tree equality, message bounds, hook suppression, signing failure, commit
journal recovery, no push, and unsupported Jujutsu behavior.

Future review work must additionally test stable task projection, exact-digest
receipt matching, acknowledgement and exception paths, opt-out visibility,
no-agent refusal, report redaction and bounds, `.kvist/reviews/` discovery and
staleness exclusion, strict review context, and evidence that `component accept`
performs no agent or network work. Observed-intent tests must cover no-clobber
drafting, uncertainty, absence of normative contract generation, and
non-mutating comparison. Contract-verification tests must cover locator
stability, test-to-clause mappings, approved execution evidence, and
uncovered/failed clause reporting.
