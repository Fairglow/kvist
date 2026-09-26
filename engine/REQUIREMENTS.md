<!-- kvist-requirements-version: 1 -->

# Kvist Engine Requirements

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

The root component provides the local headless Kvist engine and CLI. It owns
project initialization, component-document validation, bounded discovery,
project status, durable task queues, task lifecycle, Kvist-specific role and
policy selection, sandbox integration, context selection, import/conversion,
and durable execution evidence.

The reusable provider runtime belongs to the child `agent-runtime` component.
Kvist consumes that component's public contract and MUST NOT make its provider
or implementation types part of Kvist's durable formats.

This document defines both implemented requirements and newly approved target
requirements. The current CLI contract remains authoritative for implemented
commands: in particular, `component accept` currently performs structural
validation and revision recording only. The review, intent-proposal, and
contract-verification workflows below are planned and MUST NOT be reported as
implemented until their queues, code, tests, and independent review are
complete.

## Stakeholders and concerns

- Human architects need explicit approval, attribution, and arbitration.
- Component designers need stable requirement and contract identifiers.
- Implementers need bounded local context and deterministic task selection.
- Consumers need contracts without provider implementation details.
- Security reviewers need explicit trust, process, filesystem, network,
  credential, and resource boundaries.
- Compliance reviewers need independently derived observed behavior.
- CLI users and automation need deterministic output and actionable failures.

## Functional requirements

The following stable requirement identifiers define the mandatory behavior of
the root component.

### REQ-ARTIFACT-MODEL

A new project MUST contain versioned `VISION.md`, `ARCHITECTURE.md`,
`ROOT_CONTRACT.md`, configuration, and a root component with
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, and `IMPL.md`.
Every discovered child component MUST use the same adjacent five-artifact set.

### REQ-COMPONENT-DOCUMENTS

Kvist MUST create deterministic requirements, contract, and design templates
without overwriting an existing document. It MUST validate each document's
version marker, required section order, uniqueness, and nonempty content and
return line-aware diagnostics without rewriting user content.

### REQ-DISCOVERY-STATUS

Kvist MUST discover the configured recursive component hierarchy
deterministically within configured hard bounds. Status MUST validate all
adjacent artifacts and report missing, invalid, unsupported, stale, blocked, or
current state without modifying project files.

### REQ-REVISION-PROVENANCE

Each queue MUST record SHA-256 revisions for local requirements, contract, and
design and, for a non-root component, its immediate parent contract. Status
MUST attribute each mismatch separately. Parent requirements or design changes
MUST NOT propagate merely because they are internal to the parent.

### REQ-TASK-QUEUE

`TODOS.yaml` MUST be a strict, versioned, deterministically serialized local
DAG. Tasks MUST have stable IDs, bounded purpose and outcome fields, durable
requirement references, legal lifecycle states, timestamps, and dependencies.
Lifecycle chains MUST order test, implementation, security audit, then
compliance review.

### REQ-TASK-LIFECYCLE

Task selection and transition MUST require a current, completely VCS-tracked
component. Writes MUST use exclusive user-owned locking, legal transitions,
atomic queue replacement, and append-only prepared/committed attempt evidence.
Stale locks and ambiguous prepared attempts MUST require explicit recovery.
Recovery MUST bind a unique attempt, pre-state, intended post-state, approved
policy, and write scope; it MUST NOT infer rollback, completion, or absence of
effects from a missing process.

### REQ-PROJECT-LAYOUT

Kvist's target repository layout MUST place the root engine artifacts, Cargo
workspace manifest and lockfile, engine source, and root integration tests
inside one `engine/` component directory. `agent-runtime` and
`sandbox-runner` MUST be complete child components. Project vision,
architecture, root contract, decisions, licensing, and project documentation
MUST remain outside the implementation component. The migration MUST NOT retain
aliases for the retired paths.

### REQ-EXECUTION-BOUNDARY

Effectful task execution MUST require a complete explicit sandbox policy and an
independently installed runner outside the selected worktree. Kvist MUST
validate the exact runner, Bubblewrap backend, policy, typed path grants,
network capability, environment, command, toolchain, time, output, process,
file, and scratch bounds before invocation and MUST NOT fall back to
unconstrained host execution.

