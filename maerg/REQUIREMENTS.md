<!-- kvist-requirements-version: 1 -->

# Kvist Maerg Requirements

## Shared registry configuration acceptance (ADR 0015)

Setup MUST persist shared profile references rather than copied commands.
Shared resolution MUST pin typed provider/endpoint/model/executable data and
bind it into source evidence. Role policy and protected-task authority MUST
remain maerg-owned, including refusal of external coding agents.

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Purpose and scope

The root component provides the local headless Kvist maerg and CLI. It owns
project initialization, component-document validation, bounded discovery,
project status, durable task queues, task lifecycle, Kvist-specific role and
policy selection, sandbox integration, context selection, import/conversion,
and durable execution evidence.

The reusable provider runtime belongs to the child `sav` component.
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

### REQ-SHARED-RUNTIME

Protected task startup MUST NOT require serving-capacity discovery to succeed.
An explicit native context limit MUST bypass discovery. Otherwise a single
optional metadata probe MUST take at most one second and one tenth of the
remaining task budget. Missing/unsupported metadata and transient availability
failures MUST produce a visible, recorded warning and permit inference using a
bounded provisional working budget, not an asserted model capacity. Cancellation,
malformed metadata, invalid configuration and recording failures MUST still fail.
The provisional budget MUST fit the complete initial request plus its output
reserve and history headroom, without dropping approved intent or increasing
approved wall/token/effect authority. Provider context rejection remains explicit.
Task context MUST include only the selected task's authored queue definition,
not unrelated tasks or queue lifecycle evidence; the complete queue MUST still
be validated and bounded before projection. Startup diagnostics MUST distinguish
capacity discovery from inference and report estimated request size.
This human-requested repair has an explicit advisory exception while receipts
are unavailable; independent observation and comparison remain separate.

Delivered human guidance MUST renew model-generation attempts, without
resetting approved task wall/token budgets. Both shared-native and explicit
endpoint transports MUST clip individual calls to their independent deadline
ceiling rather than reject an otherwise valid multi-day task budget.

Protected authoring MUST NOT stop at an implicit generation-count ceiling.
The approved positive role timeout MUST bound the whole task, MAY span up to
30 days, and MUST remain unchanged by guidance. Individual sandbox effects
MUST retain the runner's 24-hour ceiling, clipped to remaining task time.
Default role timeouts remain unchanged; extending them requires the existing
policy approval flow. Tests MUST exceed the former 64-generation ceiling.
This human-requested repair has an explicit advisory exception while receipts
are unavailable.

Maerg MUST consume shared provider/profile resolution and native runtime
mechanisms without yielding task policy, context, authorization, acquisition,
verification, promotion or evidence authority. Explicit run selections MUST be
resolved once and recorded. External-agent profiles that cannot meet the
protected broker contract MUST fail before effects without host fallback.

Protected attempts MUST emit bounded versioned diagnostic events during
execution, not reconstruct their only trajectory after completion. Required
recording failure MUST prevent later effects. Read-only observers MUST not own
execution. Optional controls MUST be explicitly authorized, record guidance,
preserve approved context/grants and accounting, and never promise rollback or
resume of unknown effects. Diagnostics remain distinct from canonical evidence.

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

Required-section content MUST include subordinate Markdown headings and their
bodies. Fenced examples MUST NOT declare required sections. A recovery placeholder
MUST be structurally valid but durably block acceptance and task execution until
resolved. Inspection MUST distinguish this condition from malformed documents.
Immediate-parent contract and global project-intent blockers MUST propagate,
without introducing implicit peer context.

### REQ-DISCOVERY-STATUS

Kvist MUST discover the configured recursive component hierarchy
deterministically within configured hard bounds. Status MUST validate all
adjacent artifacts and report missing, invalid, unsupported, stale, blocked, or
current state without modifying project files. Every status report format
MUST explain a non-current project state: each non-valid root artifact with
its exact reason, any project-root diagnostic and discovery error, and a
concrete next action that names the unblocking command (`kvist init` for
uninitialized projects, `kvist repair` when the only defect is an unsorted or
duplicated TODO-queue set list or missing timestamp metadata).

