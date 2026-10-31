# Numbered decision responses: independent source-blind comparison

Compared 2026-10-31. This is a bounded local-document diagnostic, not a protected
task run, native review receipt, task disposition, compliance certification, or
final implementation acceptance.

## Evidence boundary

The only evidence read was:

- `/opt/proj/kvist/maerg/REQUIREMENTS.md`, scoped to
  `REQ-DECISION-AUTOMATION` and directly applicable local constraints;
- `/opt/proj/kvist/maerg/CONTRACT.md`, scoped to deterministic decisions and
  directly applicable shell, authority, and verification clauses;
- `/opt/proj/kvist/maerg/DESIGN.md`, principally “Deterministic decision workspace”;
- `/opt/proj/kvist/docs/reviews/decision-shortcuts-observation.md`, including its
  narrow source-only correction addendum;
- `/home/stefan/.copilot/session-state/778fd089-f61b-4bbd-8891-e55cc9d22944/files/decision-shortcuts-tests.log`,
  including every final test-result summary through EOF.

Scoped searches were confined to the three permitted intent files. No
implementation or test source, existing `IMPL.md`, queue, other review, Git
history/diff, chat history, other component, guide, or checkpoint was inspected.
No Cargo, model, live operation, or subagent was invoked. The observation is
independently supplied source-derived evidence, not source inspected by this
comparer; its assertions are not independently re-certified here.

## Final runtime evidence

The permitted final log contains **41 `test result: ok` summaries**. Their
passed counts, in order, are:

```text
594, 0, 8, 8, 32, 1, 3, 5, 1, 1, 5, 8, 11, 78, 5, 3, 7, 2, 7, 1, 11,
6, 2, 1, 8, 10, 7, 11, 5, 4, 3, 6, 2, 14, 76, 9, 2, 7, 5, 13, 0
```

These sum to **982 passed, 0 failed, 7 ignored**, with zero measured or filtered
tests. The ignores occur as 1 in the initial 595-test suite, 2 in a four-test
suite, and 4 in a seven-test suite. The coordinator identifies them as existing
ignores; this log independently establishes their count, not their historical
status. The log reaches a final zero-test successful summary.

The observation's earlier truncated snapshot and reported checkpoint-test
failure are historical evidence, not the final result. Its addendum's statement
that a post-correction run was pending likewise predates this complete log.
The coordinator supplies the post-correction attribution; no source-revision
binding was independently established within this evidence boundary.

`fmt --check`, strict all-target Clippy, release build, and component validation
passed **according to the coordinator**. This comparer neither executed those
checks nor inspected their output. The quiet test log reports aggregate
success, not named per-test outcomes or a coverage matrix.

## Comparison against current intent

The governing numbered-response requirement is `REQ-DECISION-AUTOMATION`
(requirements lines 956–1012), elaborated by the deterministic decision
contract (lines 272–346) and workspace design (lines 1184–1266).

| Obligation | Supplied observation and diagnostic assessment |
| --- | --- |
| Component-scoped numbers retain one exact typed action; no reassignment within retained state | The authenticated identity-bound registry deduplicates typed actions and allocates monotonically. Explicit component grammar and completion scope do not fall back to shell focus. No contradiction established. |
| Full expansion is visible; invocation uses native dispatch, not displayed shell text | Listings/completion expose descriptions and commands; `do` echoes its expansion before execution on stderr, including with JSON stdout. Stored typed arguments dispatch directly. Displayed command strings are never evaluated as shell authority. No contradiction established. |
| Choices only prepare; apply-only and apply-and-continue remain separate | Option selection performs planning/review without controlled changes. Separate approval actions bind the exact frozen plan, approval token, and continuation flag. Number invocation supplies explicit approval without another prompt or model selection. No contradiction established. |
| Native expansion and number enforce identical option freshness | The correction addendum exposes `plan --catalog SHA256`, requiring `--option`, in both native grammar and displayed expansion. Both routes pass the same digest to preparation. Exact catalog bytes pin the embedded capture identity, which is also validated against the current capture. No current discrepancy established. |
| Unknown, tampered, cross-component, or stale choices fail without retargeting | Observation describes authenticated strict registry validation, exact catalog mismatch refusal before inference, exact-plan freshness checks, and no latest-plan fallback. Revised regression assertions cover refusal through both an old number and its native expansion. No contradiction established. |
| Existing exact commands and authenticated retained-plan discovery remain available | Native planning/import/application remain available; `actions` discovers authenticated plans without inference. Pending transactions prevent new shortcut publication; completion lists read-only state. No contradiction established. |
| Exact application has deterministic, fenced recovery; continuation uses ordinary approved scheduling | Observation describes lock, authenticated progress journal, before/after-byte recovery, idempotent receipt, and pending-transaction fencing. Continuation occurs after application and lock release through the ordinary scheduler, with inserted prerequisites before the resumed review. Failure after application reports retained effects. No contradiction established within the observed boundary. |
| Independent review and prior evidence are not converted into acceptance | The observation describes stale-evidence identification, not rewritten implementation evidence or successful task disposition. Planning/review acknowledgement is distinct from implementation acceptance. This diagnostic supplies neither acceptance nor certification. |

