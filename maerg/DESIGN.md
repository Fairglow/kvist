<!-- kvist-design-version: 1 -->

# Kvist Maerg Design

## Design overview

The root crate separates CLI parsing and dispatch from testable library
modules. Domain modules own configuration, filesystem safety, artifacts,
component documents, discovery, project state, queues, task commands, sandbox
protocol, VCS inspection, agent integration, conversion, and presentation.
`main.rs` only converts the library result into process output and exit status.

Kvist depends on the standalone `sav` crate for reusable provider and
process mechanisms while retaining task policy, authority, context, and
evidence.

## Internal structure

| Area              | Modules                                                                                                            | Responsibility                                                                                                                      |
| ----------------- | ------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| CLI boundary      | `cli`, `main`, `error`, `help`                                                                                     | Typed grammar, dispatch, JSON/text output, domain errors, guided help tour and concept topics                                       |
| Durable artifacts | `artifacts`, `component_documents`, `file_io`, `filesystem`                                                        | Templates, Markdown validation, bounded safe reads, atomic writes                                                                   |
| Project model     | `config`, `discovery`, `project_state`, `tree`, `status`, `repair`, `vcs`                                          | Configuration, recursive layout, state classification, deterministic reports, bounded repair                                        |
| Workflow          | `task_queue`, `task_commands`, `sandbox`                                                                           | Queue schema, lifecycle, locks, policy approval, runner protocol, evidence                                                          |
| Agent integration | `agent`, `prompt_input`, `wizard`                                                                                  | Role selection, prompt sources, standalone runtime integration                                                                      |
| Interactive shell | `shell` (`completion`, `journal`, `locks`, `pager`, `prompt_editor`, `runs`, `state`, `status`, `stream`, `style`) | Reedline host, clap-derived completion tree, dynamic state snapshot, builtins, journal, lock inspection, paging, streaming, theming |
| Onboarding        | `init`, `convert`, `import`, `reverse_discovery`                                                                   | New projects and explicit source-derived drafts                                                                                     |

`maerg/`, `sav/`, and `galla/` are top-level peer
components under the root Rust workspace manifest (`/Cargo.toml`). Each owns
its own requirements, contract, design, queue, records, tests, and manifest.
`kvist.maerg` depends on `sav` as a Rust library, while
`galla` is an independent execution boundary that communicates with
the maerg only through the versioned protocol. The one-way migration from the
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
`sav::verify_profile` with the fixed setup prompt, so supervision,
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
the turn. Each authorized effect is applied by the maerg itself inside the
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

The maerg replaces the unreleased sandbox protocol's original shape while
retaining protocol version 1. A request is a closed typed value containing the
phase, working directory, argv, environment, network capability, resource
limits, and mount grants. Each mount identifies its canonical source, fixed
sandbox destination, access, purpose, and approval-bound identity. Before the
request is serialized the maerg resolves `argv[0]` to an exact executable: a
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
maerg types or trust maerg path validation as a substitute for its own checks.
The maerg drains runner stdout and stderr nonblockingly and fairly under that
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

The maerg implements this as typed, bounded, fallible planning in
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
surfaced, and every authorized effect applied by the maerg. A decision worthy of
intervention terminates the run in awaiting-decision rather than continuing, and
a fatal
transport or gateway failure terminates it in fatal.

Reads execute within a bounded read scope: the whole current project plus
approved dependency source, constrained by per-file, per-turn, and directory-depth
limits. Reads are logged to the per-run trajectory with the run's redaction
values applied, so intermediate investigation is inspectable without being
persisted as an effect. Only brokered write effects are applied by the maerg
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

The broker ([`maerg/src/authoring`](src/authoring/mod.rs)) routes each untrusted
intent through the single funnel [`classify_intent`](src/authoring/mod.rs) to one
of four outcomes: an authorized write effect ([`CheckedIntent`]), a surfaced
decision ([`ProposedDecision`]), an accepted dependency request
([`DependencyRequest`]), or a dropped intent ([`DroppedIntent`]). The
[`AuthoringPlan`](src/authoring/mod.rs) now carries `decisions` and
`dependency_requests` alongside `effects` and `dropped`.

