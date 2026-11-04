# Markdown workspace: source-blind diagnostic comparison

Date: 2026-11-03 (UTC)

## Outcome and authority

No concrete **current blocking discrepancy** is established by the permitted
Markdown-workspace observations after their correction addendum. The five
original defects remain historical findings; the independent observer describes
corrections addressing each concrete example.

This is a bounded diagnostic comparison, not compliance certification, native
task execution, a review receipt, intent acceptance, or policy approval.
Unverified obligations below remain unverified, not implicitly satisfied.
No accepted revision or protected gate is changed.

## Evidence boundary and disclosure

The comparison uses:

- `maerg/REQUIREMENTS.md`: `REQ-MARKDOWN-OUTPUT`,
  `REQ-INTERACTIVE-TUI`, `REQ-TRANSCRIPT-PERSISTENCE`,
  `REQ-SETTINGS-MENU`, and `REQ-RUN-COMPONENT`.
- `maerg/CONTRACT.md`: “Markdown-first output and the interactive TUI,”
  including its explicit native-output exceptions.
- `maerg/DESIGN.md`: “Markdown-first output and the Blad-rendered TUI,”
  including its continuous-execution paragraphs.
- `maerg/IMPL.md`: only the new “Scoped Markdown workspace observations”
  section as substantive implementation-record evidence.
- `docs/reviews/markdown-workspace-observation.md`, including the entire
  correction addendum.
- `docs/command-set.md`, section 4.5, as consumer documentation.
- The task's explicitly attributed implementation-agent execution evidence and
  one ending-of-log snapshot from the designated final aggregate log.

**Input-boundary exception:** the initial range reads were too broad. They
returned adjacent intent text and unrelated, older `IMPL.md` sections, including
references and summaries of earlier reviews. Those sections were not used as
comparison evidence, and their linked documents were not opened. Nevertheless,
this was not a strictly permitted-input-only clean-slate read. Independence from
implementation/test source is preserved; the broader input-boundary violation
is disclosed rather than concealed.

No implementation source, test source, queue, peer artifact, Git diff/history,
or prior conversation was deliberately inspected. No agents were invoked.
The comparator personally performed document reads, the single log snapshot,
and report creation/readback; it ran no tests, builds, formatter, linter, PTY
session, or live command workflow.

## Comparison against authored intent

The intent documents establish obligations. The observer supplies descriptions
derived from selected source and test bodies, not executed results. This
comparison does not treat either kind of statement as interchangeable.

| Obligation | Supplied observation | Assessment |
| --- | --- | --- |
| Canonical readable Markdown; structured status, reasons, revisions and JSON report bodies; native machine formats preserved | The observer describes structured status/doctor facts, recursive JSON projection retaining unknown metadata, inert child controls, and explicit native routes. The addendum corrects ordinary stream line preservation, legacy line separation and incomplete human repair. | The concrete presentation contradictions found by the observer are addressed. Universal command-path and semantic completeness are not established. |
| Bare interactive entry; finite redirected invocation; no JSON TUI; degradation path | The observer describes terminal-gated bare entry, finite redirected routes, global JSON handling, and a distinct Reedline fallback selected by capability, size and environment checks. | No current contradiction identified in the described routing. This is not a terminal/platform matrix execution. |
| Host-owned drawing/input; argument-vector children; readable output; responsive navigation/cancellation; quit reaping; paste safety | The observer describes isolated pipes, owned process groups, bounded channels, polling, control sanitization, navigation and paste normalization. The addendum supplies child stdin EOF on empty Ctrl+D and a separate Ctrl+Q quit/cancel path. | The concrete missing EOF path is addressed. Correctness under arbitrary process/terminal behavior is not established by this record. |
| Status heading; project/component focus; visible editable prefill; deletion and task argument order; CLI completion/help | The observer describes focus/state/progress and branch/sandbox/model/lock presentation, clap-derived completion, dynamic snapshots, checked replacement spans, visible component insertion and deliberate-deletion retention. | Described mechanisms match the scoped obligations. Exhaustive command/argument combinations and rendered-screen fidelity remain unverified. |
| Canonical bounded-memory history with unlimited total retention, safe files, older-segment access, scope restoration and explicit errors | The observer describes bounded segment reads, block-aware rotation, no total pruning, one displayed segment, cross-segment navigation, private regular nonfollowing files, writer exclusion and fatal recording errors. | No current contradiction established. The contract explicitly permits 1-MiB targets, a 16-MiB indivisible-block ceiling and independent page rendering. These are not unlimited individual-document guarantees. |
| Settings for component, model, effort, history and theme; persistent immediate selection; unchanged authority | The observer describes settings overlays, focus changes, persisted theme handling and developer-role configuration actions without policy approval. The addendum replaces unsafe theme/preference reads with bounded regular-file reads. | The concrete file-type/read-bound defect is addressed. This does not independently prove every protected configuration transition or policy interaction. |
| Bare component run follows dependencies, stops at completion/blocker/decision, never resolves pauses itself, and preserves locking/evidence/recovery | Authored contract/design and consumer documentation agree on these semantics. The scoped workspace observation describes task dispatch with streaming, but does not independently document the scheduling loop and its stop/lock/evidence behavior. | Insufficient scoped observed evidence for a full continuous-execution comparison; no concrete violation is established. Agreement among intent documents is not implementation proof. |