The overview report MUST keep its layout aligned: the per-document
`expected`/`observed` revision lines under a changed document MUST carry the
same left border as the surrounding overview lines, aligned under the changed
document's name. A blocked task's `blocked_reason` first line MUST be shown
in full — the queue's own text bound and soft wrapping bound the display,
never a renderer-side truncation with ellipsis.

### REQ-PROJECT-REPAIR

Kvist MUST provide bounded recovery without requiring manual artifact edits.
Default repair MUST plan comprehensive deterministic salvage, first trying
lossless queue normalization, missing timestamp metadata and dependency
declaration ordering. Valid queues
MUST remain exact.
Every rewrite MUST retain a private, durable original and report its location.

Salvage MUST preserve independently valid tasks, explain every
reconciliation and removal, and validate the resulting queue with the ordinary
strict validator. Missing dependencies, cycles, conflicting identities and
missing lifecycle predecessors MUST NOT be repaired by silently weakening
dependencies or manufacturing verification. Ambiguous timestamp reconciliation,
field removal and task removal require approval of the exact proposed changes.
Unsupported schemas MUST NOT be silently migrated.

Absent historical timestamp metadata MUST be repaired automatically with a
backup, without approval, task removal, status/blocker changes, weakened edges,
cleared finalization references or changed component acceptance metadata.
Existing valid dates MUST remain exact. Missing creation/update dates MAY use
explicitly synthetic chronological lower bounds; missing completion MUST remain
null, including for a previously completed task. Such tasks MUST carry durable
machine-readable unknown-history provenance, not fabricated completion evidence.
An otherwise-valid completed task with this provenance and unknown completion
MUST pass native validation. Known dates MUST still satisfy ordinary validation.
Malformed supplied history alone MUST NOT discard a task or its dependency
closure; approval MAY convert it to an explicitly blocked recovery record
retaining identity, authored fields and edges. Synthetic recovery timestamps
MUST NOT imply observed creation, update, completion or verification.
Subsequent explicit transitions MAY resume work but MUST preserve that provenance.
Lossless metadata repair MUST retain ordinary lock and attempt-evidence guards;
unresolved execution evidence MUST NOT become implicitly accepted.

Every root/component artifact MUST have a template-reset fallback, including
malformed syntax, invalid encoding, missing files and wrong filesystem types.
Before reset, Markdown salvage SHOULD preserve prose while repairing
current-format markers/headings and required-section structure. Missing content
MUST be labeled a recovery draft with a durable blocker, not invented stakeholder
intent or evidence. Fences alone MUST NOT force a reset when section boundaries
are unambiguous. Native reset drafts MUST also retain a blocker.
An explicit clean slate MUST offer fresh root/component drafts and archive
project-owned workflow state, preserving source and VCS data. Reset drafts and
salvaged queues MUST NOT constitute acceptance, certification or proof that
prior execution had no effects. Live operations MUST block mutation. Ordinary
repair MUST preserve fenced or unresolved attempt evidence; only an explicitly
approved clean slate may archive it, with an effects-remain-unknown warning.

Destructive recovery MUST first preview affected paths, old/new values or task
removals and reasons, then require a content-bound approval token. Changes to
the inspected inputs invalidate approval. Dry runs and unapproved proposals
MUST NOT modify managed artifacts or create backups. The sole staging exception
is an explicitly requested new AI proposal file; dry runs MUST NOT create it.
Backup failure MUST prevent mutation; partial application
MUST retain originals and a manifest explaining progress. Recovery MUST operate
with alternative private backup storage when the default is obstructed.
Intact quarantine MAY require same-filesystem storage, with an explicit
alternative-directory hint rather than deletion of an unbacked object.
Recovery MUST operate
without valid project configuration, inspect recursively within explicit bounds,
reject traversal and link-like parents, and report discovery/I/O/bounds failures
rather than claiming success. Disk and permission failures remain actionable
environmental blockers, not permission to discard unbacked data. The command
MUST exit nonzero while structural artifact problems or unapplied proposals
remain. Tests MUST cover the reported completion timestamp, ordering, salvage,
approval binding, resets, backups, invalid configuration and filesystem gates.
This human-requested expansion has an explicit per-bundle advisory exception
while review receipts are unavailable; independent observation/comparison remain
separate from implementation.

