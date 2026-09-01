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

The child `agent_runtime/` directory is a separately packaged component with
its own requirements, contract, design, queue, record, tests, and manifest.

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
the runtime's fixed-prompt qualification only. A successful qualification
continues to role selection and atomic configuration persistence; a failed
qualification exits before those steps unless `--force` was supplied.
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

Sandbox runner launch validates file identity and uses descriptor-bound Linux
execution to reduce time-of-check/time-of-use substitution. Timeout and output
overflow terminate the runner process tree and become explicit failures.

## Security and resource design

All filesystem entry points inspect metadata without following final links,
enforce regular-file and size rules, and avoid shell interpolation. VCS
inspection is read-only and preserves non-UTF-8 paths internally.

Project-controlled sandbox configuration cannot approve itself. Approval state
is authenticated in user-owned storage and bound to exact project, worktree,
root-contract, runner, and policy identity. Environment inheritance is
allowlisted, network is denied, and the component is the only writable mount.

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

Future review work must additionally test stable task projection, exact-digest
receipt matching, acknowledgement and exception paths, opt-out visibility,
no-agent refusal, report redaction and bounds, `.kvist/reviews/` discovery and
staleness exclusion, strict review context, and evidence that `component accept`
performs no agent or network work. Observed-intent tests must cover no-clobber
drafting, uncertainty, absence of normative contract generation, and
non-mutating comparison. Contract-verification tests must cover locator
stability, test-to-clause mappings, approved execution evidence, and
uncovered/failed clause reporting.