The unreleased `kvist-sandbox-probe-v1` and
`kvist-sandbox-request-v1` contracts MUST be redefined in place. The accepted
version-one request MUST distinguish authoring, dependency acquisition, and
verification, use an explicit working directory and typed argument vector, and
represent every read-only, read-write, context, dependency-cache, toolchain,
scratch, and isolated-lockfile-workspace mount. Legacy version-one request
semantics MUST be rejected.

Authoring agents MUST NOT receive write access to intent, queues,
implementation records, approval material, canonical evidence, Git metadata,
ambient home state, or child and peer implementations. Verification MAY
receive read-only provider source and workspace metadata required by the build
without adding those paths to authoring context or write authority.

Read access is a separate, bounded, logged capability and is not governed by the
write restrictions above; the exact bounds and exclusions are defined by
`REQ-AGENT-READ-SCOPE`.

### REQ-AGENT-READ-SCOPE

An authoring agent MAY read the whole current project and the source code of
approved dependencies, so it can navigate, understand, and correctly modify the
component it is working on. Reads do not change state; they MUST be bounded by
per-file size, total bytes per turn, and directory recursion depth, and MUST be
recorded in the durable trajectory so the run remains inspectable. Reads MUST
never surface user-owned secrets outside the project: approval material, the
user-owned authentication secret, and ambient home state remain outside the read
scope.

### REQ-COMPONENT-WRITING-SCOPE

An authoring agent MAY write any part of its own component directory, including
component-root files such as `Cargo.toml`, `deny.toml`, and build scripts, but
MUST NEVER write Kvist-controlled intent and record documents
(`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`), the
component's `.kvist` state and evidence, its `.git`, or any sub-component
directory. A sub-component is any directory that is itself a component by the
discovery rules; it is identified and excluded from the writable scope.

`write_file` MAY fully overwrite an existing file, because every authored file is
version controlled and its commit is gated on full human review, so any change
can be reverted. `edit_file` remains available for targeted, identity-bound
replacements. The broker's writable scope is the component directory minus the
exclusions above; everything outside it is refused.

### REQ-DEPENDENCY-ACQUISITION

Dependency acquisition MUST be a distinct approved phase. Cargo MUST be able to
resolve and download new or changed dependencies from exact configured
supported sources into a real bounded attempt-local writable `CARGO_HOME`, an
isolated writable lockfile workspace, and separate target scratch without
receiving the user's Cargo home. Acquisition is exactly non-compiling
`cargo fetch`, not `cargo fetch --locked`; a changed lockfile is an expected
result and its exact before/after content identities are bound to promotion.
Initial source support MUST cover canonical crates.io sparse-index and crate
download origins. Additional registries MUST declare exact index and download
origins. Git dependencies MUST initially require an exact approved repository
URL and immutable revision.

The acquisition phase MUST NOT execute dependency build scripts. Successful
cache promotion MUST validate source policy, checksums, lockfile changes,
bounds, paths, links, and concurrent preconditions. It MUST construct a new
immutable cache generation beneath a trusted provider-owned parent rather than
mutating an arbitrary project-cache pathname. Verification MUST run exactly
`cargo test --locked`, with network denied and offline true, using the approved
generation as a read-only `CARGO_HOME` and separate writable target scratch.

Current implementation status: the engine implements typed, bounded planning
with distinct host and sandbox paths, full source-config parity, and
attempt-local lockfile identities. The independently installed runner validates
the exact Cargo phase shapes, independently derives source identities, and
provides an immutable-generation primitive and transport-policy value types.
These are primitives only: OS mount, process, DNS/address-pinning, redirect,
and network enforcement, live `task run` wiring, and final project-cache
generation selection remain deferred. An otherwise valid request fails closed.

### REQ-DEPENDENCY-REQUEST

