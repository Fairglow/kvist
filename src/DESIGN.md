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

Significant artifact separation rationale is retained in
`docs/decisions/0001-separate-component-intent.md`.

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

Prompt and subprocess data are bounded before persistence. Literal configured
redactions apply across output chunks and streams. Errors avoid secrets and
success-shaped fallbacks.

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