`propose_decision` takes `summary`, `why`, and an optional `patch`. It never
writes a protected intent document; instead the maerg records a bounded,
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
- **Provisioning step (host, outside the sandbox).** `kvist toolchain
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
  `kvist toolchain`"). When the manifest is absent, resolution proceeds
  without a recorded baseline (today's behavior), so the feature is additive.
  A pinned channel that is not installed fails closed with an actionable
  message naming `kvist toolchain`.

**Authoring-phase toolchain gap.** The authoring phase cannot receive the
offline Cargo topology under the current shared runner contract: the runner's
phase-purpose validation permits only `Context`, `Authoring`, `Toolchain`,
and `Scratch` purposes in authoring, rejects `Toolchain::Cargo` outside Cargo
phases, and rejects a declared Cargo cache in authoring. The vendored
registry, sandbox cargo config, and runtime bin therefore cannot be mounted
for authoring. Two paths exist: (a) extend the runner contract (protocol
change plus conformance updates in `galla` and the `skott`
serialization) to permit the read-only Cargo purposes in authoring, or (b)
carry the vendored registry, cargo config, and runtime bin under the already
permitted `Context` purpose with a `Toolchain::System` toolchain block rooted
at the toolchain destination and a writable `Scratch` serving as
`CARGO_HOME`/`CARGO_TARGET_DIR` — no protocol change, but the request builder
must be maerg-side and the resulting topology is a documented variant of the
verification topology. Path (b) is the first candidate because it reuses the
existing closed purposes and the pinned-toolchain manifest.

### Language support, per-language provisioning, and evidence (ADR-0013)

`detect_language_strategy` selects the owning language by lock file (Rust
first, then Go, Python, JavaScript, C/Conan, C/vcpkg). `verify_task` routes Rust to the
closed offline Cargo topology; every other **vendored** language is routed to
the shared offline language topology
(`language_verification::run_offline_language_verification`); non-vendored
projects keep the generic approved-test-command path in the network-denied
sandbox against host system toolchains.

**The shared offline language topology.** Vendoring is enforced first (missing,
incomplete, or stale material fails closed — never a fallback to a host
build), then the language's vendored material is mounted read-only with the
lock-file digest identity, one disjoint writable scratch at the Cargo
topology's fixed destination (`/workspace/scratch`) absorbs caches, build
output, and `HOME`, the network is denied, and the language's canonical
offline test command runs against the host system toolchain (the sandbox
`PATH` is the fixed `/usr/bin:/bin`; the language binary is located on the
host and canonicalized so the toolchain grant binds a regular, non-symlink
executable, preferring a PATH candidate under a bound system prefix —
`/usr`, `/lib`, `/lib64`, `/bin`, `/sbin` — because only those remain
reachable inside the sandbox while toolchain managers frequently put
installs such as `/opt/hostedtoolcache/...` ahead of the system toolchain
on `PATH`). For Go, `GOROOT` is the grandparent of `bin/go` and must sit
under a bound prefix as well; a go whose `GOROOT` the sandbox cannot reach
fails closed with an actionable message (the in-sandbox symptom would
otherwise be `package <std> is not in std`). An approved test command
drives only the C/C++ profile (the build system is project-defined); the
canonical-command languages reject one and fail closed.

**Per-language profiles and provisioning** (`kvist vendor` dispatches per
detected strategy):

1. **Go.** `kvist vendor` runs `go mod vendor`; the committed `vendor/`
   directory travels inside the component mount, so no extra mounts. The
   profile runs `go test -mod=vendor ./...` with `GOPROXY=off`,
   `GOTOOLCHAIN=local`, `GOFLAGS=-mod=vendor`, and `GOCACHE`/`GOTMPDIR`/
   `GOPATH`/`GOMODCACHE` under the scratch (the standard scratch
   subdirectories are pre-created because Go requires them to exist). E2E
   evidence: `language_offline_e2e.rs::go_offline_verification_builds_and_