An agent MAY request a new or changed dependency mid-task, because the required
dependency is often only known while implementing. A request carries the crate
identity and an exact, approved source. When the request matches the configured
supported package sources and all bounds, the engine MUST fetch it in the distinct
dependency-acquisition phase, bind the lockfile before/after identities, promote
an immutable generation, and allow the agent to continue using it, all without
human intervention. The request and its outcome MUST be recorded durably as
evidence.

When a request needs a source, registry, or git revision outside the approved
policy, it MUST NOT be applied automatically. It is instead surfaced to the human
as a blocking decision and placed the component in an awaiting-decision state
until the human approves it, rejects it, or narrows the policy.

Current implementation status (phase 4): the broker accepts the
`request_dependency` tool, evaluates each origin against the dependency policy,
records an in-policy request as durable evidence so the agent continues without
interruption, and surfaces an out-of-policy request as a decision that places the
component in an awaiting-decision state. The distinct acquisition phase that
auto-fetches an in-policy revision and promotes an immutable generation remains a
documented follow-up, so an in-policy request is recorded for acquisition rather
than fetched inline.

### REQ-RUST-TOOLCHAIN

The Rust toolchain used by offline Cargo verification MUST be a pinned,
host-provisioned artifact with durable, versioned state. When a project root
contains `rust-toolchain.toml` (preferred) or `rust-toolchain`, its channel
MUST be the authoritative toolchain selection; otherwise the rustup default
MUST be used and recorded as such. Channel parsing MUST fail closed on
malformed, oversized, or link-like pin files, and channel syntax MUST be
validated to the rustup-supported forms (exact versions, `stable`, `beta`,
`nightly`, and dated variants).

Toolchain provisioning MUST be a distinct host-authorized step performed
outside the sandbox (ADR-0012): it resolves the pinned channel, installs it
via `rustup` when absent (the supported upgrade and downgrade path), validates
the resolved toolchain layout, and records a versioned manifest under
`.kvist/` carrying the channel, toolchain root, exact cargo path, and cargo
content digest. Builds and verification MUST NEVER install, upgrade, or modify
a toolchain; they MUST only consume an already-provisioned one. Verification
MUST resolve the toolchain channel-explicitly (independent of the process
working directory and ambient rustup overrides) and MUST fail closed with an
actionable message when the pinned toolchain is absent or the recorded manifest
no longer matches the on-disk toolchain.

Current implementation status: not implemented. The verification path resolves
the ambient rustup default without a pin, a provisioning step, or a recorded
manifest, and the authoring phase receives no usable Rust toolchain (see
`REQ-LANGUAGE-SUPPORT`).

### REQ-LANGUAGE-SUPPORT

Language support in the build flow MUST be declared, evidenced, and bounded
per language. A language is supported for a phase only when an end-to-end
integration test vendors (or otherwise provisions) a small real project on the
host and runs its real build/test offline inside the Bubblewrap sandbox,
asserting a successful run; tests MUST self-skip on hosts without the live
sandbox. Supported languages and their limitations MUST be documented in the
root README and this component's contract.

The current support state is: Rust first-class for verification (exact
vendored registry, pinned toolchain pending `REQ-RUST-TOOLCHAIN`); C/C++, Go,
Python, and JavaScript through the generic approved-test-command path against
host system toolchains (network denied, no vendored mounts); JVM and Ruby for
zero-dependency projects only. The intended order is the easy languages first
(Go, JavaScript), then the important ones (Python, C/C++), with the existing
non-Rust vendoring strategies (lock-file match plus presence) wired into
`kvist vendor` provisioning and the verification mount plan before any of them
is claimed as vendored-supported; per-package verification MUST land before
that claim. Container-based builds, toolchains without an offline/locked mode,
toolchains requiring ambient home/global mutable state, and GPU/accelerator
toolchains MUST be documented as explicitly unsupported.

Current implementation status: the generic test-command path and the Rust
closed topology are implemented and evidenced (Rust by
`offline_cargo_verification_e2e`). The non-Rust vendoring strategies exist at
the enforcement layer only; `kvist vendor` is Rust-only and verification passes
no vendored mounts for non-Rust languages. The authoring phase exposes no
usable Rust toolchain (the shared runner contract permits the Cargo toolchain
and the vendored-registry/config/runtime purposes only in verification
phases); extending that contract is a tracked follow-up.

