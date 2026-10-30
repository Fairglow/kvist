# Human attention briefs — source-blind diagnostic comparison

Date: 2026-10-30.

## Authority, evidence and boundary

This is a diagnostic comparison, not implementation work, source observation,
compliance certification, acceptance, or a protected review receipt. No source,
test source, source diff, Git/history, queue, peer implementation, or Galla
evidence/example was inspected. No tests, builds, application commands or agents
were run. Source locations mentioned by the observers were not followed.

The comparison uses:

- **Intent:** `ROOT_CONTRACT.md`, for human authority, component context and
  artifact/review rules; `maerg/REQUIREMENTS.md`, specifically
  `REQ-DECISION-SURFACING`, `REQ-CONTINUOUS-EXECUTION`,
  `REQ-SUPERVISED-EXECUTION` and `REQ-MARKDOWN-OUTPUT`;
  `maerg/CONTRACT.md`, **Human attention briefs** and directly related task
  lifecycle/independent-review behavior; `maerg/DESIGN.md`,
  **Human decision and blocker presentation**.
- **Observation A:** `docs/reviews/attention-brief-observation.md`, the initial
  source-only record, including its explicitly unexecuted test assertions.
- **Observation B:** `docs/reviews/attention-retention-observation.md`, the later,
  narrow current-source record. Its inspected unit assertions were not executed.
- **Record C:** only **Scoped human attention observations - 2026-10-30** in
  `maerg/IMPL.md`. Its source-derived summary is distinguished below from its
  explicitly implementation-agent-attributed execution claims.

**Procedural limitation:** an initial heading/snippet search also surfaced
unrelated `maerg/IMPL.md` headings and excerpts. They were excluded from this
comparison, but that exposure means this record cannot claim the requested
strictly exclusive-read boundary. A fresh comparison is necessary if that
boundary is required for the project review. This limitation is not evidence
of an implementation defect.

## Chronological comparison

### 1. Initial retention contradiction; later observation changes the finding

The design requires proposal identity to bind the unique run-log session, turn,
proposal ordinal and call ID, with no-clobber publication preventing reused call
IDs from replacing older JSON/Markdown evidence.

Observation A, **Concrete retention concern**, reports filenames derived only
from the call ID and replacement of both raw proposal files. That is a concrete
contradiction of this retention design at the initially observed state.
Attention history did not preserve the full original raw proposal.

Observation B, **Current behavior and limits — Identity, Session allocation,
Retention/publication**, subsequently reports session/turn/decision-index
identity combined with the call ID, exclusive log allocation and no-clobber
publication. Reusing the call ID alone therefore no longer identifies the same
proposal. Allocation has a finite collision allowance and fails rather than
overwrites; it is not an unlimited identity reservation.

Record C repeats this chronology. Its separate implementing-agent paragraph
claims the overwrite was reproduced before repair. That is attributed execution
evidence, not independent reproduction by either observer or this reviewer.

**Finding:** do not carry the initial overwrite finding forward as established
current behavior. The later narrow observation supplies different retention
behavior, but does not establish all failure-path retention properties.

### 2. Partial publication remains a concrete observed behavior

Observation B, **Retention/publication** and **Definite correctness concern**,
reports JSON publication before Markdown, without a pair-wide transaction or
rollback. JSON can remain without its companion after Markdown publication
fails; both files can remain after directory-sync failure. Repeating the same
identity can fail at existing JSON rather than complete the companion.
Observation A also describes nontransactional multi-file publication.

The intent requires Markdown companions, explicit persistence failures and
preservation of original causes/effects/recovery fences. It does not expressly
require pair-transactional publication. Thus partial publication is concrete;
calling it an unconditional violation of an atomic-pair guarantee would invent
such a guarantee. Failure-state companion availability and recovery remain
human-arbitration points. The observations do not demonstrate recovery of a
partially published pair or preservation of every original error in that path.

### 3. Explanation authorship is requested, not structurally guaranteed

`REQ-DECISION-SURFACING`, the contract and design request situation, design
impact, alternatives/tradeoffs, recommendation/rationale and missing evidence
from the existing agent response.

Observation A, **Authorship and presentation** and **Review companions**, reports
these prompt requests, agent-written explanations, redacted proposal JSON and
Markdown, and review companions. It reports nonempty/byte-bound validation, not
section completeness or factual validation. Record C expressly repeats that
section presence and factual quality are not mechanically established.

**Coverage limitation:** prompts support the intended authorship mechanism but
do not prove every response has all required substance. No observed deficient
response is supplied here; lack of structural validation alone is not proof of
one. Recommendation quality and calibrated uncertainty remain unestablished.

### 4. Decision and blocker presentation preserves separate authority

