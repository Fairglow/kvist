# Maerg scoped status/navigation: source-blind diagnostic comparison

Date: 2026-11-04.

## Status, scope and authority

This is a bounded diagnostic comparison, **not approval, certification, a
security audit, a native compliance result, or intent acceptance**. Discrepancies
and evidence gaps are recorded for explicit arbitration, not automatic changes.

The comparison used only:

- `maerg/REQUIREMENTS.md`: REQ-DISCOVERY-STATUS, REQ-SCOPED-STATUS and relevant
  interactive component-prefill/navigation provisions.
- `maerg/CONTRACT.md`: status CLI and interactive-host semantics.
- `maerg/DESIGN.md`: scoped status, focus insertion and focus/history transitions.
- The first four scoped-status definitions and states in `maerg/TODOS.yaml`.
- The scoped-status section of `maerg/IMPL.md`.
- `scoped-status-observation.md`, **including its final separate clean-slate
  correction observation**.
- `ROOT_CONTRACT.md` for review separation and authority constraints.
- The parent's supplied implementation-agent execution summaries, including
  the final full-suite follow-up below.

No Rust production or test source, source diffs, Git history, peer
implementations/intent, or earlier unrelated reviews were inspected. Source
references in the observation were not followed. No tests or workflow commands
were executed by this reviewer. Only this report was written.

## Evidence attribution

The source-only observation independently describes implementation and
test-source assertions, not executed results. Its separate correction
observation supersedes original findings 1 (run-advice gates) and 4 (idle TUI
history recall). Original findings 2 and 3 describe retained semantics and
inspection limits; they are not automatically requirement violations.
`maerg/IMPL.md` summarizes these observations and is not additional independent
execution evidence.

The implementation agent supplied the following execution evidence; this
reviewer did not execute it or independently inspect its logs:

- Initially all six new scoped-status tests failed. Prefill tests exposed
  missing focused-status insertion and unwanted `cd` insertion; the degraded
  bare-`cd` regression failed. New run-advice and recalled-status assertions
  also failed before their respective repairs.
- After repairs, seven targeted integration suites passed **107 tests**:
  completions 1, dogfood_boundary 78, markdown_output 3, scoped_status 8,
  shell_smoke 2, status 14 and tui_workspace 1. This included actual PTYs for
  both hosts. The bark library selector passed **271 tests**.
- Strict all-target/all-feature Maerg Clippy and workspace formatting passed.
- All-feature Nextest run `45740d66-0bf8-483f-bd03-954f988d6865` ran **1083
  tests: 1082 passed and one failed**. The old assertion treated literal `.`
  as project scope. It now explicitly selects `--project .`; its targeted
  rerun and all eight scoped-status tests passed. That correction agrees
  with the written distinction between project selection and root-component
  `.`. This failed run is retained as historical evidence.
- The final all-feature Maerg Nextest run
  `4dc3a369-acbe-4159-ac10-5d1ee25b2c00` completed successfully: **1083 tests
  across 44 binaries, 1083 passed and 7 existing skips**, in 143.421 seconds
  (build: 11.89 seconds). The parent supplied this implementation-agent tool
  result; the execution log is
  `/home/stefan/.copilot/session-state/edb68e2c-6dec-4a84-af1c-902fb3ad0884/files/scoped-status-nextest-verified.log`.
  This reviewer did not inspect that log. Strict all-target/all-feature Maerg
  Clippy and workspace formatting also passed after the fixture updates.
  **The pending-full-rerun evidence gap is resolved on this attributed basis.**
- Release build, component-document structural validation and a release
  scoped-JSON smoke check passed. No component acceptance, audit receipt or
  protected execution occurred.

## Bounded comparison