### REQ-SUPERVISED-EXECUTION

The initial production-runner tier MUST be supervised. It MUST require an
explicit task ID, disable automatic retry, record a unique reviewable attempt,
and require a separate human finalization action before completion. Process
success or verification success alone MUST NOT complete the task. Timeout,
output breach, runner failure, and ambiguous interruption MUST fence or block
the attempt with bounded evidence.

Unattended _work_ — a task run proceeding across many turns without per-turn
human intervention — is the intended operating mode and does not require the
private bounded workspaces above; only final completion and VCS commit remain
human-gated. A mid-run decision that warrants human input does not complete the
task: it places the component in an awaiting-decision state for further
implementation until resolved, as defined by `REQ-DECISION-SURFACING`.

### REQ-MULTI-TURN-EXECUTION

A supervised task run MAY execute the agent across multiple model turns on the
host until the agent reports completion, a decision requires human input, a fatal
failure occurs, or the shared wall-clock budget is exhausted. The user's intent
is at the task level or higher: scheduling MUST NOT depend on the number of
intermediate turns, and normal work MUST proceed without per-turn human
intervention.

The shared wall-clock budget MUST equal the configured profile timeout and MUST
cover the liveness probe, every turn attempt, every retry backoff, every read,
and every brokered effect. The per-attempt transport deadline remains the
remaining budget. Only transient gateway availability failures are retried a
bounded number of times; response-level failures and surfaced decisions end the
loop deterministically.

Each turn returns untrusted intents that the engine classifies into exactly one
of: read (bounded, state-changing, logged), write (brokered and applied in the
effect sandbox), dependency acquisition request (evaluated against policy), or
decision proposal (surfaced to the human). Read and write results are fed back
into the model's continuing context; only decisions and fatal failures stop the
loop. Completion is the agent's explicit completion signal together with a clean
verification result; a single turn's success is not completion.

### REQ-ACCEPTED-VCS-COMMIT

Every human acceptance operation MUST produce a canonical acceptance set
binding exact accepted paths, creations and deletions, pre- and post-digests,
queue and evidence changes, review or attempt identity, expected VCS head, and
a bounded commit message. Component, project, and supervised-task acceptance
MUST offer an explicit option to create a local commit containing exactly that
set. Commit automation MUST NOT implicitly stage all files, push, stash, reset,
clean, amend, or include unrelated user changes.

The Git backend MUST preserve the user's real index, permit unrelated dirty
state only when it does not overlap the acceptance set, reject concurrent head
or accepted-path changes, construct the commit through an isolated index,
verify the exact resulting tree, and compare-and-swap the intended branch.
Repository hooks MUST be disabled by default and may run only through a later
explicit approved execution policy. Required signing MUST fail rather than
silently creating an unsigned commit.

Acceptance MUST remain durable if commit creation fails. Kvist MUST retain a
versioned recovery journal and allow the exact pending set to be committed
later without repeating acceptance. One commit per acceptance MUST be the
default. Jujutsu and other write-capable VCS backends MUST fail as unsupported
until independently implemented and promoted.

### REQ-AGENT-INTEGRATION

Kvist MAY invoke configured external agents for explicit prompt or task
operations. It MUST resolve commands without a shell, treat outputs as
untrusted and bounded, retain Kvist role and policy authority, and distinguish
acknowledged host execution from sandboxed task execution. Explicit prompts
MUST allow a configured model to be selected within the chosen role and MAY
apply a typed reasoning effort only when the selected model command declares
support. Plain output MUST contain provider content without a synthetic
completion message; JSON output MUST be one valid object with a `content`
field. Captured bytes that are not valid UTF-8 MUST be represented with U+FFFD
replacement characters.