The shell MUST discover repair options at an empty argument and at `-` or `--`,
without requiring a prefix. A completed `-- ` separator and pending option values
MUST retain their normal positional/value semantics.

An opt-in native local-model fallback MUST attempt bounded, tool-free proposed
replacements when deterministic salvage cannot preserve data. It MUST work
independently of valid project configuration, never infer original timestamps or
certify authored intent, never edit managed files directly, and never require
serving-capacity discovery. Strict native validation MUST gate staged candidates.
The proposal MUST freeze model output and original/context fingerprints; applying
it MUST not invoke the model again. Unknown paths, unsupported schemas, active
execution, malformed output, stale inputs, exceeded bounds and unavailable models
MUST fail explicitly without managed writes. AI-authored Markdown requires a
blocker; AI-recovered queues require stale authority and blocked, unknown-history
tasks. Approval MUST bind the exact frozen proposal as well as ordinary inputs.

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

Suggested-task confirmation MUST finish before execution progress starts.
Progress MUST NOT overwrite terminal input, retry notices, or streamed output.
Result presentation MUST retain complete diagnostics and actionable commands.

Protected inference requests MUST automatically
retry transient transport failures and HTTP 429/500/502/503/504/507 responses
within the approved wall/token budgets and selected attempt limit. Backoff
MUST grow exponentially with bounded jitter and remain cancellable. Each retry
MUST report its phase, model, attempt, cause and delay on stderr and in the live
journal. Exhaustion MUST identify the stopping bound and selected endpoint.
Transient failures after provisional streamed progress MUST be retried without
executing incomplete proposals; discarded progress MUST be explicitly reported
on stderr and in the journal. Cancellation, invalid input and required-recording
failures MUST NOT be retried. Effects and verification commands MUST
NOT be replayed automatically when their effects may be unknown.

Ordinary execution and verification failures MUST return failure, persist a
blocked task with the underlying cause, and identify the attempt and inspection
and retry commands. Evidence-persistence failures MUST preserve the unresolved
attempt and explicitly report that state instead of hiding the original error.
Attempts started within the same clock second MUST retain distinct logs and
diagnostic journals without overwriting prior evidence.
Selection MUST NOT silently skip an unresolved attempt; batch execution MUST
stop at failures, decisions and pending human finalization without claiming
completion. Existing ambiguous attempts MUST NOT be retrospectively certified.
This human-requested repair has an explicit advisory exception while receipts
are unavailable; independent review remains separate from acceptance.

An explicit human `task finalize ... block --reason ...` MUST remain available
when otherwise valid, VCS-tracked component intent is stale or another task is
blocked. It MUST bind the latest exact task/attempt, retain prior evidence and
effects, and record rejection without certifying completion, no execution or
rollback. Rejection MUST NOT require unchanged accepted-result digests or
current execution approval. Malformed artifacts, active locks and explicit
recovery fences MUST remain protected. A failed disposition write MUST NOT
silently release unresolved execution; repeating that same block decision
MUST be able to finish its interrupted recording.
Read-only task-log inspection MUST remain available while intent is stale.
After abandonment, acceptance MUST report remaining blocked tasks accurately
and identify acceptance-before-transition recovery order. This human-requested
repair has an explicit advisory exception while review receipts are unavailable.

Task selection MUST require a current, completely VCS-tracked component.
Ordinary transitions MUST require complete tracked artifacts and current intent;
blocked task state alone MUST NOT prevent an otherwise legal human transition.
Writes MUST use exclusive user-owned locking, legal transitions,
atomic queue replacement, and append-only prepared/committed attempt evidence.
Stale locks and ambiguous prepared attempts MUST require explicit recovery.
Recovery MUST bind a unique attempt, pre-state, intended post-state, approved
policy, and write scope; it MUST NOT infer rollback, completion, or absence of
effects from a missing process.

### REQ-TRANSITION-RECOVERY

Every documented task-state edge MUST work through the command boundary,
including entering and resuming awaiting-decision with its required reason.
Rejected ordinary transition requests MUST leave queue and evidence unchanged.
Verification MUST cover all 25 state pairs, reason and dependency gates, stale intent,
terminal states, and supported recovery sequences, not only the pure enum.
Guidance MUST name commands their own state gates permit.

