# Task attention Markdown — source-only observation

## Evidence boundary

This fresh observation uses only the bounded current Maerg source and scoped test
source cited below, including immediate persistence/redaction helpers. No intent,
queue files, prior records/reviews, Git history/diffs, execution logs, external
examples or other agents' results were read. No build, test, application command
or agent was executed. This is neither compliance nor security certification.
Paths and line ranges below are repository-relative.

## Observed implementation

- **Authorship and presentation.** `propose_decision` asks the model for situation,
  implications, alternatives/tradeoffs, recommendation and missing evidence.
  Enforcement checks nonempty, byte-bounded strings, not these sections or their
  factual quality: summary/why each allow 8 KiB; patch allows 64 KiB.
  (`maerg/src/authoring/mod.rs:91–97,355–362,419–446,683–708`.)
  The host writes redacted JSON plus Markdown; summary/why remain agent-authored,
  and an optional draft is indented and labelled not applied.
  (`maerg/src/agent.rs:1969–2010`.)
- **Collected explanation.** Returned stdout combines the last model text and
  surfaced proposals, then redacts and truncates to the profile output limit
  without a truncation marker. Thus a tool-only response can supply an
  explanation, but the attention copy need not contain the entire proposal.
  This turn's effects are skipped when a decision is recorded; this does not
  establish that earlier turns had no effects.
  (`maerg/src/agent.rs:1355–1375,1626–1668,2074–2089`.)
- **Decision lifecycle.** The task command writes the attention brief before
  transitioning to awaiting-decision; output and the stored reason name its
  mutable Markdown alias and raw-evidence directory. A write failure prevents
  that transition and can leave an unresolved attempt. The presentation helper
  itself neither calls a model nor disposes a task.
  (`maerg/src/task_commands.rs:4844–4895,5169–5197`;
  `maerg/src/task_commands/attention.rs:62–94`.)
- **Error coverage.** The named-task wrapper catches all returned helper errors,
  including early preflight and lock-release errors. Available successful-result
  stdout is retained for later failures; execution returning `Err` supplies no
  agent explanation. The continuous wrapper also catches preflight/checkpoint
  errors and can produce a run-level brief around a task-level error.
  (`maerg/src/task_commands.rs:4447–4508,4751–4815,5016–5091,5121–5197`;
  `maerg/src/task_commands/continuous.rs:44–79,312–339`.)
  Failure text is host-derived, labels any earlier explanation advisory, lists
  queued blockers/decisions and offers generic alternatives—not new inferred
  design advice. Invalid component/configuration resolution or persistence
  failure yields an explicit unavailable-brief diagnostic.
  (`maerg/src/task_commands/attention.rs:87–190`.)
- **Human response.** Host instructions distinguish choosing, clarification,
  rejection and deliberate resumption; warn against replay/deleting fences;
  quote command arguments; and say chat alone is not acceptance. There is no
  added model retry or decision-response/disposition operation in this
  presentation path. Existing task execution still performs its own transitions.
  (`maerg/src/task_commands/attention.rs:8–60`;
  `maerg/src/task_commands.rs:4794–4812,4881–4889`.)
- **Bounds, redaction and paths.** Attention/review Markdown rejects output above
  256 KiB after literal configured-value redaction. Host causes alone are
  truncated at 128 KiB with a marker; a large explanation/queue can still prevent
  the whole brief. This is not general secret detection or Markdown sanitization.
  Attention checks directory ancestors, hashes unsafe task filenames, retains
  content-addressed versions, verifies existing history bytes and replaces the
  regular non-link alias. These are path checks, not demonstrated resistance to
  concurrent filesystem replacement.
  (`maerg/src/task_commands/attention.rs:116–154,193–280,355–378`;
  `maerg/src/task_commands.rs:5486–5506`.)
  Writes use synchronized same-directory staging; history is no-clobber, aliases
  are replacements. Multi-file publication is not one transaction.
  (`maerg/src/file_io.rs:14–90`.)
- **Review companions.** The reviewer prompt requests Markdown and explicitly
  denies certification authority. Review JSON and Markdown have attempt-specific
  files plus replaceable task aliases; either observation or comparison can
  request arbitration. Final state writes `component.json` then `component.md`,
  whose index links task aliases and explicitly notes absent historical Markdown.
  The index checks file type, not each report's source digest/freshness.
  (`maerg/src/agent/review.rs:371–423`;
  `maerg/src/task_commands/continuous.rs:633–676,791–867`;
  `maerg/src/task_commands/attention.rs:283–352`.)

## Unexecuted assertions, not execution evidence

Source assertions cover tool-only proposal JSON/Markdown and stdout
(`maerg/tests/agent.rs:253–370`); preflight briefs without queue changes
(`maerg/tests/task_commands.rs:1592–1609`); review decisions, response instructions
and later-task pause (`2216–2281`); authoring decisions (`2286–2345`); failed brief
writes retaining unresolved state (`2350–2401`); checkpoint stops (`2406–2448`);
and malformed-review blockers (`2582–2605`). Review redaction assertions inspect
report-directory sinks (`2650–2722`). Attention unit assertions address bounds,
history, quoting and link refusal
(`maerg/src/task_commands/attention.rs:396–474`).

These are not pass results. The scoped assertions do not establish realistic
model brief quality, rendered Markdown behavior, index freshness, concurrent
path safety or repeated tool-call-ID retention.

## Concrete retention concern

Raw proposal filenames hash **only the tool-call ID**, and both JSON and Markdown
are replaced, not retained per attempt. A later proposal reusing that ID—even
from another task in the component—overwrites earlier raw evidence
(`maerg/src/agent.rs:1990–1992,2043–2050`). Attention history preserves its own
possibly truncated text, not the original full JSON proposal. This overwrite
behavior is definite from source; no runtime reproduction was performed.