tests_denied_network`.
2. **JavaScript.** `kvist vendor` reconciles the locked graph for the detected
   package manager into `.kvist/vendored-js`: `npm ci` into a tarball cache
   (`package-lock.json`), `yarn install --frozen-lockfile` into a yarn cache
   (`yarn.lock`), or `pnpm install` vendoring the content-addressable pnpm
   store (`pnpm-lock.yaml`, located with `pnpm store path` and copied into
   `.kvist/`). The profile runs `node --test` with the vendored catalogue and
   the generated offline `.npmrc` mounted. E2E evidence:
   `language_offline_e2e.rs::javascript_offline_verification_builds_and_
tests_denied_network`,
   `language_offline_e2e.rs::javascript_pnpm_offline_verification_builds_and_
tests_denied_network`.
3. **Python.** `kvist vendor` downloads the locked wheels into
   `.kvist/vendored-python`, provisions `.kvist/venv` (uv preferred, stdlib
   `venv` fallback), and installs the locked material into it offline; the
   profile runs `python3 -m unittest -v` with `PYTHONPATH` at the mounted
   venv's site-packages (CPython discovers a venv from the `pyvenv.cfg` beside
   its executable, not from `VIRTUAL_ENV`, so the host interpreter is pointed
   at the venv explicitly; `VIRTUAL_ENV` is set for the tools that read it) and
   bytecode/user-site writes disabled. Both `requirements.lock.txt` (pip) and
   `uv.lock` (uv) are supported lock forms; a `uv.lock` project is provisioned
   by exporting its exact resolved graph with `uv export --locked` to a
   pip-format file before the locked wheels are vendored and installed
   offline. C-extension packages must be manylinux wheels, and the
   interpreter version must match the host system `python3`. E2E evidence:
   `language_offline_e2e.rs::python_offline_verification_builds_and_tests_
denied_network`, `language_offline_e2e.rs::python_uv_lock_offline_verification_builds_and_tests_denied_network`.
4. **C/C++ (Conan).** `kvist vendor` runs `conan profile detect` (when no
   profile exists) and
   `conan install . [--lockfile=…|--lockfile-out=…] --build=missing -of