### Historical findings and corrective evidence

1. **Streamed Markdown/redaction:** the addendum reports removal of the extra
   ordinary-line newline. Source assertions check exact heading/table/code
   preservation at chunk sizes 1, 7 and whole-input, and ordinary multiline
   redaction for whole/split input. These address the concrete defect, not all
   possible stream segmentations.
2. **Embedded prompt EOF:** the addendum reports dropping the answer sender so
   the writer closes child stdin after queued answers. The PTY assertion now
   submits stdin prompt text, sends Ctrl+D, and waits for a successful-command
   marker. Ctrl+Q was source-inspected, not exercised by that observer.
3. **Legacy prose/metadata concatenation:** the addendum reports separator
   insertion and exact prose-to-metadata/bullet regression assertions.
4. **Incomplete human repair:** the addendum reports a Markdown error payload
   for the human route and unchanged serialized JSON for explicit JSON.
   The integration assertions distinguish those two stderr forms.
5. **Theme/preference I/O:** the addendum reports nonfollowing/nonblocking,
   opened-regular-file bounded reads and tests for FIFO rejection, oversized
   content, and valid/oversized symlink targets. Explicit theme-path
   canonicalization still permits a valid symlink target; it does not claim
   arbitrary ancestor-race isolation.

The related explicit writer unlock and reduced-write append changes are
source observations with a duplicated-descriptor regression assertion, not
proof of crash safety or measured performance.

## Execution evidence and attribution

The independent source observer executed no tests in either its original
observation or correction follow-up. Its lists of tests describe assertions
present in source only.

The task supplies separate implementation-agent execution evidence:

- 21 TUI unit tests and affected CLI/prompt/status/repair/PTY integration suites
  passed after corrections.
- An earlier all-feature workspace run passed 2,244 tests with 15 skips
  **before** the final corrections.
- A separate highly concurrent single-process library invocation failed an
  unchanged checkpoint signer-start timing assertion because `signer.pid` was
  absent. No root cause is established; neither harmless flakiness nor
  attribution to this transition is proved.

The comparator read the ending of
`/home/stefan/.copilot/session-state/edb68e2c-6dec-4a84-af1c-902fb3ad0884/files/workspace-nextest-final.log`
once. Its last available lines showed compilation finished and Nextest run
`ec037c96-f8ac-4e39-99a5-d4a5888da51e` **starting 2,250 tests across 112 binaries,
with 15 tests skipped**. There was no terminal test summary.

Thus final aggregate completion/success was **not established at the read**.
The log was not polled. The successful shell status of the log-reading command
establishes only that the snapshot was read, not that Nextest passed. The
earlier aggregate and the attributed targeted passes cannot be combined into
an invented final post-correction aggregate result.

