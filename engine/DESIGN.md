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

| Area | Modules | Responsibility |
| --- | --- | --- |
| CLI boundary | `cli`, `main`, `error` | Typed grammar, dispatch, JSON/text output, domain errors |
| Durable artifacts | `artifacts`, `component_documents`, `file_io`, `filesystem` | Templates, Markdown validation, bounded safe reads, atomic writes |
| Project model | `config`, `discovery`, `project_state`, `tree`, `status`, `vcs` | Configuration, recursive layout, state classification, deterministic reports |
| Workflow | `task_queue`, `task_commands`, `sandbox` | Queue schema, lifecycle, locks, policy approval, runner protocol, evidence |
| Agent integration | `agent`, `prompt_input`, `wizard` | Role selection, prompt sources, standalone runtime integration |
| Onboarding | `init`, `convert`, `import`, `reverse_discovery` | New projects and explicit source-derived drafts |

The target `engine/` directory is both the root component and Rust workspace.
Its `agent_runtime/` and `sandbox_runner/` directories are separately packaged
child components with their own requirements, contracts, designs, queues,
records, tests, and manifests. The one-way migration from the current `src/`
layout is described in
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

Task execution adds policy approval, runner identity and capability checks,
read-only root and immediate-parent contract mounts, agent execution, output
redaction, and implementation test verification.

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

Significant artifact separation rationale is retained in
`docs/decisions/0001-separate-component-intent.md`.

### Planned dogfooding execution boundary

The engine replaces the unreleased sandbox protocol's original shape while
retaining protocol version 1. A request is a closed typed value containing the
phase, working directory, argv, environment, network capability, resource
limits, context paths, and mount grants. Each mount identifies its canonical
source, fixed sandbox destination, access, purpose, and approval-bound
identity. Unknown fields, purposes, overlaps, aliases, links, special files,
and paths outside approved roots fail before the runner is probed.

The Bubblewrap runner is a child component but is installed as one regular
executable outside the worktree. Engine approval binds the descriptor-launched
runner bytes, Bubblewrap path and digest, kernel capability result, typed
policy, toolchain, command, sources, and grant plan. The runner independently
parses the request and cannot import engine types or trust engine path
validation as a substitute for its own checks.

Authoring and verification use separate filesystem views. Authoring receives
only local component context and explicit writable implementation/test roots.
Workflow artifacts and evidence are read-only or absent. Verification can read
approved workspace metadata, provider source, toolchains, and dependency
caches needed by the build, but those files are not added to the agent prompt
or writable set. Nested child implementation paths are masked from a parent
authoring view unless separately granted.

The dependency phase runs Cargo acquisition without compiling. A
source-aware network boundary permits only configured registry index/download
origins or an exact approved Git repository and immutable revision. Cargo uses
an empty home plus attempt-local registry, Git, cache, lockfile, and scratch
paths. Valid content may enter a project cache only after bounded traversal,
checksum, lockfile, source, link, and concurrent-state validation.
Verification remounts that content read-only and disables network.

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

Acceptance and commit journals are separate. Acceptance does not become false
because the VCS operation failed. Recovery reports the accepted-but-uncommitted
state and exact retry command without silently committing a broadened or
changed set.

Sandbox runner launch validates file identity and uses descriptor-bound Linux
execution to reduce time-of-check/time-of-use substitution. Timeout and output
overflow terminate the runner process tree and become explicit failures.

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