| Intent | Independent observation and comparison |
| --- | --- |
| Compact project status has one row per matching component, state, artifact health, task progress and an actionable next command; no repeated component detail tables/digests/reasons. | The observed compact Markdown projection has those fields, explicit empty results, bounded queue reads and visible queue failures. Detail-only content is omitted. Attributed scoped-status/Markdown passes support the described examples, not exhaustive correctness. |
| Exact component selection applies to every format; invalid selections fail instead of falling back. | Selection precedes presentation and uses exact discovered paths. Healthy sibling/descendant artifact and VCS detail is filtered out. Literal `.`, positional project selection and explicit `--project` are distinguished as documented. |
| `--details` expands the chosen scope; component selection already requests human details. Existing artifact/state filters and native version-one shapes remain. | Observed dispatch retains a component filter under `--details`; project details and artifact-only details are separate from compact output. Native text/JSON remain version-one projections, including global JSON routing. There is no requirement that `--details` widen a selected component to the project. |
| Global problems remain visible, including in selected reports; inspection is read-only. | Global diagnostics/classification and VCS summary are retained rather than recomputed away after filtering. The observation describes inspection/rendering and an unchanged-queue assertion; attributed tests support read-only examples. This does not establish exclusive filesystem access or universal absence of side effects. |
| Execution advice requires both current project and current component in compact and detailed Markdown. | Original finding 1 contradicted this intent. The separate correction observes both gates in both projections; details retain ready-task facts with “Execution unavailable” when execution is not eligible. The attributed failed-before/passed-after assertions support the repair. The original defect is historical, not a remaining finding. |
| Both editors visibly insert selected focus for bare `status`, preserve explicit values/deletion, and treat history recall like typing. | Shared insertion modifies the real buffer, handles bare `status` without trailing whitespace, observes explicit values and suppresses reinsertion after deletion until submission reset. Original finding 4 contradicted idle TUI recall intent; the correction observes immediate prefill after idle recall. It deliberately does not prefill child stdin while a command runs, matching the design. Attributed PTY and recalled-status results support the repaired behavior. |
| `cd` remains completable but never prefills a destination; bare `cd` and `cd -` return to project scope, while explicit selection changes focus. | Both hosts exclude `cd` from prefill and clear focus for bare/dash forms. The TUI restores project transcript/history through its focus transition. Explicit values remain selectable; no hidden status dispatch fallback defeats deletion. Attributed PTYs and degraded-host regressions support the observed paths. |

### Semantics that are not discrepancies

- **`--unfinished` retains non-current-component-state filtering.** A current
  component with unfinished tasks can be omitted. This preserves the existing
  state filter required by the bundle; no task-completion interpretation was
  specified.
- **Exact output selection is not isolated inspection.** Full project discovery
  and inspection may precede filtering, and peer filesystem failures can abort
  a selected report. Non-valid peer artifacts retained as global diagnostic
  context do not create sibling component rows or descendant selection.
  The contract expressly retains global problems; it does not promise exclusive
  selected-directory reads.
- **Global-health gating may leave ordinary project status without component
  rows.** The observer notes potentially generic “No components match” wording,
  but global diagnostics remain. This is a presentation limitation, not evidence
  that discovered matching components were silently omitted after selection.
- **Native projections are not Markdown task-progress tables.** Keeping their
  existing shapes is intentional. The observation's best-effort supplementary
  native-text blocked-task read is a limit, not by itself proof that initial
  artifact inspection concealed a queue defect.

## Discrepancies and remaining evidence gaps

No high-confidence **remaining implementation contradiction** is established
by the allowed evidence after applying the separate correction observation.
The historical run-advice and idle-recall contradictions are explicitly
superseded, not ignored.

The formerly pending final full-suite outcome is now supported by the
attributed successful run above. This does not erase the earlier failed run
or independently establish the result through this reviewer.

The following remain unverified here:

1. **Some surrounding navigation obligations.** The observation explicitly
   establishes TUI transcript/history restoration and degraded-host focus
   clearing, but does not separately establish degraded-host history-scope
   restoration or trace menu-selected focus through the same transition as
   `cd`. Aggregate passing counts do not identify those particular assertions.
   These are evidence gaps, not inferred defects.
2. **Detailed diagnostic completeness.** The observation describes revisions,
   blocked reasons and native diagnostics, but does not specifically establish
   every required aligned revision border or full, untruncated blocked-reason
   first line. Its supplementary native-text queue-read limitation also
   precludes an unqualified claim that every possible read/parse failure is
   visibly explained by that helper. Existing initial inspection may supply
   diagnostics; no counterexample was independently demonstrated here.
3. **Adverse filesystem behavior and independent gates.** No permission-error
   reproduction or independent read-only/path/input audit was supplied.
   Successful structural validation and scoped smoke checks are narrower than
   those obligations and cannot replace protected review.

## Queue and acceptance state

The first four scoped-status tasks retain the required order and dependencies.
For the parent's supplied final task disposition, `write_tests` and
`implement_code` are completed; `security_audit` then `compliance_review` remain
pending. The earlier inspected queue snapshot had implementation in progress;
the parent is completing it based on the final evidence. This final disposition
is attributed to the parent, not an independent queue reread or a transition by
this reviewer. Only this report was updated. The requirement's explicit advisory
exception is not an advisory receipt or acceptance record.

**Native security-audit/compliance gates and intent acceptance remain pending.**
No protected execution, acceptance, approval or certification is implied by
this comparison or by the attributed passing commands.