Observation A, **Decision lifecycle**, **Error coverage** and **Human response**,
reports briefs written before awaiting-decision disposition, agent explanation
when available, host-labelled diagnostics on failure, evidence references, and
choose/clarify/reject/resume instructions. Exact-task and continuous wrappers
include preflight and checkpoint errors, without an extra explanatory model
call. The presentation helper does not itself dispose tasks or call a model.

These observations support the scoped design mechanism. They do not prove every
possible blocker receives a persisted brief. A brief-write failure can prevent
the decision transition and leave an unresolved attempt; the observed fence
must not be treated as cleared because presentation failed.

The existing instructions require durable intent/queue edits, review and
`component accept` when intent changes, deliberate resumption, and explicit
recovery/disposition for unresolved attempts. The contract expressly provides
no new approval/clarification command. Manual response steps are therefore not
a missing new interface. Their presence does not establish enforcement of
future advisory-review gates or automatic decision propagation.

### 5. Safe publication can be unavailable before execution

Observation A, **Error coverage** and **Bounds, redaction and paths**, and Record
C report explicit unavailable-brief diagnostics when configuration/component
resolution, safe paths or persistence are unavailable.

The design expressly says unsafe/unavailable state directories fail visibly.
Consequently, an early failure without a safe configuration/path is not
equivalent to silently omitting an otherwise publishable brief. There is a
literal tension with “every” decision/blocker having a persisted brief; human
arbitration should clarify the exceptional delivery obligation, rather than
assume unsafe writes are required. The supplied records do not establish that
all combined publication errors preserve the original cause and fences.

### 6. Bounds, history and display have explicit limits

Observation A, **Bounds, redaction and paths**, reports literal configured-value
redaction, a 256 KiB Markdown limit, content-addressed attention history and
replaceable aliases. Oversized explanations/queue content can prevent a whole
brief. Neither general secret detection, Markdown sanitization nor resistance
to concurrent path replacement is established.

Observation A, **Collected explanation**, initially reports output-limited
attention text without a truncation marker. Observation B, **Output-limit
caveats**, later reports an explicit attention-header warning and pointers to
raw Markdown companions, plus advisory/historical-snapshot labels. This changes
the initial disclosure finding; it does not restore omitted text. B reports no
explicit recorder-local size bound or output-limit warning in the raw proposal
template and does not establish upstream completeness.

The design's “verbatim” wording warrants clarification against its bounded,
redacted presentation: an unchanged excerpt is not necessarily a complete
proposal. The records do not establish full-text availability in every case.
This is not evidence that the whole task response is unbounded, because upstream
limits were outside B's narrow observation.

The requirements and contract explicitly permit printing a specific `.md` path
until transcript integration. Observation A reports that pointer. Absence of
full transcript rendering is not a contradiction of that interim interface.
Actual rendered usability remains unobserved.

Attention history and snapshot labels support distinguishing retained evidence
from current state; Observation A, **Review companions**, says the final review
index checks file type, not source-digest freshness. Neither an existing report
nor its index establishes current-state correctness, review coverage or human
acceptance.

## Verification attribution and remaining coverage

Observation A's **Unexecuted assertions, not execution evidence** and B's
**Unit assertions — inspected, not executed** are descriptions of test source,
not passing test results. B's retention assertions do not cover partial-pair
recovery, synchronization behavior or Markdown-byte preservation on collision.

Record C's **Implementation-agent execution, separately attributed** claims:
pre-repair reproduction of missing decision Markdown and reused-ID overwrite;
a final 920-test pass with seven existing ignores; passing format, strict
all-target Clippy, release build and document validation; and an earlier
checkpoint-timing failure followed by isolated and final full-run passes.
These claims were neither independently executed nor independently verified
here. They do not demonstrate model explanation quality, rendering, race safety
or missing failure-path coverage. No example outside the permitted observations
is used as evidence.

## Human arbitration and disposition

1. Decide whether this diagnostic record may be used despite the disclosed
   exclusive-read boundary breach; it cannot substitute for a fresh strictly
   bounded comparison if one is required.
2. Resolve expectations for partially published JSON/Markdown pairs, same-identity
   recovery, and original-error/fence preservation before treating those failure
   paths as established.
3. Clarify complete versus bounded “verbatim” explanations and the obligation
   when no safe publication path exists. No intent was changed to resolve these
   tensions.
4. Require additional evidence only where needed for the decision: response
   completeness, combined persistence failures, freshness, rendering and
   concurrent path safety are coverage limitations, not proven defects.

**Bottom line:** the initial raw-evidence overwrite was a concrete contradiction
at the initially observed state; the later observation no longer supports it as
a current blocker. No blocking current implementation contradiction is
established by these permitted records. Partial publication is a concrete
current limitation requiring arbitration. The disclosed review-boundary breach
prevents claiming completion of the strictly exclusive-read review. Nothing
here accepts intent, approves an approach, clears a blocker, replays work or
certifies the implementation.