### Historical catalog-pin defect

The original observation identified a concrete violation of exact-command
equivalence: numbered selection carried an additional freshness pin that the
displayed native command could not replay. After catalog replacement, the old
number refused while its unpinned expansion could select revised content.

The explicitly permitted source-only addendum reports correction of that
specific defect through the shared native `--catalog` argument and removal of
the numbered-only pin. The revised test assertions exercise stale refusal for
both invocation routes; the final supplied log is now successful. Consequently,
the missing-pin defect is **historical, not an established current finding**.
This does not establish equivalence for every possible input or certify the
source-only correction.

## Explicit limits, not stronger guarantees

- Numeric stability depends on retaining the current registry. Deletion or
  restoration is not supported as a means of preserving old numeric references.
  No lifetime high-water mark, tombstone, or authentic-rollback detection is
  promised. This matches the requirement's “within retained shortcut state”
  qualification.
- Listings and completion include historical, stale, and applied actions;
  invocation checks applicability. They are not promised to show only currently
  actionable responses or label freshness.
- Repeating an already-applied continuing response can schedule current ordinary
  work again. Exact application is idempotent; continuation is explicitly not
  exactly-once and does not stay bound to the original queue snapshot.
- The contract expressly caps the registry at **2,048 actions and 1 MiB JSON**,
  provides no pruning command, and preserves existing numbers on limit failure
  with full-command fallback. The observation additionally describes bounded
  discovery at 4,096 entries and 8 MiB encoded plans. Unbounded retention or
  indefinitely available discovery is not promised.
- Authentication does not protect against an actor possessing the user-owned
  approval secret. Linux-only executable support is the local quality limit;
  this evidence does not establish other-platform behavior.
- Numbered planning does not expose import, custom guidance, or review-exception
  overrides; the full-command surfaces retain them. Optional ordinary
  continuation can invoke models or checkpoint work; model-free application
  concerns the exact apply phase, not that subsequent execution.

## Remaining verification gaps and arbitration

1. The corrected regression described by the observer checks stale native
   expansion refusal, but not successful planning through a fresh expanded
   command. Broader replay-equivalence evidence remains incomplete.
2. The observed PTY test covers real numbered completion/submission and stale
   refusal without queue mutation or an extra prompt. It does not demonstrate
   successful exact application through that PTY interaction.
3. Quiet aggregate runtime results do not independently map named shortcut
   regressions, recovery injections, and editor tests to individual passes.
   The observation supplies their asserted behavior; neither aggregate success
   nor those assertions prove exhaustive security, concurrency, or failure
   handling. Calls outside the observer's bounded source scope remain unaudited.
4. General shell confirmation requirements are elaborated by the contract's
   named destructive operations (lines 773–775), while numbered decisions
   explicitly make invocation the choice/approval. No requirement for an
   additional decision confirmation is established by these clauses.

**Conclusion:** no current concrete contradiction or genuine requirement
conflict requiring arbitration is established by the permitted evidence.
The historical replay defect is reported corrected, and the final log confirms
982 passing tests with seven ignores. The limits and gaps above remain for
explicit human consideration; this comparison does not close lifecycle gates,
certify observations, or authorize final implementation acceptance.
