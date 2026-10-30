# Continuous execution: source-blind comparison

Date: 2026-10-30. Reviewer: GitHub Copilot, separate from the implementer and
source observers. This is a scoped diagnostic comparison, not compliance,
acceptance, security assurance, or certification of model-generated findings.

## Exact scope and evidence

Compared intended behavior:

- `ROOT_CONTRACT.md`: component/context boundaries, independent observation and
  comparison, execution versus acceptance, checkpoint ownership, and recovery.
- `maerg/REQUIREMENTS.md`: `REQ-CONTINUOUS-EXECUTION`,
  `REQ-RUN-COMPONENT`, `REQ-COMPLIANCE`; only related review diagnostics in
  `REQ-SHARED-RUNTIME`, supervised execution/recovery, continuous shell execution
  and cancellation, editable component prefill in `REQ-INTERACTIVE-TUI` and
  `REQ-SETTINGS-MENU`, and applicable bounded-resource/VCS constraints.
- `maerg/CONTRACT.md`: required explicit run arguments and continuous execution,
  item/component/manual checkpoint policies, independent review framing,
  diagnostic redaction/controls, shell run dispatch and focus, and applicable
  recovery, approval, signing and isolated-index guarantees.
- `maerg/DESIGN.md`: related shell focus/prefill design and “Run the whole
  component” through editor suspension, including ledger authentication,
  checkpoint processes, fresh review phases, fragmentation and redaction.

Observed evidence, in chronological order:

1. `continuous-execution-observation.md` (this directory): independent initial
   source/test observation; no personally rerun tests. Its host-attributed
   execution counts are not independent verification.
2. `continuous-execution-corrections-observation.md`: later independent
   source/test-only corrections; all described test assertions are unexecuted.
   Prefill/completion and shell-smoke sources were excluded from that follow-up.
3. `continuous-redaction-predicate-observation.md`: latest independent, narrow
   source/test-only predicate observation; no execution.
4. Only “Scoped continuous-execution observations - 2026-10-30” in
   `maerg/IMPL.md`: summary of those records plus explicitly
   **implementation-agent-attributed** execution results. Reported package,
   formatting, Clippy, release/validation and PTY successes do not independently
   verify this comparison.

No implementation/test source, source diffs, Git/history, session/chat history,
queues, peer implementations or other review content was inspected. No builds,
tests or agents were invoked.

## Comparison supported by the records

- Required `task run COMPONENT [TASK_ID]`, exact single-task selection, iterative
  dependency-ready scheduling under one operation lock, stopping on decisions,
  blockers and unresolved attempts, and durable awaiting-approval state are
  described. Verified continuous implementation completion remains distinct
  from explicit single-task finalization and human acceptance.
- Approval-bound item/component/manual policies, defined-item checkpoint
  completion, exact owned paths, isolated-index preservation of unrelated
  staging, signing, prepared commits and expected-head publication are
  described. Authentication precedes terminal-ledger replacement; new authored
  unfinished work can establish a fresh approved baseline without replaying
  completed tasks. Pending commits and ambiguous attempts retain recovery
  restrictions.
- Fresh tool-free observation/comparison requests, no authoring history or
  configurable authoring prompt, durable phase reports, decision propagation,
  capacity-aware UTF-8 fragmentation and shared wall/token accounting are
  described. Structural framing is not proof of truthful or complete findings.
- Actual editable-buffer insertion, explicit/partial argument precedence,
  deletion suppression, quoting and dash-leading paths are described by the
  initial observer. Later evidence describes explicit shell/CLI path dispatch;
  it does not independently re-observe editor behavior. Implementer PTY results
  reached the missing-policy-approval gate, not protected task execution.

The initial escaped-diagnostic and unattended Git/signing-process findings are
superseded by the later pre-serialization/stream redaction and five-minute
per-command supervision observations. The later whitespace-only validation
finding is superseded by the latest predicate observation: blank values are
rejected while nonblank literal bytes are preserved. These are not retained as
current defects. Partial lifecycle items are explicitly permitted by current
intent; absence of four roles per inferred item is not itself a discrepancy.

## Definite narrow discrepancy

**Group-wide reaping is not the observed guarantee.** The checkpoint design
says deadline/cancellation “kills and reaps that owned group.” The corrections
record describes SIGKILL to the pinned process group but waiting/reaping only
the direct child; its helper assertion accepts absent **or zombie** descendants.
Literal group-wide reaping therefore exceeds the described implementation
guarantee. Human arbitration must resolve this design/realization mismatch.
This does not prove that group termination failed or that a checkpoint hangs.
Neither intent nor implementation was changed to reconcile it here.

## Coverage gaps and limitations—not established defects

- No protected native-runner execution of this feature's queued security or
  compliance tasks is evidenced. No protected receipt or final acceptance is
  implied. Mock responses, source assertions and implementer executions cannot
  substitute for that missing evidence.
- The records do not establish every required exact stop-state/remedy command,
  item-name edge case, or abrupt-crash/failure ordering. Authenticated snapshots
  and explicit retry paths do not establish exhaustive recovery correctness.
- Paused edits can retain completed reviews without new verification; effect
  digests exclude the queue. Final review/evidence freshness needs explicit
  human assessment, not an assumption that all external edits are detected.
- Source frames omit root build inputs; binaries are digest-only. Filename
  exclusions do not remove embedded intent/fixtures or nested component content.
  Complete fragment traversal does not establish semantic completeness,
  uncontaminated source-only meaning, or truthful synthesis.
- Five minutes bounds supervised commands/transactions, not the entire
  checkpoint. Filesystem work, spawn and blocking cleanup waits lack independent
  deadlines; descendants leaving the group are outside cleanup. Ref advancement
  and index installation remain separate recoverable operations.
- Administrative path announcements bypass literal redaction. The contract
  separately calls private paths administrative identifiers, so this is not an
  unambiguous redaction breach on these records alone. Human arbitration should
  settle whether secret-bearing paths must be covered by broader review-sink
  language. Literal/JSON-escaped matching is not arbitrary-encoding detection;
  model-input evidence is not scrubbed by output redaction.
- Context accounting is estimated; provisional capacity is not discovered model
  truth. Output caps are per documented sink, not one aggregate disk ceiling.
  Linux/procfs dependence, inventory bounds and editor undo-history reset remain
  disclosed limits.

## Conclusion

The records support substantial feature-level correspondence, with one concrete
group-reaping design discrepancy requiring human disposition. Remaining gaps
and path-redaction ambiguity must not be relabeled as proven runtime defects or
silently treated as satisfied requirements. This record supplies neither a
compliance verdict nor authority to accept the component.