.kvist/conan-build` with
   `CONAN_HOME=.kvist/vendored-conan`; the profile runs the approved project
   test command with `CONAN_HOME` at the Conan home's canonical host path
   (the home is mounted read-only at that same path, so the absolute cache
   paths embedded in the generated toolchain file resolve unchanged).
   The `CMakeToolchain`/`CMakeDeps` generators are not Conan 2 defaults and
   must be requested explicitly, so the generated toolchain file and
   `find_package` material are what the project build system consumes.
5. **C/C++ (vcpkg).** `kvist vendor` runs `vcpkg install [--locked/--lockfile-out] --triplet <triple>`
   with `VCPKG_ROOT=.kvist/vendored-vcpkg`; the profile runs the approved project
   test command with `VCPKG_ROOT` at the vendored vcpkg root's canonical host
   path (the root is mounted read-only at that same path, so the absolute
   install paths embedded in the generated CMake toolchain file resolve
   unchanged). The triple is read from `vcpkg.json` (`x-triplet`) and defaults
   to `x64-linux`. Vendoring the whole vcpkg root (the tool plus the installed
   ports) is what makes offline verification possible; the root must already be
   a provisioned vcpkg installation. E2E evidence:
   `language_offline_e2e.rs::c_vcpkg_offline_verification_builds_and_tests_denied_network`.

Before any non-Rust language is claimed as vendored-supported, an end-to-end
integration test MUST pass (provision a small real project on the host, run
its real build/test offline in the sandbox, self-skip without the live
sandbox). Go, JavaScript, and Python are claimed with that evidence; C/C++ is
tracked in `TODOS.yaml`. JavaScript is evidenced for npm/yarn and for pnpm; C/C++
is evidenced for Conan and for vcpkg.

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
SIGINT/SIGTERM through the shared `sav::interrupt` registry. The
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

Presentation lives in `shell/style.rs` and `shell/theme.rs`. A `Theme` is a
copyable handle `{enabled, id}` into a process-wide theme registry (an
`OnceLock` of theme definitions): the built-in `dark` (id 0, the default) and
`light` (id 1) are always present, and a user theme registers when the user
spec file is loaded (once per process). A `Palette` is 14 semantic SGR roles
(prompt, component, failure, indicator, border, dim, the six status hues, and
the agent result foreground/background); the dark palette renders bright text
on the terminal's background with the agent result on a true black surface
(`37;40`), and the light palette darkens the text with a white agent surface.
Every renderer is a pure function of its inputs plus the theme, so styled and
plain output are unit-testable without a terminal, and the plain theme (the
`NO_COLOR`/`CLICOLOR=0`/`TERM=dumb`/non-terminal degradation, with
`CLICOLOR_FORCE` forcing styling) is the identity. Styled table cells are
padded on visible (ANSI-stripped) width so columns stay aligned. `titled_box`
sizes its box to the content, applies the 40-column floor and the
terminal-width cap, and extends the box when content is wider than the cap so
nothing is ever truncated.

Theme resolution (`Theme::resolve`) prefers, in order: the `KVIST_THEME`
environment variable, the project-local `.kvist/theme` preference, the user
preference at `~/.config/kvist/theme` (honoring `XDG_CONFIG_HOME`), an OSC 11
terminal-background probe, and the dark default. A preference value is
classified as a built-in name, a registered user-theme name, or a path (a
path separator or a `.toml` suffix), and an unresolvable value reports a
warning and falls back to detection. The OSC 11 probe writes the query in raw
mode on `/dev/tty` (via `nix::cfmakeraw` on an `File` handle, so no `unsafe`
is needed), reads a bounded response with a 150 ms timeout, and classifies
the surface by relative luminance (threshold 128); a non-responding terminal
yields the dark default. A user theme is a TOML spec at
`~/.config/kvist/theme.toml`: a `name` (1-32 characters of `[a-z0-9-]`), an
optional `mode` (`dark` or `light`), and a `[colors]` table mapping known
roles to a `#rgb`/`#rrggbb` hex, a named ANSI color, or a 0-255 index;
specs are size-bounded (8 KiB), unknown roles are rejected, and the parsed
overrides are layered onto the mode's base palette. `theme set` persists the
preference to `.kvist/theme` with an atomic tmp-file-plus-rename write.
The `theme` builtin previews every available theme as a live titled box drawn
in the theme itself (prompt line, separator rule, agent result bar, status
badge), and the completion menu shows the same preview as the description of
`theme set` candidates.

The prompt renders a single full-width separator rule (`├─…─`) above the
status line — the one border between the output and the input — then the
short prompt (`kvist <component>/ (<branch>) ✘ ❯ `): the component focus is
set by `cd`, the failure marker (`✘`) appears after a failed command (cleared
by the next success; cooperative cancellations are not failures), and the
`❯` indicator leads. The dimmed status badge (sandbox backend, live/stale
lock counts, default model) sits on the input line's right edge
(`right_prompt_on_last_line`). The most important key hints (`TAB complete ·
↑↓ pick · ESC close · Ctrl+C cancel · Ctrl+D exit`) ride the input line as
dim ghost text when the buffer is empty (reedline renders nothing below the
input line, so the placeholder position is the standard spot): whole groups
are dropped from the tail until the hint fits the width left over for the
short prompt and the current badge, so it never truncates and the badge is
never hidden by the hint. The welcome banner is a titled box with aligned labels including
the active theme; the key hints live on the input line, not in the banner.
Ctrl+L is bound to the editor's clear-and-redraw event.

Streaming task execution renders one `Agent Working` titled box that stays
open across the live output and the result: the result rows are padded to the
box's inner width and styled with the theme's agent-result role (the black
bar in the dark theme), and the box closes once, so the output and the prompt
are separated by the single separator rule. The stream manager's theme is
updated in place on `theme set`, so a running session switches over live.