Stale-lock removal and authenticated execution-did-not-start recovery MUST
remain available for otherwise valid tracked stale or blocked components.
Neither operation accepts intent, certifies completion, authorizes replay,
or bypasses live-owner, evidence, approval, scope, identity or digest checks.
Missing, invalid, unsupported or untracked artifacts remain protected.
Execution and result acceptance MUST retain their current-intent gates.
Result finalization MUST bind the latest exact task/attempt and MUST NOT
overwrite a conflicting human disposition or authenticated recovery decision.
Finalizing one task MUST NOT invalidate another unresolved attempt's recovery
pre-state; reconcile that attempt before any unrelated queue decision.
Acceptance MUST persist the queue decision before append-only final evidence;
repeating the same non-committing acceptance MUST finish interrupted recording
without duplicating evidence, mutating completed state or replaying effects.
This human-requested verification and repair has an explicit advisory exception
while review receipts are unavailable; independent observation remains separate.

### REQ-DOGFOOD-FIXTURES

Workflow and recovery test fixtures MUST use compact versioned artifact
templates rather than copying the repository's evolving authority documents
or implementation records. Their complete selected context, including local
artifacts, root contract and any parent contract, MUST remain within 16 KiB.
The fixture's 65,536-token context limit and 1,024-token response reserve MUST
remain unchanged; production context limits and lifecycle assertions MUST NOT
be relaxed to repair fixture failures.

Verification MUST establish fixture creation without repository authority
files and rerun affected tests after final intent, queue and evidence updates.
Repository-layout and real-runner tests MUST retain their actual repository
and enforcement checks. This test-only repair has an explicit advisory
exception while review receipts are unavailable.

### REQ-PROJECT-LAYOUT

Kvist's target repository layout MUST place the root maerg artifacts, Cargo
workspace manifest and lockfile, maerg source, and root integration tests
inside one `maerg/` component directory. `sav` and
`galla` MUST be complete child components. Project vision,
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

Current implementation status: the maerg implements typed, bounded planning
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
supported package sources and all bounds, the maerg MUST fetch it in the distinct
dependency-acquisition phase, bind the lockfile before/after identities, promote
an immutable generation, and allow the agent to continue using it, all without
human intervention. The request and its outcome MUST be recorded durably as
evidence.

When a request needs a source, registry, or git revision outside the approved
policy, it MUST NOT be applied automatically. It is instead surfaced to the human
as a blocking decision and placed the component in an awaiting-decision state
until the human approves it, rejects it, or narrows the policy.

Current delivery limitation, exposed by the independent ADR 0015 source review:
the broker evaluates requests, but exact request-to-lock identity binding and
mediated vendor repacking are unavailable. The former automatic host-provisioning
fallback is not an implementation of the protected requirement above. Protected
requests now fail explicitly before provisioning effects, even if unrelated
vendoring is ready. Explicit human `kvist vendor` remains separate. Automatic
fulfillment is an unmet target requiring arbitration, not an approved deferral
or a claim of compliance. Out-of-policy requests still surface a decision.

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

Current implementation status: implemented. Pin parsing, channel validation,
the host provisioning step (`kvist toolchain`), the durable manifest,
and channel-explicit, fail-closed enforcement on use are implemented and
evidenced by unit tests plus end-to-end tests, including a pinned
`nightly` toolchain built and tested offline in the sandbox. The authoring
path also consumes installed-only resources as described below.

### REQ-LANGUAGE-SUPPORT

Sandbox language tooling MUST use bin-only search paths, including
`/usr/local/bin`, and exclude the standard administrative directories before
and after canonicalization when locating host tools. All authoring and
verification requests consume Galla's filtered System view; no sbin access or
additional write authority is granted.