## Remaining limits and arbitration points

- The universal Markdown requirement includes observed state, document
  validity, task progress, next-ready guidance and actionable non-current
  guidance. Observed status rendering supports those facts; a heading wrapper
  alone does not prove that every human command provides them. The scoped
  record does not establish a concrete currently missing field on a named
  command, nor establish full universal coverage.
- Native JSON/text, completion and internal-effect exceptions are explicitly
  documented and are not themselves Markdown defects. Version/empty-output
  bypasses described by the observer do not establish comprehensive command
  output interception. Selected passing assertions cannot close that gap.
- Bounded pages render independently; cross-page reference links and footnotes
  do not form one whole-history Markdown document. This limitation is explicitly
  disclosed in the contract and consumer documentation.
- Interrupted rotation may duplicate diagnostics. Filesystem checks do not
  isolate every ancestor race or kernel stall. No injected crash/power-loss
  proof is supplied.
- Masked answers are excluded from command history, but children may echo them.
  Configured redaction is not general secret discovery.
- Direct renderer assertions and PTY substring checks are not complete
  rendered-screen comparisons. Portability, all terminal capabilities, sustained
  output performance, every subprocess lifecycle and every native CLI route
  remain outside the established execution evidence.
- Continuous-run scheduling/authority coverage, the initial unfinished
  aggregate-log snapshot (superseded by the completion addendum below), and
  the disclosed input-boundary exception limit this report's use.
  They are evidence limitations, not invented runtime failures.

**Blocking discrepancies established here: none.** This does not close
security/compliance tasks or justify acceptance. Human arbitration and any
further independent verification remain separate from this diagnostic report.

## Evidence-completion addendum — 2026-11-03 (UTC)

This same-task follow-up inspected only the designated aggregate log and the
new concurrent-library log, once each, without re-reviewing source, tests or
intent. The earlier incomplete-snapshot wording is retained as historical
evidence; aggregate incompleteness is no longer a current limitation.

- `workspace-nextest-final.log`, in the session files directory identified
  above, now ends with Nextest run
  `ec037c96-f8ac-4e39-99a5-d4a5888da51e` reporting **2,250 tests run,
  2,250 passed (one slow), 15 skipped**, in 143.150 seconds. This confirms
  aggregate completion in the log, replacing the earlier startup-only snapshot.
  The parent identifies the skips as existing.
- `maerg-unit-concurrent-final.log`, in the same directory, ends with
  **624 passed, 0 failed, 1 ignored**, in 16.45 seconds. This confirms the
  separately attributed concurrent single-process library rerun's logged
  outcome. It does not establish a root cause for the earlier missing
  `signer.pid` timing assertion or prove that timing failures cannot recur.
- The parent additionally reports final strict all-target Maerg Clippy,
  formatting, and release-build success. Those are attributed parent execution
  results; no corresponding lint/format/build logs were inspected here.

These are observations of execution logs, not tests personally executed by the
comparator or independent proof of their complete environment and provenance.
No new concrete blocking discrepancy is established. The prior input-boundary
disclosure, semantic/coverage limits, and non-certification status remain.
No intent, implementation, tests, queue, accepted revisions or gates were changed.

## Final host-diagnostic comparison and evidence addendum — 2026-11-04 (UTC)

### Narrow scope and attribution

This follow-up used only the final “Host-diagnostic routing addendum” in the
source observation, the bounded host-log paragraph in the Markdown/TUI design,
the updated top Markdown-workspace section of `IMPL.md`, and one ending-of-log
snapshot from each of the three newly designated logs. It did not reopen the
earlier implementation comparison or inspect implementation/test source.
The existing overbroad-initial-input disclosure remains. A follow-up locator
search also returned unrelated design context; that context was not used here.
No agents, tests, builds or live terminal workflows were run by this comparator.

### Authored intent versus source-observed behavior