Long non-streaming output is paged only when the pure `pager_policy`
(terminal height minus the prompt row, 15-line fallback, `KVIST_NO_PAGER`
override) says it would scroll. Interactive paging runs the built-in
full-screen pager in `shell/scroll_pager.rs` (crossterm raw mode, alternate
screen, mouse capture): it re-wraps the text at the content width (terminal
width minus a two-column gutter) and draws a one-column scrollbar on the
right. The render pass draws each content line at column 0 of its own terminal
row (crossterm's `MoveTo(x, y)` takes the column first — the one historical
source of the all-content-on-row-one bug) and the scrollbar symbols in the
last column, so the full terminal height carries content and scrolling
reveals it line by line. The render pass writes to any `Write`, so the
emitted cursor geometry is unit-testable without a terminal. The thumb geometry is a pure, testable function
`scrollbar_geometry(total, visible, offset)` returning `(thumb_top,
thumb_height)`: a document that fits fills the track, a thumb is never
shorter than two rows, and both size and position are proportional, so the
reader always sees how large the output is and where they are. Keys: `q`/
`Esc`/`Ctrl+C` quit, arrows and `j`/`k` one line, `PageUp`/`PageDown` a
page, `g`/`G` and Home/End the ends, `d`/`u` a half page, and the mouse wheel
three lines. On exit the pager reprints the last visible line (like `less`) so
the shell's next prompt follows the content; a pager that cannot take the
terminal (no size, too small, raw-mode failure) returns `false` and the
caller prints the output directly, so output is never lost. The `minus`
dependency was removed. Completion attaches rich descriptions to dynamic
values (task status and title, a next-ready star in run contexts, the
current component), and the walker treats a complete `--` token as the end
of flag parsing, so Tab after `--` completes positionals only. Editor launch
retries the transient Linux `ETXTBSY` ("text file busy") a bounded number of
times before reporting it.

### Guided help and actionable status guidance

`help.rs` is a pure, I/O-free module: `HelpTopic` is a closed
`ValueEnum` set (`concepts`, `lifecycle`, `task-states`), and
`render(Option<HelpTopic>)` is a pure string function. `kvist help [TOPIC]`
(clap variant `Help`) and the shell's `help [TOPIC]` builtin both dispatch to
the same renderer, so the content cannot drift between the two surfaces. Tab
completion is closed-set: the clap-derived completion tree supplies the topic
values for `kvist help`, and the shell builtin `help` node carries the same
closed `TOPIC` positional. Every topic ends with a pointer to the deeper
documentation (`GUIDE.md`, `docs/command-set.md`, or a component's
`CONTRACT.md`), and each actionable line names a real command.

Status guidance in `status.rs` reuses the `TODOS.yaml` parse it already
performs for task progress. For a blocked component it lists each blocked
task with its `blocked_reason` first line in full (the queue parser bounds the
reason to a single 4096-character line, and soft wrapping bounds the display
width, so queue content cannot break the report layout) and derives the exact
next command from
`task_queue::dependencies_completed(task, tasks)`: when the dependency chain
is completed the command is `kvist task run <COMPONENT_DIR> <TASK_ID>`
(blocked → in-progress is legal); otherwise it is `kvist task transition
<COMPONENT_DIR> <TASK_ID> pending` with the count of incomplete dependencies.
Awaiting-decision tasks are listed with a pointer to `kvist help
task-states` because resuming them is a human decision. The details render as
separate indented lines under the overview Action line and as `blocked:` /
`decision:` entries in the stable text report; the JSON report is unchanged.
The overview's per-document change details (`expected`, `observed`, and the
`git diff HEAD` hint) carry the report's left border and align under the
changed document's name, so the block reads as part of the framed report
instead of floating outside it.
The shell's `tasks` table reuses the same `dependencies_completed` /
`incomplete_dependency_count` selection in `next_command_for` and renders one
bounded single-line reason plus one `next:` line per blocked or
awaiting-decision row, so table columns never drift.