Language support in the build flow MUST be declared, evidenced, and bounded
per language. A language is supported for a phase only when an end-to-end
integration test vendors (or otherwise provisions) a small real project on the
host and runs its real build/test offline inside the Bubblewrap sandbox,
asserting a successful run; legacy language tests MAY self-skip on hosts without
the live sandbox. Supported languages and their limitations MUST be documented in the
component README and this component's contract.

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
`offline_cargo_verification_e2e`). `kvist vendor` dispatches per detected
language strategy, and every vendored non-Rust project verifies through the
shared offline language topology (ADR-0013): vendoring enforced, vendored
material mounted read-only with the lock-file digest identity, one writable
scratch at the fixed Cargo scratch destination, network denied, and the
language's canonical offline test command (or, for C/C++ only, the approved
`[test_policy]` command). Go, JavaScript, Python, and C/C++ are vendored-supported and evidenced end to
end by `language_offline_e2e` (`go_offline_verification_builds_and_tests_denied_network`, `javascript_offline_verification_builds_and_tests_denied_network`, `javascript_pnpm_offline_verification_builds_and_tests_denied_network`, `python_offline_verification_builds_and_tests_denied_network`, `c_offline_verification_builds_and_tests_denied_network`, `c_vcpkg_offline_verification_builds_and_tests_denied_network`, `python_uv_lock_offline_verification_builds_and_tests_denied_network`). JavaScript is vendored-supported for npm (`package-lock.json`), yarn (`yarn.lock`), and pnpm (`pnpm-lock.yaml`); the pnpm path vendors the content-addressable store. Both `requirements.lock.txt` (pip) and `uv.lock` (uv) locked Python projects vendoring and verify offline; a `uv.lock` project is provisioned by exporting its exact resolved graph with `uv export --locked` before the locked wheels are vendored and installed into the venv. C/C++ is vendored-supported for Conan (`conanfile`) and for vcpkg (`vcpkg.json`); the vcpkg path vendors the whole vendored vcpkg root. The
Rust authoring MUST expose the complete validated standard-layout installed
Rust inventory and its targets read-only through System/Toolchain grants.
For Rust projects, it MUST initialize sandbox-private selection settings from
generated templates, preserve the project pin or otherwise host default and
support invocation-local `rustup default` without host mutation, installation
or network access. PATH tools MUST agree on selection and Cargo MUST default
to offline/locked vendored resolution. Non-Rust authoring MUST retain its
approved environment and MUST NOT require Rust. Protected component artifacts
and peers MUST NOT gain write authority. Staged resources MUST remain owned
through execution and be included before mount-plan hashing.
An ordinary, non-ignored native test MUST verify installed inventory/default
parity, selection, offline builds and host non-mutation; missing prerequisites
MUST fail explicitly, not skip or install anything.
The requested repair has a per-bundle advisory exception while receipts are
unimplemented; independent observation and comparison are not acceptance.

For Rust authoring, declared workspace members and local path dependencies
inside the approved project MUST be available as read-only build resources,
with original workspace metadata and lockfiles at a consistent sandbox-native
topology. This is explicit human-approved build context, not implicit peer
prompt context. Provider intent/state, user homes and operational caches MUST
remain excluded. No peer write authority may be added. Out-of-project,
linked, unsupported or over-bound declarations MUST fail explicitly.
PATH Cargo MUST build through this view; formatting/compiler source paths for
the authored component MUST resolve back to its existing writable scope.
Non-ignored tests MUST build a real multi-crate workspace with inherited
package/dependency fields, verify immutable provider source/metadata and
unchanged host locks, and cover invalid dependency paths.

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
and every brokered effect. Each per-attempt transport deadline MUST be bounded
by the remaining budget and the independent transport/profile ceiling.
Only transient gateway availability failures are retried a
bounded number of times; response-level failures and surfaced decisions end the
loop deterministically.

Each turn returns untrusted intents that the maerg classifies into exactly one
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

### REQ-MARKDOWN-OUTPUT

Every Kvist command MUST report its result as a well-formed Markdown document
that is the canonical description of that result. The document MUST state the
observed state, document validity, task progress, the next ready task, and an
actionable next step for every non-current state, and MUST render blocked and
awaiting-decision reasons and stale-document revision details as structured
Markdown (headings, tables, lists, fenced code) rather than unstyled text. The
Markdown document is the single source of truth for what a command reports. The
stable `status --format text` and `--format json` report forms MUST be unchanged
so non-interactive consumers and scripts are unaffected, and the plain-text
degradation contract of `REQ-INTERACTIVE-SHELL` MUST still apply when the
terminal cannot host the TUI or styling is disabled. The Markdown is the
presentation of state, not a substitute for the durable artifacts or for
executable tests.