The design calls for the existing tracing filter, a bounded nonblocking
32-fragment channel with a 16-KiB fragment bound, fenced transcript diagnostics,
explicit truncation/loss notices, and theme warnings routed through tracing.

The independent observer describes a scoped thread-local subscriber installed
before workspace initialization, compact ANSI-free formatting, UTF-8-boundary
fragment truncation, nonblocking `try_send`, bounded draining, and a
collision-resistant `text` fence under `### Host diagnostics`. Theme fallback,
preference-read and optional-theme failures emit tracing warnings. These
observations support the intended scoped diagnostic route without establishing
universal interception or lossless logging.

Important precision and coverage limits remain:

- The subscriber is thread-local. Separately spawned host threads are not
  automatically redirected, and the global subscriber still writes to stderr.
  No actual out-of-band terminal-write defect is established in the inspected
  change, but an absolute all-thread “logging cannot ... draw over the
  terminal” guarantee is not established by this mechanism.
- Capacity and truncation bound retained writer fragments, not complete
  event-formatting allocations, total diagnostic volume, or total transcript
  retention. Nonblocking channel submission does not prove that all logging
  work, transcript I/O or kernel operations cannot stall.
- The design says “dropped-event count”; the observer describes counting
  unsuccessful fragment sends. This is a documentation-precision discrepancy:
  the notice must not be interpreted as a proven count of complete lost events.
- There is no final drain on initialization failure or closure exit. Events
  queued just before those paths may never enter the transcript. Explicit
  saturation/truncation notices do not imply an explicit notice for every such
  undrained fragment.
- The observer identifies a stale module introduction claiming exclusive
  stderr routing. That wording describes the global route, not the new scoped
  route; this is a source-documentation discrepancy, not an observed functional
  failure.

The observer's unit-test description supplies assertions for filtered capture,
burst-loss notices and oversized-event truncation. It explicitly does not
establish tested multibyte truncation boundaries or cross-thread routing.
The PTY assertion waits for a host-diagnostics transcript heading under
`KVIST_LOG=debug`; it is not proof of the absence of all out-of-band writes.
These remain descriptions of test assertions, not tests executed by the
observer or this comparator.

### Completed execution evidence

All three logs below are in the same session files directory previously
identified in this report:

- `workspace-nextest-terminal-bounded.log` ends with run
  `486b62b8-a5c5-43dc-9bcf-76de5ee1e670`: **2,251 tests run, 2,251 passed
  (one slow), 15 skipped**, in 293.178 seconds. The parent identifies the skips
  as existing and the test concurrency as two; the inspected ending confirms
  the result, not the concurrency setting.
- The preceding `workspace-nextest-terminal-final.log` ends with **2,249 passed,
  two failed, 15 skipped**, in 137.607 seconds. The failures name
  `mermaid::budget_tests::encoded_output_is_admitted_at_its_exact_byte_boundary`
  and `color_depth_never_changes_visible_geometry`. The inspected tail shows
  `deadline_exceeded` for the latter. The parent identifies this as the
  default-concurrency invocation and both failures as unchanged Blad
  rendering-deadline assertions. No root cause is established.
- `maerg-unit-terminal-final.log` ends with **625 passed, zero failed,
  one ignored**, in 15.16 seconds, confirming the logged concurrent
  single-process library rerun outcome.

The parent additionally reports successful final format checking, strict
all-target Maerg Clippy, release build and diff whitespace checking. No
corresponding check/build logs were inspected here; those successes remain
explicitly parent-attributed.

The newer results supplement, rather than erase, the earlier 2,250-test
aggregate and 624-test library snapshots. A successful two-test-concurrency run
does not convert the preceding default-concurrency failure into a pass or prove
its cause. Neither that result nor the successful library rerun explains the
earlier signer-start timing failure.

**No new concrete functional blocking discrepancy is established by this
narrow evidence.** Documentation precision, thread scope, bounds, final-drain
and test-coverage limits remain as stated. This addendum is diagnostic, not
certification, a native receipt, acceptance or authority to close any gate.
Only this comparison report was changed.