`task transition` failures (`transition_error`) enumerate the legal target
statuses from `TaskStatus::can_transition_to` — the closed five-state set in
deterministic declaration order — followed by a pointer to
`kvist help task-states`, so an illegal move is always paired with the legal
moves.

### Project-level diagnostics and bounded repair

`project_state::inspect` already computes a per-artifact verdict for every
root artifact (`ArtifactStatus`, with a class and a human reason); only the
renderers were discarding it. All three status formats now surface it for any
non-current project state: the overview adds a `Project issues` block (each
non-valid artifact with its reason, the project-root diagnostic, the
discovery error, and an `Action` line), the stable text report adds
`root-artifact:` / `root-diagnostic:` lines plus a `Next Step:` line, and the
JSON report adds `root_artifacts`, `root_diagnostic`, and `guidance` fields.
The action is derived from the inspection alone (`project_action_line`):
`kvist init` for uninitialized, the missing artifacts for partial, `kvist
repair` when a TODO queue's only defect is the sorted/unique set-list
violation (detected from the parser's own message), and a manual-repair plus
`kvist doctor` pointer otherwise. No new I/O and no new state.

`repair.rs` owns the single defined rewrite. For each queue enumerated by
`project_state::todo_queue_paths` it first tries the strict
`task_queue::parse`; a parseable queue is never written (it is reported
`unchanged` or `non-canonical`). Only when strict parsing fails does it try
`task_queue::parse_repairable` — the same pipeline with the one
`validate_sorted_unique` check suppressed, so every other invariant is
enforced identically. A repairable parse proves the sole defect is set-list
ordering; `normalize_set_lists` then sorts and deduplicates each task's
dependency and requirement lists (meaning-preserving: the schema already
forces both to be sorted and duplicate-free), and the canonical
`task_queue::serialize` output is written with `file_io::replace_file_
atomically`. Fenced queues (any `recovery_state`) are reported with a pointer
to `kvist task recover` and left untouched, keeping recovery authority
exclusive to the task commands. The report re-inspects the project and lists
whatever non-valid artifacts remain; the command exits nonzero while any
remain, so `repair` doubles as a dry-run check (`--dry-run` previews with the
same exit-code semantics).

### Soft wrapping

`style.rs` owns the pure wrapping primitives so every surface shares one
algorithm. `wrap_line` parses a line into styled pieces (the shell's SGR spans
are a single open plus a reset, so the parser tracks the current code),
splits at word boundaries by _visible_ width, and re-emits each physical line
with the piece styles, so ANSI styling survives wrapping. Continuation lines
are prefixed with the original line's leading whitespace, which keeps text
block indentation; an unbreakable word longer than the remaining width is
hard-split at the width. When the leading whitespace is followed by a `key:`
prefix (a word of letters, digits, `_`, `-`, then a colon and at least one
space), the first physical line keeps the key and every continuation line is
indented to the value's column (key plus separator), so a wrapped `reason:`
field reads as one aligned block. `prepare_display` in `pager.rs` applies the right
primitive per line (bordered for `│`-framed rows, plain otherwise) and is the
identity when the width is zero or unknown. `wrap_bordered_line` handles the
`│content│` lines of the bordered status report: it strips the frame, wraps
the inner content (trimming the right padding first), and re-draws the border
and padding on every physical line, so the report's frame never breaks. Each
primitive is the identity for a line that already fits, so output is
byte-identical until a line actually overflows.

`titled_box` wraps its rows to the box's inner width before sizing, so a box
fits the terminal cap (40-column floor, width-minus-margin or 100-column
cap) and its border stays intact instead of extending past the terminal; the
`Prompt` stage passes the full command line to the box and no longer
truncates it. `display_output` applies the per-line wrapping only when stdout
is a terminal with a probed width, choosing `wrap_bordered_line` for `│`-framed
lines and `wrap_line` otherwise; piped, captured, and width-unknown output is
never transformed, keeping stable report consumers and scripts unaffected.

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