### REQ-INTERACTIVE-TUI

The interactive shell MUST be a full-screen terminal UI in the shape of the
`skott` layout: a status heading, a scrollable main transcript area, and a
command footer. The status heading MUST show the active component together with
its state and task progress, plus the VCS branch, sandbox backend, default
model, and live/stale lock counts. When no component is selected the heading
MUST show the whole project and the shell MUST operate on the project; when a
component is selected it MUST be auto-selected as the default for every command
that expects a component, so the operator is never asked for it and it is
pre-filled. The command footer MUST expose the same command surface as the CLI
with full tab-completion (static from the clap surface, dynamic from the
project state) and MUST provide keyboard-shortcut help rendered the same way
`skott` does it. The TUI MUST degrade to the plain-text line-oriented shell
when the terminal cannot host it.

### REQ-TRANSCRIPT-PERSISTENCE

The main transcript area MUST be a bounded on-screen window over a durable,
per-component, disk-persisted record of every interaction with Kvist (commands
and their rendered Markdown output). The record MUST be unlimited in size and
MUST split into successive files when one grows too large. The operator MUST be
able to scroll back through the session with a scrollbar. Switching component
MUST clear the transcript area and MUST restore that component's former
transcript when the operator returns to it. The transcript is local uninspected
state and MUST NOT be treated as compliance evidence.

### REQ-SETTINGS-MENU

The interactive shell MUST provide a settings menu (the `skott` overlay
pattern) that exposes the major choices: model, reasoning effort, session
history, component selection, theme, and other settings. Selecting a component
from the menu MUST be the same focus change as the `cd` builtin and MUST
pre-fill the component for subsequent commands. Theme, model, and effort
selections MUST persist the preference and apply immediately to the session.

### REQ-RUN-COMPONENT

The shell MUST provide a command that runs the whole selected component: it
MUST iterate over the component's tasks in dependency order, running each ready
task, and MUST stop when the component is complete, when a task becomes
blocked, or when a task reaches an awaiting-decision point. It MUST NOT
auto-resolve a decision or unblock a blocked task; it MUST pause and report the
exact state and the command that resolves it, so the operator can decide and
resume. The command MUST respect the same locking, policy-approval, and
evidence guarantees as `task run`, and MUST leave durable task state for
explicit finalize or recovery when it stops at a blocker or decision.

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

### REQ-HELP-GUIDANCE

Kvist MUST answer "what is this state, and what do I do next?" from the
command line and the interactive shell without external documentation. Status
output for every non-current component state MUST name the concrete cause —
for blocked components, each blocked task with its recorded `blocked_reason`
(truncated to a bounded first line) and each awaiting-decision task — and an
actionable next command, or, where the next step is a human decision, an
explicit pointer to the help topic that explains the state and its resolution.
Hints MUST be short, dense, and name real commands.

The maerg MUST provide `kvist help` rendering a short tour of the core
concepts (project, component, intent documents, tasks) and the component
lifecycle, plus named topics: the core concepts, the lifecycle with the exact
commands, and the durable task states with the legal transitions and the
command that achieves each. Topics MUST be selectable by name on the command
line with tab completion, and the shell's `help` builtin MUST accept the same
topics with completion. Every topic MUST end with a pointer to the deeper
documentation (`GUIDE.md`, `docs/command-set.md`, or a component's
`CONTRACT.md`).

An invalid task transition MUST report the legal transitions from the current
state together with a pointer to the task-states help topic. The `task run`
help MUST document the multi-task task specifications (exact task ID, item
prefix, `all`).

### REQ-SOFT-WRAP