Interactive agent setup MUST automatically qualify a newly generated provider
command with the fixed minimal prompt `Reply with exactly: OK`. Invoking setup
MUST first present bounded provider-advertised model choices when the selected
runtime provider exposes them, with manual model entry available only through
an explicit final custom choice. Invoking setup MUST count as acknowledgement
for those bounded discovery commands and the qualification command only and MUST NOT
authorize later host prompt execution. Failed qualification MUST prevent
configuration persistence unless the user supplied `--force`; that override
MAY persist the failed profile only after displaying an explicit warning and
MUST NOT override cancellation.
In JSON mode, setup prompts and status MUST be written to standard error,
qualification output MUST NOT be forwarded, and standard output MUST contain
exactly one valid result object.

An explicit interactive agent check MUST live-verify each model profile
configured in the selected scope by executing that profile's exact configured
command on the host with the fixed minimal prompt `Reply with exactly: OK`.
The check MUST NOT execute any test command without an explicit interactive
acknowledgement, and that acknowledgement MUST itself be a cancellation
point. For each failing profile, the check MUST offer profile removal (using
the standard profile removal semantics) or temporary ignore (which leaves the
configuration unchanged), and it MUST remain cancellable at every prompt.
`agent role list` MUST present only the predefined roles and their assigned
model profiles and MUST NOT present model profile names as roles.

Initial sandboxed task authoring MAY use local agents without model-network or
credential grants. Future remote model operation MUST keep model transport and
credential references in a host-owned broker outside the effect sandbox and
MUST submit only typed authorized tool requests to the runner.

### REQ-INTERACTIVE-SHELL

The `shell` command MUST provide an interactive workspace shell on an
interactive terminal and MUST fail with an actionable diagnostic when standard
input is not a terminal. It MUST parse lines against the same command surface
as the CLI and MUST derive static completion from that surface so completions
cannot drift from parseable commands. Dynamic completion MUST cover component
paths, task IDs, attempt IDs, model profile names, and the active VCS branch,
MUST refresh after every executed command, and MUST degrade per source without
aborting the session. `task run` without a task ID MUST NOT auto-execute a
task: it MUST suggest the first ready task of the selected component and run it
only after an explicit interactive confirmation (a bare ENTER accepts; a
refusal MUST change no state). When no task is ready, or when standard input is
not an interactive terminal, it MUST fail with an actionable diagnostic and
MUST change no durable state.

The shell MUST persist an append-only JSONL session journal of final command
inputs and result summaries at `.kvist/session.log` and a line-based editor
history at `.kvist/history`; both are local uninspected state and MUST NOT be
treated as compliance evidence. Command failures, prompt-editor cancellations,
and transient terminal read failures MUST NOT terminate the session; repeated
terminal read failures MUST exit with an actionable diagnostic. SIGINT during
a running command MUST request cancellation, terminate the supervised process
group, and return to the prompt without silently discarding durable task
state. Streaming task execution MUST relay sandbox output to the terminal
while it is produced and MUST retain the full bounded log as evidence. The
status presentation MUST distinguish live task locks from stale ones and MUST
NOT present stale locks as active. Destructive operations MUST require an
explicit in-shell confirmation before executing. Shell presentation MUST
degrade to plain text when `NO_COLOR` is set (any value), `CLICOLOR=0`,
`TERM=dumb`, or standard output is not a terminal, and `CLICOLOR_FORCE` (any
value but `0`) MUST force styling even when standard output is not a
terminal. Styled output MUST keep table columns aligned on visible width,
and titled boxes MUST fit the probed terminal width without truncating
content.

### REQ-COMPLIANCE

Implementation work MUST NOT certify itself. A clean-slate documenter derives
`IMPL.md` without intended requirements, contract, or design. A separate
source-blind reviewer compares that record with intended artifacts and retains
every discrepancy for human arbitration.

### REQ-ADVISORY-DOCUMENT-REVIEW

For the target component-acceptance workflow, Kvist MUST authorize acceptance
through one of three explicit paths:

1. when project review is required, a current review receipt covering the exact
   digests of
   `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, and a canonical projection of
   task definitions from `TODOS.yaml`, plus explicit human acknowledgement; or
2. an explicit exception bound to the same exact digests; or
3. a visible, deliberate project `[review] required = false` opt-out.

Review MUST default to required when the `[review]` section or `required` field
is absent, and generated project templates MUST emit `required = true`
explicitly. The task projection MUST contain only `id`, `title`, `description`,
`context`, `purpose`, `expected_outcome`, `kind`, `depends_on`, and
`requirements`; it MUST exclude all `component` metadata plus task `status`,
`timestamps`, `blocked_reason`, and `recovery_state`. Generated intent drafts
MUST NOT be exempt. The review is advisory: no finding, score, or severity
threshold may require a change, block acceptance, or determine compliance. A
per-bundle exception MUST record actor, timestamp, reason, and exact digests. If
project review is required but no review agent is configured, Kvist MUST
require an explicit exception and MUST NOT infer a pass.

`component accept` MUST remain deterministic and local and MUST NOT launch an
agent or make a network call. A separate planned review operation MUST use the
existing shell-free, bounded, explicitly approved or acknowledged agent
execution path. Review context MUST be limited to the local intent set and
canonical task projection, `ROOT_CONTRACT.md`, and the immediate parent
`CONTRACT.md`; peer and parent internals MUST be excluded.

Kvist MUST mint receipts from bounded execution evidence rather than accepting
model-generated receipts. Versioned receipts and redacted reports MUST be
stored under component-local `.kvist/reviews/` and MUST bind target digests,
scope, reviewer and authoring context identity when known,
provider/profile/model/tool identity, Kvist version, timestamp, bounded report
digest and path, and acknowledgement or exception. Raw transcripts and secrets
MUST NOT be retained. These files MUST be VCS-trackable but MUST NOT join the
five-artifact set, trigger component candidacy or staleness, enter task
context, or count as compliance evidence.

Project-level review of `VISION.md`, `ARCHITECTURE.md`, `ROOT_CONTRACT.md`,
ADRs, and referenced native schemas MUST be addressed by a later project-level
acceptance surface rather than claimed as current component enforcement. The
gate MUST NOT be generalized to arbitrary Markdown files.

### REQ-OBSERVED-INTENT-PROPOSAL

Kvist MUST provide a planned `propose intent` or `derive draft` workflow that
reads an independently generated `IMPL.md` and writes no-clobber draft
`REQUIREMENTS.md` and `DESIGN.md` only. It MUST mark uncertainty, MUST NOT
claim to recover stakeholder intent, and MUST NOT generate a normative
`CONTRACT.md`.

Generated drafts MUST remain subject to human review, advisory AI review or an
explicit exception, acknowledgement, and normal acceptance. A separate
advisory comparison MAY report differences between `IMPL.md` and existing
intent without modifying either. That comparison MUST NOT use compliance
verdict vocabulary and MUST NOT count as compliance evidence.

The existing `reverse-discover` source-based onboarding pipeline remains
distinct. Any contract it generates is a non-normative draft.

### REQ-DECISION-SURFACING

An agent MUST distinguish issues worthy of human intervention from those that are
not. A decision is worthy of intervention only when it substantially alters the
implementation, is not already covered by the component's `REQUIREMENTS.md`,
`CONTRACT.md`, `DESIGN.md`, or `TODOS.yaml`, and reasonably should be covered
there. Such a decision MUST stop the run and place the component in an
awaiting-decision state for further implementation until the human decides and
accepts or rejects the agent's proposal.

Issues that are not worthy of intervention — approach, style, minor refactors, or
decisions already covered by the existing intent — MUST NOT block the run. They
are recorded and left for the post-hoc advisory comparison of the independently
generated `IMPL.md` with the existing intent.

The agent MUST understand the purpose and policy of the Kvist-controlled intent
documents so it can judge coverage correctly. A proposal for a worthy decision
MUST be a no-clobber draft describing the decision and the proposed change, and
MUST be subject to human review and the normal advisory-review-or-exception
acceptance gate. An awaiting-decision component MUST NOT accept new
implementation work until the decision is resolved.

Accepting a proposal MUST propagate the decision to the durable intent before
work resumes. `TODOS.yaml` MUST gain every task required to implement the
accepted change, and `IMPL.md` MUST become stale because the implementation no
longer matches the intent. The component stays in the awaiting-decision state
until the intent is updated, an updated advisory review is performed, and the
human accepts the updated intent; only then is `IMPL.md` rederived from the code
and the component becomes valid again. Accepting a proposal MUST NOT skip the
review gate, and `IMPL.md` revision is a staleness cause so changing it marks the
component stale.

Current implementation status (phase 4): the `AwaitingDecision` task state exists
and the broker accepts the `propose_decision` tool; a surfaced decision ends the
run and the run harness transitions the task to awaiting-decision (never running
verification or jumping to completed), recording the proposal as a redacted patch
under the component state directory. Accepting a proposal still must propagate the
decision to `TODOS.yaml`, invalidate `IMPL.md`, and require an updated advisory
review before the component becomes valid again; the harness pauses the task and
defers that finalization to the human. The existing `propose intent`/`derive
draft` path provides the no-clobber draft mechanism this extends.

### REQ-CONTRACT-VERIFICATION

Kvist MUST plan stable contract-clause locators, initially using existing
heading anchors. Explicit clause IDs MAY be introduced only through an
explicit format/version decision. Tests MUST be traceable to contract clauses,
and approved test execution evidence MUST feed a contract-clause traceability
report that identifies uncovered and failed clauses.

Tests are evidence, not proof. This report MUST NOT be described as code
coverage and MUST NOT replace compliance comparison of `CONTRACT.md`,
`IMPL.md`, and test evidence.

### REQ-CONVERSION-IMPORT

Conversion, reverse discovery, and repository import MUST preserve existing
implementation files, validate generated or imported artifacts before
activation, refuse ambiguous existing metadata, and make generated intent
explicitly draft rather than inferred truth.

## Quality requirements and constraints

- Rust stable edition 2024 is required; unsafe Rust is forbidden.
- Executable support is Linux-only until independently tested platform
  boundaries exist.
- Core inspection and lifecycle operations perform no hidden network or model
  invocation.
- Advisory document review, observed-intent proposal, and contract-clause
  traceability are target requirements, not claims about the current CLI.
- Multi-turn agent execution, bounded whole-project reading, decision-driven
  awaiting-decision states, and the `propose_decision`/`request_dependency`
  tools (request, policy evaluation, and run-harness pause) are implemented and
  claimed by the current CLI. Whole-component-minus-exclusions writing scope
  (write at the component root for `Cargo.toml`/`deny.toml`) and agent-driven
  dependency requests that auto-fetch an in-policy revision remain target
  requirements, not claims about the current CLI.
- Configuration is limited to 64 KiB. Component Markdown and YAML artifacts
  read by the engine are limited to 1 MiB.
- Traversal depth, directory count, component count, entries per directory,
  path length, subprocess duration, and output are explicitly bounded.
- Writes use regular non-link paths and synchronized no-clobber or atomic
  replacement appropriate to the operation.
- Output, ordering, hashing, and canonical queue serialization are
  deterministic.
- Inputs and imported content are untrusted and invalid semantics fail
  explicitly rather than being ignored or repaired.
- Package-source origins, dependency files, archives, manifests, lockfiles,
  build scripts, and caches are untrusted and require explicit bounds and
  provenance.
- VCS paths, refs, index state, commit messages, signing configuration, hooks,
  and concurrent repository changes are untrusted execution inputs.
- The dependency graph remains small and justified for a local single-binary
  CLI.

## Acceptance and traceability

Each implemented `REQ-*` item requires unit or integration evidence appropriate
to its boundary. CLI and filesystem workflows require integration tests; pure
schema and transition behavior require unit tests; Linux sandbox and process
behavior require native boundary tests. Tests provide evidence rather than
proof. Security audit and independent compliance review are mandatory terminal
tasks for each deliverable chain.

The system decomposition and authority direction are defined in
[`../ARCHITECTURE.md`](../ARCHITECTURE.md). Consumer-visible behavior is
defined in [`CONTRACT.md`](CONTRACT.md), private realization in
[`DESIGN.md`](DESIGN.md), and standards alignment in
[`../docs/standards.md`](../docs/standards.md).