Interactive shell output MUST fit the terminal width: long lines MUST be soft
wrapped at word boundaries before rendering, so that (1) boxed surfaces — the
welcome banner, the prompt/working/result stages, and bordered status reports —
keep their borders intact on every physical line, and (2) text blocks keep
their indentation, with continuation lines prefixed by the line's leading
whitespace. A `key: value` line (a word, a colon, and at least one space after
the leading whitespace) MUST wrap with its continuation lines aligned under the
start of the value, not the key. Wrapping MUST be a pure function of the text
and the probed terminal width, and ANSI styling MUST survive wrapping (a styled
span re-emits on every physical line it touches). Output that is piped, captured, or rendered
without a known width MUST be byte-identical to the unwrapped form so scripts
and stable report consumers are unaffected. Unbreakable words longer than the
available width MUST be hard-split at the width rather than dropped.

### REQ-SHELL-THEME

The interactive shell MUST render all presentation through a semantic color
palette of named roles (prompt, component, failure, indicator, border, dim,
six status hues, and the agent result surface) so no renderer hard-codes a
color, and MUST provide the built-in `dark` (default) and `light` themes.
The `theme` builtin MUST show a live in-terminal preview of every available
theme, and `theme set <NAME|PATH>` MUST switch the session theme immediately
for all subsequent rendering and persist the preference to `.kvist/theme`.
Session theme resolution MUST prefer, in order: the `KVIST_THEME`
environment variable, the project-local `.kvist/theme` preference, the user
preference at `~/.config/kvist/theme` (honoring `XDG_CONFIG_HOME`), an OSC 11
terminal-background probe (light when the probed surface is light), and MUST
fall back to `dark`; an unresolvable preference MUST warn and fall back
to detection, and a preference that names a spec file MUST load that file.
A user theme MUST be a TOML spec with a `name` (1-32 characters of
`[a-z0-9-]`), an optional `mode` (`dark` or `light`, default `dark`), and a
`[colors]` table mapping palette roles to a `#rgb`/`#rrggbb` hex color, a
named ANSI color, or a 0-255 index; a spec MUST NOT exceed 8 KiB, MUST
reject unknown roles, and an invalid, over-size, or unreadable spec MUST
fail with an actionable diagnostic without changing the session. The
plain-text degradation contract of `REQ-INTERACTIVE-SHELL` MUST apply to
every theme. In the `dark` theme, the agent result surface MUST use a true
black background for maximum contrast.

### REQ-SCROLL-PAGER

Output that scrolls past the pager threshold MUST be paged in a built-in
full-screen pager that renders a one-column scrollbar on the right whose
thumb is sized proportionally to the output (never shorter than two rows)
and whose position reflects the current scroll offset, so the reader always
sees how large the output is and where they are inside it. The pager MUST use
the full terminal height: every content line occupies its own terminal row,
drawn at the left edge of that row, with the gutter and scrollbar reserved on
the right. The pager MUST
support at least: quit (`q`/`Esc`), one-line scrolling (arrows, `j`/`k`),
page scrolling (`PageUp`/`PageDown`), top/bottom jumps (`g`/`G`), and
mouse-wheel scrolling, and MUST fall back to direct printing when it cannot
take the terminal so no output is lost. Piped, captured, and width-unknown
output MUST never be paged.

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

- Rust stable edition 2024 is required. Prefer safe Rust; necessary, minimally
  scoped unsafe exceptions must satisfy the root contract's justification,
  safety-invariant, targeted-verification, and independent-review rules.
  Retain current compile-time unsafe guards unless deliberately approving
  such an exception; this policy does not authorize their broad removal.
- Executable support is Linux-only until independently tested platform
  boundaries exist.
- Core inspection and lifecycle operations perform no hidden network or model
  invocation.
- Advisory document review, observed-intent proposal, and contract-clause
  traceability are target requirements, not claims about the current CLI.
- Multi-turn agent execution, bounded whole-project reading, decision-driven
  awaiting-decision states, and the `propose_decision`/`request_dependency`
  tools (request, policy evaluation, and run-harness pause) are implemented and
  claimed by the current CLI. Automatic exact dependency-request fulfillment
  remains an explicit unmet target; protected requests fail closed rather than
  invoking a host package-manager fallback.
  Whole-component-minus-exclusions writing scope (write at the component root
  for `Cargo.toml`/`deny.toml`) remains a target requirement.
- Configuration is limited to 64 KiB. Component Markdown and YAML artifacts
  read by the maerg are limited to 1 MiB.
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
