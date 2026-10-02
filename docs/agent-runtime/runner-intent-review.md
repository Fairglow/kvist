# Source-blind comparison: runner hardening and runtime output bounds

## Scope, method and authority

This review compares the seven `RUN-REQ-*` clauses, directly affected older
runner promises, and runtime optional output bounds/stream integrity with the
two independently derived implementation records and supplied run evidence.
It is not approval, certification, an acceptance receipt, a security re-audit,
or a new certification of the older runtime.

Only the inputs below were read. No implementation source, test source,
manifests, Git/history/diffs, previous conversations, research, implementation
guides, queues, receipts or other reviewers' reports were consulted. References
to implementation behavior below are observations reported in `IMPL.md`, not
independent source inspection. No agents were invoked and no tests were rerun.
The stated clean-slate derivation provenance is supplied context, not something
this review can independently authenticate.

Requirements describe outcomes, contracts consumer promises, designs private
realization, and implementation records observed behavior. Tests are evidence,
not proof; operational journals are not canonical compliance evidence.
Missing source-only detail is an **evidence gap**, not automatically a violation.
Priorities indicate attention for human arbitration, not an acceptance decision.

### Exact input file paths

Global constraints:

- `/opt/proj/kvist/VISION.md`
- `/opt/proj/kvist/ARCHITECTURE.md`
- `/opt/proj/kvist/ROOT_CONTRACT.md`
- `/opt/proj/kvist/docs/standards.md`

Runner intent and observed record:

- `/opt/proj/kvist/agent_runner/REQUIREMENTS.md`
- `/opt/proj/kvist/agent_runner/CONTRACT.md`
- `/opt/proj/kvist/agent_runner/DESIGN.md`
- `/opt/proj/kvist/agent_runner/IMPL.md`

Runtime intent and observed record:

- `/opt/proj/kvist/agent_runtime/REQUIREMENTS.md`
- `/opt/proj/kvist/agent_runtime/CONTRACT.md`
- `/opt/proj/kvist/agent_runtime/DESIGN.md`
- `/opt/proj/kvist/agent_runtime/IMPL.md`

Actual supplied run evidence:

- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/runtime-tests.log`
- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/workspace-tests.log`
- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/live-qualification.log`

### What the logs establish

Summing the printed terminal group results gives:

| Input | Result groups | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: | ---: |
| `runtime-tests.log` | 24 | 388 | 0 | 3 |
| `workspace-tests.log` | 61 | 1105 | 0 | 3 |
| `live-qualification.log` | 1 | 3 | 0 | 0 |

These are separate supplied runs, not additive independent coverage. Negative
qualification and CLI diagnostics occur inside passing groups; they are not
reported suite failures. Logs do not identify invocation, named test cases,
reviewed revisions, installed runner/helper, endpoint/model, or qualification
environment. They therefore support “these captured groups passed,” not an
independently attributable Linux-isolation/provider qualification claim.
The records describe inspected fixtures, but quiet log groups cannot be mapped
to those fixtures with certainty from the permitted evidence.

## Priority findings

### F1 — P1 — observed mismatch: executor capture is not hard-bounded

**Intent:** `RUN-REQ-AUTHORITY`, the older bounded-output quality promise,
runner contract `execute`/required sandbox interface, and design
“Failure and recovery”/“Verification strategy” promise bounded process output,
including no unbounded buffering. `RUN-REQ-CONTEXT` separately bounds the
combined model-facing result.

**Observed:** Runner record “Tests inspected, run evidence and limitations”
describes local host/sandbox collectors with unbounded drain queues, per-stream
collection checks and unchecked final drains. This conflicts with the broader
capture/buffering promise: a bounded final model preview does not bound reader
queues or captured allocations.

**Boundary:** The record does establish a combined 8192-byte model-facing
preview and bounded native success JSON. Those narrower guarantees are not
discrepant. Actual installed sandbox resource enforcement is outside the
inspected record, so its contribution remains an evidence gap rather than a
proven isolation failure. The passing logs do not resolve the collector mismatch.

### F2 — P1 — evidence gap: executor cleanup/deadline

**Process-group gap:** Runner contract `execute` and “Cancellation” explicitly
promise sandbox process-group termination. The runner record says its sandbox
spawn does not itself create a fresh process group. That missing local mechanism
leaves the promise unestablished: the record does not show whether an external
installed boundary supplies an equivalent group/cleanup arrangement. Absence
of a locally created group is not alone proof of a violated termination promise.

**Evidence gap:** `RUN-REQ-BUDGET` and the hardening design require one prompt
deadline spanning cooperative effects, and `RUN-REQ-LIFECYCLE` requires joined
teardown. The record describes a cancellation watcher but also sandbox stdin
writes, final drains and waits without independent deadlines, with
retained-pipe/uncooperative-descendant cleanup unestablished. It does not show
that all built-in executor paths honor the token sufficiently to return/join.
This is not a claim that any supplied test hung.

The contract expressly allows cooperative injected collaborators; that caveat
does not establish cancellation of the built-in blocking paths. Worker
backpressure/retained-prompt-sender teardown is separately described as handled.
Installed-boundary cleanup and targeted retained-pipe/blocked-write evidence
would be needed to close the broader guarantee.

### F3 — P1 — observed mismatch: wire finish normalization defeats strict rejection

**Intent:** Runner hardening contract, `AgentRunner` behavioral guarantees and
design interactions reject inconsistent finish/tool combinations before
effects; tools require `ToolCalls`. `RUN-REQ-PROVIDERS` also requires finish
classification and deterministic integrity tests.

**Observed:** The runtime record “Direct transport” says missing/null finishes
are inferred and `stop` with tools becomes `ToolCalls`. The runner record says
validation acts on the resulting canonical turn. Consequently a wire
`stop`-plus-tools inconsistency is erased before runner validation and can look
like an acceptable tool proposal. The transport's “no tool execution” property
does not cure normalization at its consumer boundary.

Explicit length/filter/other finishes are reported as preserved, and the runner
rejects them; no discrepancy is observed for those cases. Inference of a missing
finish also leaves strict terminal-classification assurance incomplete, but is
not by itself classified here as a separate observed violation. Human
arbitration must determine the intended normalization boundary; intent should
not be rewritten merely to conceal the inconsistency.

### F4 — P1 — contradictory intent: global protected-artifact and shell rules

**Protected artifacts:** `ROOT_CONTRACT.md` says agents must not write queues,
intent, implementation records, approval state or canonical evidence.
`RUN-REQ-AUTHORITY`/runner contract instead deliberately expose an entirely
writable selected workspace and disclaim engine authorization/evidence APIs.
Architecture explicitly distinguishes this workspace shell from the protected
broker. There is no stated root-contract exception reconciling a workspace
that contains those protected artifacts.

**Shell invocation:** Global architecture/root constraints require external
commands without a shell. Runner requirements/design/contract explicitly
provide Bash `-c` for model-authored shell scripts. The observed record matches
that local design.

These are contradictions in intended authority, not evidence of an observed
unauthorized write, sandbox escape or unintended host fallback. “Does not mint
engine evidence” and “cannot modify evidence files” are distinct guarantees.
The human needs to arbitrate scope/exceptions explicitly before an unqualified
global-conformance statement is meaningful.

### F5 — P2 — observed mismatch and contradictory intent: toolchain honesty

**Observed mismatch:** Older runner requirements require a genuinely usable
sandbox interpreter and startup failure for unavailable forced/`on` profiles.
The runner record “Tools, filesystem effects and execution authority” says the
probe accepts regular candidates under mounted roots without checking
executable mode or running them. A non-executable candidate can satisfy that
probe without satisfying the advertised ability to run. Full companion-tool
availability is also not established; interpreter gating is not proof of
every toolkit item.

**Contradictory intent:** The contract calls `ToolRegistry::new`'s unconditional
generic-plus-Python advertisement “honest,” while its broader profile promises
and requirements demand availability-gated advertisement. That public
constructor's guarantee is unresolved independently of the production
`resolve` path. `resolve`'s mounted-root/rustup exclusion and ordinary
on/auto/off gating are described consistently; the finding does not negate them.

### F6 — P2 — observed mismatch: cadence promises tokens, observation tracks bytes

Runner contract `Config` describes an inter-token watchdog: a turn producing no
token for the configured interval after its first token is stalled/retried.
Runtime record “Direct transport” instead reports HTTP/body-byte progress,
including comments/other bytes, and differing framing branches with equivalent
watchdog behavior unestablished. A heartbeat/comment stream may therefore
postpone a token-stall diagnosis despite no new decoded token.

This coupled older behavior affects retry expectations, not the new
output-bound wire mapping. An overall prompt/attempt deadline can still stop
the request; that does not satisfy the narrower inter-token promise.

### F7 — P2 — observed mismatch: configuration read bounds rely on pre-read metadata

Runner quality constraints require bounded untrusted configuration.
The runner record “Package, entry points and configuration” reports checking
regular/non-link metadata and 64-KiB length before `read_to_string`, rather than
a byte-bounded read. Pre-open/pre-read length alone cannot enforce the bound
against concurrent growth. This concerns the reported configuration ingestion
path; it is not an inference that the separately bounded history reader has
the same defect.

### F8 — P2 — evidence gap: specific lifecycle/history guarantees and acceptance gates

The runner record describes history limits of 5 MiB/4096 directory entries but
does not establish all of `RUN-REQ-LIFECYCLE`'s required details: links in every
ancestor, regular descriptor validation, pre-allocation/growth bounds and
diagnostics for unusable entries. No definite violation is inferred.
Similarly, the record mentions reasoning collapse and paired interrupted
results, but does not describe enough observations to establish preservation
of every non-reasoning row or valid subsequent-prompt reuse after cancelled
multi-call turns. Reported source-local tests are supporting descriptions,
not individually identifiable actual run results.

The supplied inputs do not establish test-before-production ordering,
format/lint/build gates, the separate security audit, or attributable opt-in
native/live qualification. These are stated acceptance obligations. Source,
history, audit reports and other additional inputs were intentionally excluded,
so this is an evidence restriction, not a claim those activities did not occur.

### F9 — P2 — observed mismatch: older runtime trajectory input remains unbounded

Runtime design lists a bounded recorded trajectory stream and says parsed data
has explicit bounds. Runtime record “CLI, grammar, hashes and trajectories”
instead describes replay without file/line/event-count bounds, no-follow or
private-mode enforcement. This is an important coupled pre-existing quality
discrepancy where users might confuse the runtime replay facility with the new
private runner journal.

The records explicitly say the runner journal and runtime trajectory formats
are incompatible and no conversion exists. No interoperability was promised by
the reviewed hardening extension, so that difference is **no discrepancy
observed**, not a request to introduce automatic replay. Neither format is
canonical engine evidence.

## Clause-by-clause bounded comparison

| Scope | Classification and conclusion |
| --- | --- |
| `RUN-REQ-AUTHORITY` | **No discrepancy observed** for sandbox default, no host fallback, explicit interactive unconfined mode, headless host/log rejection, scope labels/instructions and absence of engine approval/promotion APIs. The records support private operational provenance, not authorization proof. Global intent conflicts are F4; process/output limits are F1/F2. |
| `RUN-REQ-CONTEXT` | **No discrepancy observed** for full canonical request accounting, output reserve propagation, pre-I/O irreducible rejection, complete call/result grouping, exact systems/current goal retention, lossy labelled summaries and combined result preview. The byte-based estimate is expressly heuristic, so lack of a tokenizer guarantee is not a violation. Legacy compact helpers are documented as diagnostic, not the send path. |
| `RUN-REQ-LIFECYCLE` | **No discrepancy observed** for fallible recording, synchronized pre-effect dispatch, recording failure stopping effects, fresh answers/dispositions, unknown interrupted effects/no replay, private no-clobber files and owned worker shutdown/backpressure handling. Built-in cleanup and selected history/UI/follow-up guarantees remain F2/F8. Null mutation state is honest uncertainty, not missing effect certification. |
| `RUN-REQ-BUDGET` | **No discrepancy observed** for attempt charges, clipped deadlines/backoff, cancellable short retry waits, argument-hash correction/circuit breaker and injected turn limits 1..=50. Injected collaborator cancellation is explicitly cooperative. Built-in deadline completeness is F2; cancelled follow-up pairing evidence is F8. No requirement to prove filesystem progress or add temperature jitter is imposed by this clause. |
| `RUN-REQ-TOOLS` | **No discrepancy observed** for closed typed requests, bounded deterministic pages, exact-single-occurrence digest-bound edits, overlap/stale rejection, unrelated bytes/CRLF/no-final-newline preservation, descriptor-relative non-link mutation and same executor/helper boundary. Host-generated private outside-workspace staging with ordinary-return/error RAII cleanup is recorded. Arbitrary-writer transactional compare-and-swap is explicitly excluded; post-rename sync failure can report failure after effects. Actual installed isolation is not established by unattributed logs. |
| `RUN-REQ-HEADLESS` | **No discrepancy observed** for same-loop terminal-free execution, schema-one ordered NDJSON, stderr diagnostics, dispositions/nonzero unsuccessful status and required private outside-workspace logs. Plain successful answers/error descriptions escape controls; JSON/private transcripts retain originals. The contract explicitly allows startup/output failure to prevent a terminal envelope. |
| `RUN-REQ-PROVIDERS` | **No discrepancy observed** for retaining both providers and mapping reserves to their wire bounds. Records describe fragmented tools, failure/cancellation/context/retry fixtures and live opt-in scenarios. Finish normalization is F3; cadence is F6; named executed coverage/native qualification attribution is F8. |
| Runtime `max_output_tokens` | **No discrepancy observed**: optional canonical bound 1..=1,048,576, rejection before connecting, llama-server `max_tokens`, Ollama `options.num_predict`, and omission preserving defaults. Transport never executes tools. Provider receipt of a bound is not proof of tokenizer accounting, semantic correctness or exact real-provider enforcement. |
| Runtime stream integrity | **No discrepancy observed** for required OpenAI `[DONE]`/Ollama `done:true`, rejection of later records, bounded fragmented assembly, duplicate supplied-ID rejection and preserved explicit failure finishes. **Observed mismatch/evidence gap** for normalization in F3. OpenAI streaming traversal of multiple choices without separate choice selection is recorded; isolation of independent choice text/tool fragments is an **evidence gap**, not a proven malformed result from these logs. |

## Conclusions and actual blockers

The source-blind review itself completed without an access or execution blocker.
The records substantiate substantial hardening, especially context preflight,
paired-result loop policy, private pre-effect recording, native exact file
operations, headless restrictions and optional provider output bounds. The
supplied captured test groups report no failures.

An unqualified compliance conclusion is blocked by concrete collector and
finish-normalization discrepancies (F1/F3), process-group/deadline cleanup
uncertainty (F2), and unresolved global/local
authority contradictions (F4). Toolchain/cadence/configuration discrepancies
(F5–F7), lifecycle and qualification evidence gaps (F8), and the pre-existing
trajectory issue (F9) remain visible for explicit human disposition.

No intent was altered to remove a finding, no implementation fix is authorized
by this report alone, and no approval/certification receipt is produced. Missing
evidence may be supplied independently; intent conflicts require deliberate
arbitration. Passing tests or private operational records must not substitute
for that disposition or certify the remainder of the older runtime.

## Independent current-snapshot follow-up — 2026-10-02

### Method, inputs and snapshot linkage

The original F1–F9 and their initial conclusions above remain unchanged as
historical findings. This appended comparison derives current statuses from
current intent, newly regenerated observations and the newly permitted logs.
It does not adopt implementation claims or another reviewer's dispositions.
“Resolved” below means the particular discrepancy is no longer observed in
this bounded comparison, not formal acceptance or certification.

The follow-up reread these exact global and component inputs:

- `/opt/proj/kvist/VISION.md`
- `/opt/proj/kvist/ARCHITECTURE.md`
- `/opt/proj/kvist/ROOT_CONTRACT.md`
- `/opt/proj/kvist/docs/standards.md`
- `/opt/proj/kvist/agent_runner/REQUIREMENTS.md`
- `/opt/proj/kvist/agent_runner/CONTRACT.md`
- `/opt/proj/kvist/agent_runner/DESIGN.md`
- `/opt/proj/kvist/agent_runner/IMPL.md`
- `/opt/proj/kvist/agent_runtime/REQUIREMENTS.md`
- `/opt/proj/kvist/agent_runtime/CONTRACT.md`
- `/opt/proj/kvist/agent_runtime/DESIGN.md`
- `/opt/proj/kvist/agent_runtime/IMPL.md`

The only run inputs for this follow-up were:

- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/runtime-qualified-tests.log`
- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/qualified-quality-workspace.log`
- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/subprocess-qualified-repeat.log`
- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/live-remediation-final-qualification.log`
- `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/runtime-final-tests.log`

No production/test source, manifest, queue, Git/history/diff, engineering/
security/research report or other agent output was accessed. No tests or
qualification were run and no intent was changed. Hashes naming excluded
files were compared only as data printed in permitted logs/records; those
files were not opened. Supplied record-generation independence is provenance,
not independently authenticated process evidence.

The runner record reports independent agreement of all 36 observed
source/test/shell-fixture/manifest identities with `runtime-qualified-tests.log`.
Its current process identity is
`d0aacea446a4b2702c8a4ee84fc362e0e76d364c4d3a723b90f6f55e465bf97e`;
the current subprocess-test identity is
`221784293daa1b53d3f5fb106fbce51510456e12dd9306a9bfd815d40dbb838a`.
The runtime record reports matching all 27 of its observed file identities
with `runtime-final-tests.log`. Independent comparison of the two permitted
logs finds all 27 runtime identities identical, supporting use of that
earlier runtime evidence without treating its older runner snapshot as current.

Calculated hashes of the two record-provenance logs match their records:

- `runtime-qualified-tests.log`:
  `3e08c557585acddea10a02ff0094a7251f05b5f996fc2e8bdad135a3b0b821c9`
- `runtime-final-tests.log`:
  `724b25f2e4a619435ae1aedf8278c6a9eba23ff2a32882d78a64d8ef2ef9e15b`

Hash agreement binds descriptive observations to supplied snapshot data; it
does not independently authenticate execution or turn tests into proof.

### Current actual-run evidence

Both component runs name
`cargo test --locked -p agent-runner -p agent-runtime`, using
`rustc 1.99.0 (b940084d7 2026-09-28)` and
`cargo 1.99.0 (5f94df478 2026-08-27)`.

| Log | UTC interval on 2026-10-02 | Printed result |
| --- | --- | --- |
| `runtime-qualified-tests.log` | 00:26:37–00:26:46 | 297 runner + 140 runtime passed; zero failed; three live cases ignored |
| `runtime-final-tests.log` | 00:08:04–00:08:15 | 140 runtime passed; zero failed; other package results do not qualify the current runner |
| `qualified-quality-workspace.log` | 00:26:37–00:27:08 | 1154 workspace tests passed; zero failed; three ignored |
| `subprocess-qualified-repeat.log` | 00:26:37–00:26:49 | 20 runs of 26 named cases: 520 passes, zero failures/ignored |
| `live-remediation-final-qualification.log` | 00:20:38–00:20:41 | Three named opt-in cases passed; zero failed/ignored |

Repeated suites and overlapping workspace/component runs are not additive
independent requirement coverage. Subprocess fixtures use actual host processes
or a fake runner; their passes do not establish namespace/network isolation.

The quality log records `cargo fmt --all --check` without printed errors,
`cargo clippy --locked -p agent-runner -p agent-runtime --all-targets -- -D warnings`
and `cargo build --locked -p agent-runner -p agent-runtime -p kvist` with finished
profiles. It then records
`GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=safe.bareRepository GIT_CONFIG_VALUE_0=all cargo test --locked --workspace`.
These are substantially better actual gate records than the initial quiet
logs; they do not establish test-before-implementation ordering or a security
audit.

The live log explicitly opts into the three named ignored-by-default cases,
selecting endpoint `http://127.0.0.1:9931`, model
`Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL`, and reported effort `none`. It records:

- Installed runner SHA-256:
  `db631ad7a514a31c0b57f74815ef612f224b25ff8b662affb0a64d9af7fda1b3`
- Bubblewrap SHA-256:
  `d9498f8b15b1c69e09791badee317d56f33abd359a306d6e96534f136381ad74`
- File-helper SHA-256:
  `c709df50351fce9ef7adc7323b0b75f2ecc7663d9a1e2345f8fa4cdad3c0d4f3`

`live_llama_reads_edits_and_verifies_inside_the_real_sandbox`,
`live_llama_streams_a_bounded_answer_without_tools`, and
`real_sandbox_confines_native_files_and_denies_host_network` are individually
reported passing. This supports those concrete installed-boundary/live trials,
not every sandbox limit or every provider variant. The live run precedes the
current runner-qualified run and has no source inventory or test-harness binary
digest linking its caller to the final process snapshot. Its external binary
identities are explicit; final-snapshot caller linkage remains an evidence gap.
The filename “final” is not that missing linkage.

### Independent statuses for F1–F9

#### F1 — resolved in the bounded current comparison

**Classification: no discrepancy observed for the original capture defect.**
Runner record “Shared subprocess pump and cancellation recovery” describes
one nonblocking interleaved stdin/stdout/stderr pump, no drain-reader queues,
shared combined capacity on every drain, discarded excess marking overflow,
and exact-cap success. Current `RUN-REQ-LIFECYCLE`, contract `execute` and
design require those bounds explicitly.

Named host/sandbox rapid-exit and final-drain cap cases pass, along with
`ordinary_success_and_exact_limit_are_not_overflow` and
`native_helper_near_7000_byte_output_is_usable_through_host_executor`.
Their 20 repeated suite runs strengthen evidence against the original
unchecked-drain issue. This does not claim an aggregate memory quota for
conversation, UI or journals, or independently prove backend resource limits.

#### F2 — substantially resolved subprocess paths; partial broader deadline assurance

**Classification: no discrepancy observed for the repaired subprocess
mechanisms; evidence gap for a universal whole-prompt upper bound.**
The current record describes fresh owned groups, nonblocking writes/reads,
250-ms retained-pipe failure, one-second bounded direct-child reap, owned
cleanup on every pump exit, and no detached readers.

It additionally records `WNOWAIT` exit observation pinning the direct PID,
independent original-group signalling and direct-child kill even when group
signalling succeeds or returns ESRCH, and no following a moved child into the
supervisor group. Six named moved-direct-child cancellation/timeout/overflow
cases, each with empty/retained original-group variants, pass in the current
run and repeated suite. Blocked stdin, simultaneous request/output, retained
descriptors, setup failure cleanup and independent calls also have named passes.

This resolves the original absent-owned-group/blocked-pipe evidence problem
for the described paths. Escaped pipe-holder fixture cleanup is not evidence
of universal production escaped-descendant termination, which current contract
and design expressly exclude.

The current record still reports an unbounded-count/depth/time workspace
symlink scan without internal cancellation, unbounded whole-file identity
reads, and cooperative recording/filesystem/kernel work. Consequently the
strong reading of `RUN-REQ-BUDGET` as an unconditional wall-time bound across
all built-in pre-execution work is not established. Cooperative injected
collaborators and uninterruptible kernel exclusions are explicit; they should
not be mistaken for cancellation coverage of the built-in scan.

#### F3 — resolved for llama-server finish preservation/rejection

**Classification: no discrepancy observed for the original normalization
mismatch.** Current runtime intent expressly preserves llama `stop` with
tools, unknown missing/null terminal reasons, and malformed-reason failures.
The new runtime record describes matching unary/stream parsing. Current runner
record describes rejecting those inconsistent/unknown turns before effects.

Named explicit-stop, missing/null, terminal-marker-only, malformed-reason,
preserved non-success and later-null-delta cases pass. Crucially,
`llama_wire_stop_or_missing_finish_with_tools_never_dispatches` passes through
the production transport/runner integration. Current intent deliberately
distinguishes Ollama native stop/absent-reason conventions; the record and named
Ollama fixture match that explicit distinction. Transport intent events remain
provisional and are not dispatch.

The separate multi-choice integrity gap remains: runtime record says all
OpenAI streaming choices accumulate into one turn. No current permitted
observation or named case establishes separation/rejection of independent
choice streams. This is an evidence gap, not a demonstrated failing case.

#### F4 — open for explicit human scope arbitration

**Classification: contradictory intent remains.** Current root rules still
prohibit agent writes to intent/queues/records/approval/canonical evidence;
runner intent still gives the selected workspace full write authority and
disclaims the protected broker. Global no-shell wording remains alongside
the locally explicit Bash `-c` tool. No permitted input supplies an exception
or arbitration reconciling these scopes.

Actual confinement/network-denial trials do not protect every intent/evidence
file within the writable workspace and cannot resolve a textual authority
conflict. No prohibited write or sandbox escape is alleged by this finding.

#### F5 — original defects resolved; companion-tool guarantee remains bounded

**Classification: no discrepancy observed for non-executable candidates and
unconditional Python advertisement.** Current contract/record agree that the
pure constructor advertises only Generic and language resolution checks
executable mounted-root candidates excluding rustup proxies.
`pure_registry_does_not_claim_unprobed_language_tools` and
`nonexecutable_candidates_do_not_enable_a_language_profile` pass.

Current design explicitly does not promise every companion tool and the record
does not qualify pip or both cargo/rustc. Thus full-toolkit/buildability assurance
remains an evidence gap where older prose about “package and build tools”
is read more broadly than interpreter availability. The original constructor
contradiction is actually reconciled in current intent, unlike F4.

#### F6 — resolved for semantic cadence

**Classification: no discrepancy observed for the original byte-progress
mismatch.** Current runtime intent separates header/first-body I/O timers from
decoded generation cadence. The record describes cadence starting/resetting
only for nonempty text/reasoning or tool-generation progress, not comments,
framing, control/usage records, empty fragments or undecoded bytes.

The named heartbeat/control/usage framing, tool-fragment/reasoning,
slow-fragmented-initial-record and caller-deadline/cancellation cases pass
in both identity-matched component logs. A first-body timer is still not a
decoded-first-token guarantee; current runtime contract says so explicitly.
Synchronous callbacks remain cooperative, not forcibly preempted.

#### F7 — resolved for configuration read-time byte bounds

**Classification: no discrepancy observed for the original allocation/growth
defect.** Current contract specifies a held regular non-link descriptor and
64-KiB read-time bound. Runner record describes no-follow/nonblocking leaf
open, regular descriptor checks and growth-safe reads.
`configuration_stream_growth_cannot_exceed_the_byte_bound` passes.

Configuration ancestor links are not rejected by descriptor walking, according
to the record. This is a stated limitation, not evidence that the differently
specified history reader violates its all-ancestor policy.

#### F8 — partial: most named evidence improved, limited obligations still unestablished

**Classification: no discrepancy observed for the now-described history/
worker/collapse mechanisms; remaining evidence gaps.** Current record describes
5-MiB history/growth bounds, no-follow regular reads, 4096-entry listing limits
and unusable-entry diagnostics. Named link-target/ancestor and oversize history
cases pass. Named collapse-preserves-answers/tools/notices and worker-drop with
full-event-queue/retained-prompt-sender cases pass, improving the initial
unattributed coverage.

Interrupted remaining-call pairing is described, and ordinary multicall context
grouping/follow-up tests pass. A specifically cancelled multi-call turn reused
by a subsequent prompt is not identifiable among the supplied named cases;
that exact `RUN-REQ-BUDGET` scenario remains an evidence gap, not an inferred
violation.

Formatting/lint/build and named native/live execution now have actual evidence
as bounded above. Test-before-production ordering, a separate security audit
and formal human acceptance remain outside the allowed evidence. No audit
report was read or inferred. Live final-caller snapshot linkage remains limited.

#### F9 — open older-runtime replay discrepancy, separate from runner hardening

**Classification: observed mismatch remains in the older trajectory scope.**
Current runtime design still calls its trajectory stream bounded and says
parsed data has explicit bounds. Its new record reports raw caller-supplied
append records and replay without file/line/event-count bounds or leaf-link
rejection/privacy/durability enforcement. The three passing trajectory cases
do not establish those absent limits.

This is the older `agent-run replay`/public trajectory facility, not runner
history replay or its private operational journal. The record does not show
the runner using this facility; lack of journal-format conversion is not a
hardening failure and automatic replay remains deliberately absent. F9 must
remain visible for the older-runtime quality posture, but is not represented
as an unresolved implementation dependency blocking every new `RUN-REQ-*`
mechanism.

### Additional current coupled findings

These are current record-to-intent comparisons, not conclusions from source
inspection or a new security audit.

- **C1 — P1, observed mismatch:** Runner record “Terminal worker, rendering
  and display limitations” reports an out-of-range Markdown-fence slice when
  a recognized streamed opener lacks its first newline. This conflicts with
  the recoverable-untrusted-input/no-panic requirement and affects ordinary
  streaming presentation. It is source-observed by the independent documenter,
  not personally reproduced. The passing fragmented-fence case does not
  establish the separate newline-absent edge. This record-reported defect
  remains open.
- **C2 — P2, observed mismatch:** The same record says replay Down/PageDown
  handlers clamp without advancing and large reconstructed/replay row counts
  can truncate through `u16`. The contract promises all wrapped replay content
  remains reachable. Existing wrapping tests do not establish navigation for
  this path; current replay reachability remains discrepant. Uniform Unicode
  cell-width assurance is additionally an evidence gap, not certified by
  ordinary wrapping cases.
- **C3 — P2, observed mismatch:** Current runner record says a host-turn
  value without host execution is parsed but ignored interactively. Current
  CLI contract says `--host-turns` requires `--allow-host-execution`.
  A reassuring parser test name does not override that contrary observed
  behavior. The mismatch is fail-safe with respect to selecting host authority,
  but still violates the stated diagnostic/input semantics.
- **C4 — older-runtime quality discrepancy, outside new bound mapping:**
  Runtime requirements/design forbid unsafe Rust, while the regenerated record
  explicitly reports two unsafe signal-installation expressions. This is an
  observed mismatch with that older constraint, not evidence of an exploitable
  signal defect or failed output-bound remediation. Older supervisor early
  cleanup and signal integration limits likewise must not be swept into a
  blanket new-runtime certification.

### Bounded current conclusion

No discrepancy is observed in the optional output-token bound: both wire
mappings, absence-preserves-defaults and invalid-bound-before-connect behavior
match current intent and named fixtures. Complete-request context/reserve
preflight, exact bounded native tools, pre-effect private recording, headless
restrictions and explicit provisional stream handling remain supported by
the current records and named tests within their documented boundaries.

F1/F3/F6/F7 and the two concrete F5 defects are resolved in this comparison.
F2/F8 have materially improved evidence but retain narrower assurance gaps.
F4 is still a human-arbitration issue; F9 remains a separate older replay issue.
Current C1–C3 and the older C4 discrepancy must remain visible, rather than
treating passing suites as proof that every promise holds.

The follow-up itself has no access/execution blocker. Unqualified compliance
or formal acceptance is not established: unresolved intent, current reported
defects and the stated evidence restrictions remain. This section neither
changes intent to hide discrepancies nor authorizes fixes, approves results,
mints a receipt or certifies the older runtime.

## Final supplied-snapshot comparison — 2026-10-02

### Review boundary and exact provenance

This is an appended source-blind comparison. Initial F1–F9 and the preceding
C1–C4 findings/statuses are preserved unchanged as historical observations.
Current statuses below are independently derived from current intent, current
component records and the permitted supplied executions. Completion of this
comparison is not approval, formal acceptance, security assurance or a receipt.

The intent/record inputs were exactly:

- `/opt/proj/kvist/VISION.md`
- `/opt/proj/kvist/ARCHITECTURE.md`
- `/opt/proj/kvist/ROOT_CONTRACT.md`
- `/opt/proj/kvist/docs/standards.md`
- `/opt/proj/kvist/agent_runner/REQUIREMENTS.md`
- `/opt/proj/kvist/agent_runner/CONTRACT.md`
- `/opt/proj/kvist/agent_runner/DESIGN.md`
- `/opt/proj/kvist/agent_runner/IMPL.md`
- `/opt/proj/kvist/agent_runtime/REQUIREMENTS.md`
- `/opt/proj/kvist/agent_runtime/CONTRACT.md`
- `/opt/proj/kvist/agent_runtime/DESIGN.md`
- `/opt/proj/kvist/agent_runtime/IMPL.md`

The existing report was read only for append/history preservation. No source,
test source, manifest, queue, Git/history/diff, other report/review/research or
other agent output was accessed. No tests, builds, qualification, inspection
commands or agents were run. Log parsing and hashing read only permitted files.
Structural command results below are supplied executions, not personally
executed commands.

Calculated record identities:

| Record | SHA-256 of current record bytes |
| --- | --- |
| `/opt/proj/kvist/agent_runner/IMPL.md` | `a16117b4f014b81839ecdebd02308543be02532a70f5c46408fe48922af4b850` |
| `/opt/proj/kvist/agent_runtime/IMPL.md` | `5dd818a0e47cc63785bacfffaaf3820c9bdbf55ed1c8f7f01d075fb899da52b9` |

Both have the `kvist-implementation-record-version: 1` envelope. Runner
observations were regenerated using the current component source-only scope;
runtime observations retain their earlier source-only basis and
`runtime-final-tests.log` provenance. This reviewer did not inspect or certify
those derivation contexts.

Actual evidence paths and independently calculated log identities:

| Exact log path | SHA-256 |
| --- | --- |
| `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/runtime-complete-tests.log` | `b3bfc4ece52b59a4b9d54673bbc07d633e9161670682574055214b0b2e382b40` |
| `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/complete-quality-workspace.log` | `5389f10ed6a0e48326c77dfeef8b2291c1b0f2776a7f1bb934e123f288dd517a` |
| `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/live-complete-qualification.log` | `befb5cb76580eaa64d5ab0c3b544bbbd6bd9bbd8b1062d3d5220c6c4594a836f` |
| `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/subprocess-complete-repeat.log` | `3c3ccd7e4796f94d70674a2060761592b5afb46fa869de459bb5596bceece782` |
| `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/complete-artifact-inspection.log` | `408f4e3e68f289686d3fef72ef5c1e598d25fdc7f9989c554e6c04575a3e8bbe` |
| `/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/runtime-final-tests.log` | `724b25f2e4a619435ae1aedf8278c6a9eba23ff2a32882d78a64d8ef2ef9e15b` |

Comparison of printed inventories, without opening any named source/manifest,
finds all 36 runner record entries equal to both complete component/live logs.
Their independently reconstructed sorted-inventory digest is
`377e8990925b7e90490eb904f95c45f3dbabbbc0ae9698113854261afeaa1473`,
matching the new record. All 27 runtime record identities equal both complete
logs and the earlier runtime-final log. The repeated subprocess log's five
process/executor/sandbox/test/fixture identities also equal the complete
snapshot inventory. These establish internal snapshot linkage, not independent
authentication of execution or excluded infrastructure.

### Exact supplied results and current live-caller linkage

All intervals below are UTC on 2026-10-02. Component/live/quality logs report
`rustc 1.99.0 (b940084d7 2026-09-28)` and
`cargo 1.99.0 (5f94df478 2026-08-27)`.

| Log | Interval | Result groups | Passed | Failed | Ignored |
| --- | --- | ---: | ---: | ---: | ---: |
| `runtime-complete-tests.log` | 01:08:47–01:08:58 | 26 | 452 | 0 | 3 |
| `complete-quality-workspace.log` | 01:08:47–01:09:24 | 63 | 1169 | 0 | 3 |
| `live-complete-qualification.log` | 01:08:47–01:08:51 | 1 | 3 | 0 | 0 |
| `subprocess-complete-repeat.log` | 01:12:01–01:12:13 | 20 | 520 | 0 | 0 |
| `runtime-final-tests.log` | 00:08:04–00:08:15 | 26 | 431 | 0 | 3 |

The current component command is
`cargo test --locked -p agent-runner -p agent-runtime`. Its 452 passes comprise
312 runner and 140 runtime tests. The older 431-pass combined run is not
current-runner evidence; only its identity-matched 140 runtime passes support
the retained runtime record. The live command executes the three otherwise
ignored cases, giving 315 runner/455 combined passes across those two
commands, while preserving the component command's actual three ignored
results. Workspace and repeated-suite results overlap existing coverage;
they must not be counted as additional distinct requirement tests.

The quality log records the same exact formatter, Clippy, build and workspace
test commands as the preceding follow-up: formatter check without printed
errors, finished Clippy/build profiles, and all printed workspace groups
passing. No MSRV-1.95 or real-terminal run is inferred from Rust 1.99 fixtures.

The current live log records a `--no-run` build, the 36 matching runner source
identities, executable hashes, then execution of that same named target with
`--ignored --test-threads=1`. In contrast to the previous live log, this
supplies explicit final caller linkage:

| Logged executable | SHA-256 |
| --- | --- |
| `/opt/target/debug/deps/live_llama-91e0887ff0465c78` | `b1eb241879e75cf9931403b586732cd89892a87411b19aac89f11488ace50c1b` |
| `/opt/target/debug/agent-runner-file-tool` | `e3bec788c17915dce58f5cc2b6b72bb87d6cda814c755d31840b21f6a02ed76b` |
| `/opt/target/release/kvist-sandbox-runner` | `db631ad7a514a31c0b57f74815ef612f224b25ff8b662affb0a64d9af7fda1b3` |
| `/usr/bin/bwrap` | `d9498f8b15b1c69e09791badee317d56f33abd359a306d6e96534f136381ad74` |

Endpoint/model are `http://127.0.0.1:9931` /
`Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL`, with reported effort `none`. The three
named read/edit/verify, bounded-answer and native-confinement/network-denial
cases pass. The record describes their finite assertions: one outside sentinel
and loopback connection denial, a cap-64 text answer, and an exact CRLF/
missing-newline-preserving edit with read verification and no workspace staging.
They do not prove every path, network protocol, resource limit, model variant
or semantic output constraint. This reviewer did not open executable bytes.

### Final statuses of the preserved findings

**F1 — resolved within original capture scope; no discrepancy observed.**
The new record still describes shared bounded capture/nonblocking intermediate
I/O and bounded final drains. Rapid-exit, final-drain, exact-cap, failed-output
and near-7000-byte helper cases pass. Twenty repeated 26-case subprocess runs
match the current supervision identities. Aggregate UI/journal memory and
external resource enforcement remain outside this narrower resolution.

**F2 — original subprocess and built-in preparation gaps resolved within the
documented cooperative boundary.** Current contract/design specify streaming
identity hashing in 64-KiB blocks with a 256-MiB/file growth-safe bound; scan
quotas of 1,000,000 entries, depth 128 and 32 MiB charged retained path bytes;
and one 30-second cooperative preparation guard. The record describes passing
the prompt's actual token to private cancellable request construction,
checking token/time during reads and each scan directory/entry, and
propagating enumeration/read-link errors.

Six named preparation cases pass:

- `identity_read_rejects_more_than_256_mib_before_request_dispatch`
- `cancellation_during_identity_read_prevents_request_dispatch`
- `workspace_scan_rejects_directory_depth_over_128`
- `scope_entry_quota_accepts_exactly_one_million`
- `scope_path_quota_accepts_exact_bound_then_rejects_growth`
- `expired_preflight_and_cancellation_stop_scope_inspection`

These support the formerly absent cooperative checks and quotas. Exact quota
counter fixtures are not million-entry full-tree stress evidence. Path charging
is not whole-program allocation accounting; identity bounds are per file.
Public request construction lacks a caller token, while production supplies
one. Arbitrary collaborators, recording synchronization and kernel/filesystem
calls remain non-preemptible. Thus no unconditional elapsed-time/reap guarantee
is claimed, but the former missing built-in mechanisms are no longer observed.
Owned-group/direct-child cleanup and retained-pipe tests remain supported.

**F3 — resolved for the original finish-normalization mismatch.** The retained,
identity-matched runtime record preserves llama explicit stop/unknown/malformed
reasons, and the current runner rejects invalid terminal/tool combinations.
`llama_wire_stop_or_missing_finish_with_tools_never_dispatches` passes again.
Explicitly different native Ollama completion conventions still match current
intent. The separate streaming multi-choice integrity gap remains open.

**F4 — open, contradictory intent.** Global protected-artifact/no-shell rules
and local fully writable workspace/Bash tool still lack a reconciled exception
in the permitted intent. Trials, structural validity and completed review
cannot arbitrate that authority question. No actual prohibited write or
isolation escape is alleged.

**F5 — concrete candidate/constructor defects remain resolved.** Pure registry
Generic-only and executable mounted-root profile checking match current
intent/record and named passing fixtures. Full companion-tool/buildability
qualification remains unestablished; current design expressly avoids promising
every companion.

**F6 — resolved for semantic cadence.** Current runtime record/intent and
identity-matched named heartbeat/framing/tool-fragment/reasoning fixtures agree.
HTTP header/first-body bounds are not decoded-first-token guarantees, and
synchronous callback overrun is checked after return, not forcibly prevented.

**F7 — resolved for configuration growth/allocation bounds.** The record's
held no-follow regular leaf/growth-safe read matches the current 64-KiB
contract and named passing growth test. This does not add an all-ancestor
configuration-link promise.

**F8 — cancelled-multicall and final live-caller evidence gaps now resolved;
acceptance-process evidence remains partial.** The current record describes
interrupted remaining calls receiving paired results and valid follow-up
handling. The specifically required
`cancelled_multicall_turn_is_valid_for_a_subsequent_prompt` now passes; ordinary
multicall grouping is no longer the only named supporting case. Current live
build/hash/run linkage closes the prior caller-identity gap. History ancestor/
growth bounds, reasoning-collapse preservation and retained-sender/full-event
worker teardown continue to have record support and named passes.

No test-before-production ordering, separate security-audit result, human
arbitration or acceptance is established by these allowed inputs. A finite live
trial is not universal provider/isolation qualification, and operational
journals remain noncanonical evidence.

**F9 — open observed mismatch in the separate older runtime trajectory scope.**
The retained record still describes append/replay without file/line/event
bounds, leaf-link rejection/privacy or synchronized durability, versus the
runtime design's bounded-stream/general-input promises. Its passing replay
cases do not supply those mechanisms. This is not runner diagnostic history,
not the new journal and not an implemented automatic-resume dependency.

**C1 — resolved for the reported incomplete-fence panic edge.** Current runner
record describes checked missing-newline access leaving the opener pending
rather than out-of-range slicing. Named
`incomplete_markdown_fence_openers_wait_without_panicking` and
`markdown_fence_block_length_includes_leading_newlines` pass, alongside the
fragmented-fence regression. This establishes bounded resolution of that edge,
not absence of every renderer panic or real-terminal failure.

**C2 — concrete navigation/large-offset defects resolved; universal layout
assurance remains partial.** Current record describes advancing Down/PageDown,
`usize` history/replay offsets and manually selected physical-row viewports
without narrowing large offsets to `u16`.
`replay_keys_advance_and_keep_large_row_offsets` and
`replay_rows_beyond_u16_remain_visible` pass. Plain wrapping additionally has
passing order/indentation/grapheme/cell-width regressions.

However, the record explicitly excludes guaranteed fit for an overwide glyph/
indentation and distinguishes scalar-based Markdown/code/table rendering
from the improved plain wrapper. Ordinary transcript/help conversions still
use `u16`. The contract's universal no-overflow/all-content-reachable wording
therefore remains unestablished outside the repaired history/replay cases;
ordinary plain-wrapper tests are not proof of all Markdown/narrow/Unicode
layouts.

**C3 — resolved at parser and direct interactive override boundaries.**
The record now reports requiring host execution in the CLI and rejecting
every direct `Some(host_turns)` override otherwise. Both
`host_turns_without_host_execution_is_rejected` and
`host_turns_without_host_execution_is_rejected_by_resolver` pass, as does
`sandboxed_rejects_any_host_turn_cap`. Current behavior matches the retained
CLI contract rather than silently ignoring the flag.

**C4 — open older-runtime observed mismatch.** The unchanged runtime record
reports unsafe signal-installation expressions while runtime requirements/
design forbid unsafe Rust. A version-envelope correction, passing tests and
source-identity agreement do not resolve that constraint. Older signal/global
state/early-supervisor-cleanup limits are not newly certified by this review.

### Structural validity is not accepted revision state

The supplied artifact inspection at 01:11:17 records successful
`kvist component validate agent_runner` and `agent_runtime`; `kvist doctor`
reports valid version-one documents, including both implementation-record
envelopes. Its project “current”/read-only-ready output is structural/project
state, not source-blind compliance approval.

In that same log `kvist status` explicitly reports **both components stale**,
with changed requirements/contract/design versus recorded revisions.
`kvist task next` refuses both because they are stale. Thus validity and
tracked files must not be represented as current accepted component intent.
This comparison uses current intent, not a claim that those revisions have
been accepted. No queue progress, operational journal or suggested acceptance
command is used as compliance evidence or executed here.

The log lacks hashes of the inspected document bytes, and precedes this
report append; its structural passes describe that invocation, not a newly
executed structural validation of every final document byte. Current record
envelopes are directly observed, but historical inspection is not a minted
acceptance receipt.

### Remaining coupled observations and bounded outcome

Two further limits in the new runner record remain visible without expanding
this into a blanket review of every older UI behavior:

- **C5 — observed mismatch, interactive override validation:** The record
  reports using interactive `--cwd` directly without eager directory
  validation/canonicalization. Current CLI contract says overrides, including
  `--cwd`, are validated before the UI starts. Headless validates it, but that
  does not establish the interactive promise. No supplied named case resolves
  this narrower discrepancy.
- **C6 — evidence gap, transcript preservation on resize:** The record
  reports Markdown rerendering through following unmarked rows, potentially
  absorbing plain notices/reasoning instead of separately reconstructing them.
  Passing reasoning-collapse tests address collapse, not resize. Preservation
  of every transcript row across resize remains unestablished; this is not
  represented as a demonstrated violation of the narrower collapse clause.

No new discrepancy is observed for complete canonical context/reserve
preflight, provider output-bound wire mapping, exact bounded native operations,
fallible synchronized pre-effect recording or required sandbox-only headless
execution within the recorded scopes. F1/F3/F6/F7, the concrete F5 defects,
C1/C3 and the concrete C2 navigation defects are resolved. F2's missing built-in
checks and F8's cancelled-multicall/live-caller gaps are also resolved, with
their cooperative and process-evidence limitations explicitly retained.

Open issues are F4 authority arbitration, F9/C4 older-runtime constraints,
C5, the remaining C2 layout assurance and C6 resize evidence. Companion-tool
availability, multi-choice stream integrity, arbitrary callbacks, kernel
latency, escaped process trees, aggregate memory, concurrent-writer/crash
behavior and universal external enforcement remain bounded exclusions or
unestablished guarantees, not silently certified capabilities.

The comparison has completed without an access/execution blocker. The
structural log shows unaccepted stale component revisions, and the permitted
inputs supply neither acceptance nor security-audit evidence. All discrepancies
remain for explicit human disposition. Review completion and passing tests do
not approve intent, certify the whole runtime, authorize fixes or mint evidence.

## Handoff snapshot comparison — 2026-10-02

### Boundary, document identity and supplied evidence

This single appended comparison preserves all preceding findings and
provenance byte-for-byte. It uses only the same twelve global/component
intent/record paths listed in the preceding appendix, this report for append
preservation, and the six permitted logs below. No source, test source,
manifest, queue, Git/history/diff, other report/review/research, security report
or other agent output was accessed. No tests, agents, structural commands or
qualification were executed by this reviewer.

The twelve current document hashes were independently calculated and compared
with `handoff-artifact-inspection.log`: **twelve matches, no mismatches**.
The paths in this table are relative to exactly `/opt/proj/kvist/`:

| Current input | SHA-256 |
| --- | --- |
| `VISION.md` | `9340e53b51efce79ead87bc76b264f86b353cc99ea6228bf6bf99909367f6886` |
| `ARCHITECTURE.md` | `877edcec2e97a916120878b9df54e8eb2118a66c79adf7dd86767b68a9055666` |
| `ROOT_CONTRACT.md` | `cb10d639c82054b219b74a4517743867b85ef72fa4661c8deada8b10b744bfe6` |
| `docs/standards.md` | `fe1913244e36a4566ee386c51bcca90e3c41dd91ac10ec107af75289da145afa` |
| `agent_runner/REQUIREMENTS.md` | `1172ed1499ce2565cc2da19d9a38d2e155413e4402787d3400224b51626960d0` |
| `agent_runner/CONTRACT.md` | `e19db5744a80a0dff7ec8314d2dcf1e50ac4b961e71d6099bcf504539ec79d17` |
| `agent_runner/DESIGN.md` | `be5498e7d4c39c3e3bf8985785b415b183345e4373923d8f4c7ce28cd5301912` |
| `agent_runner/IMPL.md` | `f5bf350e8976c977d6fa9b5e1f1dfcc7a094f8ba1191302d6297e71932a64d1e` |
| `agent_runtime/REQUIREMENTS.md` | `c69a969113c1896e07097e238ccc686e7710f54d155f038090f0eda0682930f2` |
| `agent_runtime/CONTRACT.md` | `b0397042ceb9615189474439b74c60a173a4ad258354df7df6d65af5e94a9d7d` |
| `agent_runtime/DESIGN.md` | `100caaa460ee11e5db50f29425f4dd56bde6a22c945070b91df34b0ebf9bb365` |
| `agent_runtime/IMPL.md` | `5dd818a0e47cc63785bacfffaaf3820c9bdbf55ed1c8f7f01d075fb899da52b9` |

The regenerated runner record declares its source-only scope and supplied
component/live executions. This reviewer did not inspect its derivation
context. The retained runtime record and its earlier execution provenance
remain unchanged.

All evidence paths below are relative to exactly
`/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/`.
Hashes and result totals were independently calculated from those log bytes;
times are their reported UTC intervals on 2026-10-02.

| Evidence file | SHA-256 | UTC interval | Passed / failed / ignored |
| --- | --- | --- | --- |
| `runtime-handoff-tests.log` | `e10b3a68ba81aacb554ec4bc99c59596789b1a02f1ed5f185a5e1bc67707f300` | 01:32:35–01:32:47 | 460 / 0 / 3 |
| `handoff-quality-workspace.log` | `5326f4720f0bff1a68b22ddbc99447f1cb4be8bb7c2ee75c11f139b1ebea6e05` | 01:32:35–01:33:13 | 1177 / 0 / 3 |
| `live-handoff-qualification.log` | `ece3862a957d2994502a14a87bd5ff997db292644a25c9465444e962e4499ea3` | 01:32:35–01:32:41 | 3 / 0 / 0 |
| `subprocess-complete-repeat.log` | `3c3ccd7e4796f94d70674a2060761592b5afb46fa869de459bb5596bceece782` | 01:12:01–01:12:13 | 520 / 0 / 0 |
| `handoff-artifact-inspection.log` | `885c827b80f4d5a52f47f86be8fa0fad48c9a996f0ee8eba5c1300472514ee3a` | 01:41:26–01:41:26 | structural commands, not tests |
| `runtime-final-tests.log` | `724b25f2e4a619435ae1aedf8278c6a9eba23ff2a32882d78a64d8ef2ef9e15b` | 00:08:04–00:08:15 | 431 / 0 / 3 |

The component log's 26 result groups comprise **320 runner passes**:
173 library, 35 component, 18 context, four headless, three input-boundary,
33 loop, 28 native and 26 subprocess. Runner binaries/doc tests have zero
cases; three live cases are ignored in that command. Runtime contributes
**140 passes**. Separately running those three live cases yields 323 runner /
463 combined passes across the component/live commands. The earlier
431-pass combined log supplies only the runtime record's identity-matched
runtime provenance, not current-runner evidence. Workspace's 63 groups and
twenty repeated 26-case subprocess runs overlap existing coverage; their
totals do not establish additional distinct requirement cases.

The quality log names formatter check, warnings-denied Clippy, runner/runtime/
Kvist build and the safe-bareRepository-configured locked workspace test
command, as in the preceding appendix. It records finished Clippy/build/test
profiles and no formatter/Clippy failure. It has no printed source inventory:
its timing and named cases associate it with this handoff, but do not
cryptographically bind every compiled input. Reported toolchains remain
Rust/Cargo 1.99.0; MSRV-1.95 execution is not established.

### Source-snapshot and current live-caller linkage

Reading only printed inventories, all **36 runner record identities** equal
both current component/live logs. The independently reconstructed sorted-line
inventory digest is
`5b28500a34786c97258b637b909341dc5bc213d20a66861865bd956bba0ba57c`,
matching the record. All **27 runtime identities** equal both current logs,
the retained record and `runtime-final-tests.log`.

The older repeated-subprocess log's five identities still match current
process/executor/sandbox/test/fixture identities. It supplies repeated
supervision evidence for that unchanged subset, not repeated startup/Markdown
qualification for the newly changed files. Printed identities were not
followed to source or manifest files.

Current live build/hash/run linkage records:

- Caller `/opt/target/debug/deps/live_llama-91e0887ff0465c78`:
  `3c09e7a95d63467d2c89b970ab6f0901f0df5b9a00f1fe73aede8b61d3bf8ed2`.
- Helper `/opt/target/debug/agent-runner-file-tool`:
  `4eb88623d1bddc02c591edad2abc0723257222a85225389fc2e71475cbdcec28`.
- Installed `/opt/target/release/kvist-sandbox-runner`:
  `db631ad7a514a31c0b57f74815ef612f224b25ff8b662affb0a64d9af7fda1b3`.
- `/usr/bin/bwrap`:
  `d9498f8b15b1c69e09791badee317d56f33abd359a306d6e96534f136381ad74`.

The log reports a `--no-run` build followed by that same target's explicit
ignored-test execution at numeric-loopback endpoint `http://127.0.0.1:9931`,
model `Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL`, effort `none`. All three named live
cases pass in 3.32 seconds. Their finite scopes remain native sentinel/network
denial, a no-tool cap-64 answer, and exact CRLF-preserving read/edit/read.
The record explicitly says the edit prompt's requested `DONE` word is not
asserted exactly. Server binary/revision/model-weight identities are absent.
This is internal caller linkage, not authenticated binary/server attestation
or universal isolation/provider qualification.

### Current coupled findings

**C5 — resolved for the original directory-startup mismatch; no discrepancy
observed within the checked boundary.** Current record describes the same
`config::resolve_working_directory` selection in terminal and headless paths.
It canonicalizes the selected default/override and rejects missing and regular
paths before terminal setup, project probing, registry/executor/log/worker
construction. Executor, session metadata, default log/history location and
host prompt use that canonical selection. Relative overrides and directory
symlinks resolve canonically, without globally changing process cwd.

Both current named cases pass:
`startup_working_directory_rejects_missing_and_regular_paths` and
`startup_working_directory_canonicalizes_relative_and_link_paths`.
The record states these exercise the shared resolver, not a real terminal
startup. That finite execution limitation does not recreate the former
source-observed absence of validation. Configured-directory validation still
precedes overrides; explicit relative log locations remain process-cwd-based,
not implicitly rebased to the selected workspace.

**C6 — original following-row absorption gap resolved; no discrepancy observed
for block boundaries.** Current design and record agree on explicit Markdown
`Start(raw_source)` versus `Continuation` provenance. Resize replaces a Start
and its consecutive Continuations, stopping before notices, reasoning or the
next Start. `resize_preserves_plain_notices_and_reasoning_after_markdown`
passes; the record describes repeated-width sentinel assertions, not merely
reasoning-collapse coverage.

This does not establish lossless reconstruction after oldest-row eviction
removes a source-bearing Start. Orphan Continuations then take the plain-row
path, and arbitrary multi-span style/original paragraph reconstruction is not
retained. These are bounded-retention/provenance limits, not evidence that
resize still absorbs following unrelated rows.

**C2 — the reported scalar/decorated-line wrapping defects now resolved within
the current cell model; remaining concrete boundaries stay open.** The record
describes cell-based word/table sizing, grapheme-safe chunking/clipping and
final styled wrapping **after** list/gutter/table decoration. Paragraph content
has a one-cell floor rather than an artificial ten-cell minimum. Named cases
`styled_markdown_wraps_whole_graphemes_by_terminal_cells`,
`decorated_markdown_stays_within_narrow_cell_widths` and
`table_alignment_clips_only_at_grapheme_boundaries` pass, alongside retained
plain/order/indentation and large-replay-offset cases. The inspected assertion
scope includes combining/CJK/ZWJ examples, widths 3/4/7/10 and replay rows beyond
65535.

The following distinctions prevent a blanket C2 closure:

1. **Physical/evidence limit, not a new implementation defect:** an indivisible
   grapheme wider than its viewport is retained rather than split; terminal/
   font cell-width equivalence is not established by Ratatui/TestBackend tests.
   Current design explicitly recognizes that physical limit.
2. **Contradictory intent at the indentation boundary:** exact source-leading
   whitespace on continuations and never exceeding the box cannot both hold
   when that prefix alone is wider than the viewport. The record explicitly
   reports overflowing plain indentation. No policy exception in the current
   contract resolves this boundary; it must not be hidden under font limits.
3. **Record-reported observed mismatch, reachable content:** long code/table
   tails are clipped and discarded before final wrapping. That does not
   establish the contract's long-line wrapping/all-content-reachable promise
   for those inputs. Grapheme-safe clipping tests establish clipping safety,
   not reachability of discarded content. No execution of a failing-tail case
   is supplied.
4. **Record-reported observed mismatch, reveal after resize:** hidden reasoning
   is not reflowed on resize, and reveal restores rows at their old width.
   This leaves a concrete path contrary to the universal current-width
   transcript promise; it is not merely uncertainty about physical glyphs.
   The record says current resize tests exclude that path.

Ordinary transcript/help scrolling still uses `u16`; the finite 5000 visible-row
guard is narrower than proof that every wrapped help/transcript extent is
reachable. Styled-run word splitting can insert whitespace at style
boundaries; exact Markdown/text fidelity is not newly certified. These notes
remain tied to the changed layout/resize scope, not an audit of unrelated
legacy UI features.

**Bounded resize retention — no discrepancy observed at the specified mutation
point; not aggregate boundedness.** Current record describes invoking the
5000-row retention helper after visible resize reflow, as well as normal
Markdown/plain/reasoning appends, and then clamping scrolling.
`resize_keeps_narrow_transcript_rows_bounded` passes; its recorded assertion
scope reflows 4000 long plain rows to width two and checks at most 5000 rows.
This supports the changed design's bounded *resized visible transcript*.

Replacement vectors are constructed before trimming, so this is neither a
peak-allocation bound nor whole-UI RAM quota. Hidden-reasoning reveal does not
invoke the helper and can exceed that ordinary cap until a subsequent capped
operation. Pending/raw/hidden text, replay, prompt history/queue and journals
retain independent allocations or growth. No aggregate retention guarantee is
inferred, nor is the absence of one mislabeled as failure of the new resize
guard.

### Retained hardening statuses and structural/process boundary

The current observations and named runs introduce no new discrepancy within
the previously resolved capture, bounded cooperative preparation,
finish-normalization, cadence, growth-safe configuration, concrete toolchain,
worker/cancelled-multicall or live-caller scopes. C1/C3 and concrete C2 replay
navigation/large-offset repairs remain supported. This is a bounded carry-forward
comparison, not renewed certification of every older runtime mechanism.

**F4 remains contradictory intent for human authority arbitration.** Global
protected-artifact/no-shell rules still do not reconcile with the local fully
writable workspace/Bash contract. **F9 and C4 remain observed older-runtime
mismatches:** bounded/private/no-follow/durable trajectory promises versus the
retained replay observation, and the unsafe-Rust prohibition versus recorded
signal-installation expressions. Runner diagnostic history is not the older
runtime trajectory mechanism. Multi-choice stream merging and companion-tool
qualification remain unestablished separately.

The new artifact inspection now binds structural results to the exact twelve
current input hashes above, closing the previous document-byte linkage gap.
It records actual component validation and doctor commands reporting valid
version-one artifacts. In that same identity-bound run, **both components are
still stale**, and each `kvist task next` rejects the component as stale.
Project “current”/read-only-ready output is not accepted component revisions,
compliance or source approval. Queue contents/progress and suggested Git/
acceptance commands are not used as canonical evidence or followed.

Test-before-production chronology, human arbitration and formal acceptance
remain unestablished. Security-report access is deliberately excluded: this
review cannot assess the separate audit result and does **not** claim an audit
was absent or failed. Nothing here fabricates a security receipt or expands
the authorized evidence boundary.

### Bounded handoff conclusion

C5 startup validation, C6 following-row provenance, the concrete styled/
decorated cell-layout defects and the resize-visible-row retention guard now
have aligned intent/record and named passing evidence. Remaining concrete
layout discrepancies are code/table tail reachability and old-width hidden
reasoning reveal; the overwide-indentation promise needs boundary arbitration.
These differ from unavoidable indivisible-glyph/font limits and excluded
aggregate quotas. F4 authority and older F9/C4 remain open.

Finite fixtures and linked live trials do not prove universal terminal
behavior, full companion tooling, multi-choice integrity, hard callback/kernel
preemption, all escaped descendants, aggregate retention or crash/concurrent
writer enforcement. Structural validity still coexists with unaccepted stale
revisions. No access/execution blocker prevented this comparison; no source
edits, intent edits, tests, audit, acceptance or approval were performed.
Review completion only preserves evidence and discrepancies for human
disposition; it is not approval, acceptance or blanket certification.

## Supplied 02:04:56 UTC snapshot comparison — 2026-10-02

### Input continuity and independently checked provenance

This appendix preserves every previous finding and provenance entry unchanged.
Inputs were only the same twelve global/local intent and record paths listed
above, this report for append preservation, and the six permitted logs below.
No source, test source, manifest, queue, Git/history/diff, other report/review,
security report or other agent output was accessed. No tests, agents, structural
commands or live qualification were executed by this reviewer.

Independent hashing matches **all twelve current document identities** printed
by `verified-artifact-inspection.log`. Comparison with this report's preceding
input table finds only these two changed files, relative to `/opt/proj/kvist/`:

| Current input | SHA-256 |
| --- | --- |
| `agent_runner/DESIGN.md` | `373c696278e8c51bb74988abec4b2daaf406e7078b75f00e5ff61ac0568a1902` |
| `agent_runner/IMPL.md` | `535355299a18a934ce35939b8f0c5aa4ff937596aacef2def0924e0cbb88f719` |

The other ten paths/identities are exactly those in the preceding handoff
table, including runtime IMPL
`5dd818a0e47cc63785bacfffaaf3820c9bdbf55ed1c8f7f01d075fb899da52b9`.
Thus unchanged consumer promises and global constraints remain applicable;
the newer private design does not silently amend their authority.

The runner record declares a fresh source-only observation and exclusions;
this reviewer did not inspect or certify its derivation context. All **36**
printed runner source/test/manifest identities equal both current
component/live logs. Their independently reconstructed sorted-line aggregate is
`bfceaad1e79d08e8ff353d2d07014b2a3bd1b94d3c63298e4952955fc88b5f68`,
matching the record. All **27** runtime identities equal both logs, the retained
runtime record and its earlier `runtime-final-tests.log`. The repeated
subprocess log's five unchanged supervision identities still match current
inventory. No printed source/manifest paths were opened.

All following evidence paths are relative to exactly
`/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/`.
SHA-256s and totals were independently calculated; times are reported UTC
intervals on 2026-10-02.

| Evidence | SHA-256 | UTC interval | Passed / failed / ignored |
| --- | --- | --- | --- |
| `runtime-verified.log` | `c7e6b2bbccd96758ff344c8c462ebc2fa66f19ebcb5c46976e3ba29dfcd6dfd3` | 01:59:04–01:59:15 | 465 / 0 / 3 |
| `verified-quality-workspace.log` | `d93427f036e035ba8935fb466b9f5ccf84e3b26d514368dd79198c6cf62fab6b` | 01:59:04–01:59:43 | 1182 / 0 / 3 |
| `live-verified.log` | `86b0cbd54c596b1a5e609c573eab54cde50309c801ee8a61f263c470a87c0488` | 01:59:04–01:59:09 | 3 / 0 / 0 |
| `subprocess-complete-repeat.log` | `3c3ccd7e4796f94d70674a2060761592b5afb46fa869de459bb5596bceece782` | 01:12:01–01:12:13 | 520 / 0 / 0 |
| `verified-artifact-inspection.log` | `eef9779b42898e1939b68a16620c4a3d9daa0ec5f500ffc8b09aeda8e12b7eff` | 02:04:56–02:04:56 | structural commands, not tests |
| `runtime-final-tests.log` | `724b25f2e4a619435ae1aedf8278c6a9eba23ff2a32882d78a64d8ef2ef9e15b` | 00:08:04–00:08:15 | 431 / 0 / 3 |

The current component command is
`cargo test --locked -p agent-runner -p agent-runtime`. Its 26 result groups
contain **325 runner passes**: 178 library, 35 component, 18 context, four
headless, three input-boundary, 33 loop, 28 native and 26 subprocess; other
runner binary/doc targets have zero cases. Runtime contributes **140 passes**.
The separate live command gives **328 runner / 468 combined passes**, without
rewriting the component command's three ignored cases. Workspace has 63
groups; repeated supervision has twenty 26-case groups. These overlapping
totals must not be presented as additional distinct requirement coverage.
The earlier combined log is runtime provenance only, not current-runner
qualification.

Quality evidence names `cargo fmt --all --check`, warnings-denied locked
Clippy for runner/runtime all targets, the locked runner/runtime/Kvist build,
and the locked workspace test with the supplied safe-bareRepository settings.
Finished Clippy/build/test profiles are printed, with no formatter/Clippy
failure. The quality log still lacks a source inventory, so exact compiled-input
binding is weaker than the component/live inventory linkage. Reported
Rust/Cargo versions remain 1.99.0, not an MSRV-1.95 run.

### Current C2 reachability and reveal statuses

**Code/table tail discrepancy — resolved within the observed renderer;
no discrepancy observed for retaining collected tails.** Current design and
record agree that code emits complete highlighted spans, then wraps them,
instead of clipping tails. Table cells split into whole-grapheme chunks,
emit successive rows and receive final decorated-line wrapping. The record
expressly distinguishes production chunking from the private prefix-clipping
helper: fitting chunks may use that helper, while overwide chunks bypass it.
The old claim that production tails are discarded is therefore not carried
forward as current behavior.

Both `highlighted_code_keeps_the_complete_tail_when_wrapped` and
`narrow_tables_keep_complete_cell_tails` pass in the component and workspace
logs. The record describes code/CJK and single-column table/CJK width-five
assertions. Table tests remove rendering spaces before searching for the value:
this establishes the finite tail cases, not exact source whitespace or
multi-column copy reconstruction. Physical/font/all-construct fidelity is
not inferred.

**Old-width revealed reasoning and reveal-cap gap — resolved within the
observed path; no discrepancy observed.** The record describes rewrapping
saved reasoning rows at the current content width, preserving row kind and
first-span style, then applying retention/clamping/following.
`revealed_reasoning_reflows_at_the_current_width` passes; its described
20-character case retains concatenated content within inner width six.
Current visible rows are capped after append/reflow/collapse/**reveal**.
The prior old-width/reveal-bypasses-cap observations remain historical, not
current accusations. Saved rows are not original paragraph provenance, and
widening cannot reconstruct that unavailable original structure.

**Placeholder continuation/hidden-run correspondence — resolved for the
newly specified mechanism; no discrepancy observed in the exercised cases.**
Current design requires distinct placeholder continuation provenance.
The record describes tagging fragments after the first head as
`PlaceholderContinuation`, preserving that kind through later resize,
consuming a saved run only at a head and skipping label continuations on
reveal. Eviction drains hidden runs according to evicted heads, not every
wrapped label row.

`wrapped_placeholders_keep_distinct_reasoning_runs_in_order` passes with the
recorded two-run, intervening ordinary row, widths-eight-then-ten assertion
whose revealed concatenation is exactly `firstbetweensecond`.
`evicted_placeholders_do_not_restore_the_wrong_reasoning` separately passes
across 5000 filler rows. The latter does not combine placeholder resize and
eviction, so arbitrary joint-boundary coverage remains an **evidence gap**,
not a demonstrated stale/duplicate-head defect. The positive two-resize
observation must not be replaced by the earlier limitation it now addresses.

### Remaining current layout observations, separated by class

- **Observed mismatch, actionable initial-label boundary:** the current record
  explicitly reports initial collapse placeholders as single **unwrapped**
  rows, with immediate narrow display not guaranteed to fit. This conflicts
  with the consumer promise that transcript rows are prewrapped to their
  current box. The label is splittable ordinary text; this is not the
  indivisible-glyph physical exception or the repaired continuation/head
  association bug. Later resize wrapping does not establish initial fit.
  No supplied immediate-collapse/narrow-label regression resolves it.
- **Evidence gap, continuation indentation:** the record expressly notes
  wrapped code continuations do not repeat the source gutter/indentation
  convention, and styled wrapping normalizes whitespace. Repetition of the
  gutter itself is not required, but preservation of source-leading whitespace
  on continuations is a consumer promise. The allowed record/tests do not
  establish that promise for fitting code indentation. This is kept visible
  without inferring exact unseen row contents or claiming a tested violation.
- **Contradictory intent, overwide indentation:** the contract still combines
  exact leading-whitespace prefixes with never exceeding the box. When the
  prefix alone exceeds the viewport, both cannot hold. The record still
  reports overflowing indentation. No exception has been fabricated; human
  boundary-policy arbitration remains necessary.
- **Physical/assurance limits:** indivisible overwide graphemes, physical
  terminal/font widths and cross-style grapheme segmentation are not
  universally qualified. These are not automatically implementation defects.
  Styled whitespace normalization, row-based rather than block-atomic
  eviction, orphan Markdown Continuations after Start eviction, and
  rendered-row reconstruction limit fidelity. Existing fitting-layout and
  tail fixtures do not certify every retention/layout combination.
- **Aggregate/cooperative limits:** the 5000-row guard acts after constructing
  intermediate vectors, not as a peak/aggregate UI memory bound. Hidden/raw/
  pending text, replay, prompt history/queue and journals have separate
  allocations/growth. Callback and kernel cancellation remain cooperative;
  escaped descendants, concurrent writers and crashes are not universally
  controlled. These limits do not recreate the repaired per-mutation reveal
  guard or authorize a wider assurance claim.

### Retained statuses, current execution linkage and acceptance boundary

F1/F2/F3/F6/F7, concrete F5 candidate/constructor fixes, C1/C3/C5, C6's original
following-row absorption issue and concrete replay navigation/offset fixes
retain their bounded positive observations. No unrelated legacy feature has
been added to this comparison. Companion-tool qualification and runtime
multi-choice integrity remain separate unestablished guarantees.

**F4 remains contradictory global/local authority for human arbitration.**
**Older F9/C4 remain observed mismatches**, respectively the distinct runtime
trajectory bounds/path/privacy/durability posture and unsafe signal installation
versus the runtime constraint. Unchanged runtime bytes and passing replay tests
do not repair either issue or newly certify the older runtime.

Current live caller linkage supplies the same `--no-run` then ignored-test
build/hash/run sequence, all 36 matching source identities, and these logged
executable hashes:

- `/opt/target/debug/deps/live_llama-91e0887ff0465c78`:
  `21ee008846fc059275550dc8c1c847947851a2725807c96808efbc1e9259f7d6`.
- `/opt/target/debug/agent-runner-file-tool`:
  `4ca60e7a5381febc0d47ad83f5d93cf314b3ba19d8af07c06c27c99de31f5400`.
- `/opt/target/release/kvist-sandbox-runner`:
  `db631ad7a514a31c0b57f74815ef612f224b25ff8b662affb0a64d9af7fda1b3`.
- `/usr/bin/bwrap`:
  `d9498f8b15b1c69e09791badee317d56f33abd359a306d6e96534f136381ad74`.

Endpoint/model/effort remain `http://127.0.0.1:9931` /
`Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL` / `none`. All three named live cases pass
in 2.69 seconds, within their previously described finite native sentinel/
network, cap-64 answer and exact read/edit/read scopes. There is no newly
supplied real-terminal startup/resize trial, authenticated executable
attestation, or server/weights identity.

The 02:04:56 artifact log binds current input identities to actual
`kvist component validate agent_runner`, `kvist component validate agent_runtime`
and `kvist doctor` outputs reporting valid version-one artifacts.
`kvist status` nevertheless reports **both components stale**, and
`kvist task next agent_runner` / `kvist task next agent_runtime` reject that
state. No queue progress, suggested Git command or acceptance suggestion is
used as compliance evidence or executed. Valid documents/project “current”
output is not accepted component revisions.

Security-report access remains deliberately excluded. This review does not
assess the separate audit or infer that it was absent/failed. Test-before-code
chronology, human arbitration and formal acceptance remain unestablished by
these inputs.

**Bounded outcome:** the previously actionable code/table tails, old-width
reveal and specified placeholder correspondence defects now have aligned
record/design and named passing evidence. Initial narrow placeholder fit
remains an actionable record-reported mismatch; fitting code-continuation
indentation and joint eviction/resize coverage remain evidence gaps. Overwide
indentation and F4 need human arbitration, while older F9/C4 and explicit
assurance limits remain visible. There was no access/execution blocker to this
comparison. Its completion changes only this report and is not approval,
formal acceptance, security evidence or universal certification.

## Final initial-label snapshot — 2026-10-02, supplied inspection 02:16:15 UTC

### Exact input continuity and evidence linkage

This bounded appendix preserves all previous text and provenance unchanged.
Inputs were only the same twelve global/local intent and IMPL paths already
listed, this report for append preservation, and the six permitted logs below.
No source, test source, manifest, queue, Git/history/diff, other report/review,
security report or other agent output was accessed. No tests, agents,
structural commands or live calls were executed by this reviewer.

Independent hashing matches **all twelve current input identities** printed
by `final-delivery-artifact-inspection.log`. Only these two differ from the
preceding input provenance, relative to `/opt/proj/kvist/`:

| Current input | SHA-256 |
| --- | --- |
| `agent_runner/DESIGN.md` | `37cd366b0c63ec27bcfb9a53dc47f15d94659b57591180027363716f1f385226` |
| `agent_runner/IMPL.md` | `ab614a72be04f2018a625265470d3a5fb32093f57047db675c18e9a9bdb4b8c5` |

The remaining ten exact paths/hashes are unchanged from the prior tables.
In particular, requirements, consumer contracts and global authority have
not acquired an indentation or authority exception. Runtime IMPL remains
`5dd818a0e47cc63785bacfffaaf3820c9bdbf55ed1c8f7f01d075fb899da52b9`.
The regenerated runner record declares source-only observation and its
exclusions; this reviewer did not inspect or certify its derivation context.

All **36 runner record inventory entries** independently equal both delivery
component/live logs. The reconstructed sorted-line inventory digest is
`222bd3795032698430464f02c6c599ee819dacfb30d789bf3101dcf8871debbe`,
matching the record. All **27 runtime entries** equal both delivery logs,
the retained record and `runtime-final-tests.log`. The repeated-subprocess
log's five supervision identities still match the current subset. These
compare printed identities only, without opening their source/manifest paths;
they establish internal linkage, not independent authenticity.

All following log paths are relative to exactly
`/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/`.
Digests and result totals were independently calculated. Intervals are
reported UTC on 2026-10-02.

| Evidence | SHA-256 | UTC interval | Passed / failed / ignored |
| --- | --- | --- | --- |
| `runtime-final-delivery.log` | `4d2c5be78877dc197024102781df69f8269ef8eee33473a4fcfd0503c51ce308` | 02:10:54–02:11:06 | 466 / 0 / 3 |
| `quality-final-delivery.log` | `8b9a9d18e7d8c310b5ba108ac9e2c3ff243c55a8c454ee7d4bf355ba2a1d0ea4` | 02:10:54–02:11:34 | 1183 / 0 / 3 |
| `live-final-delivery.log` | `89b3a2df3253c0e2b731493c5f6d0c1630989425a90c33784b2e55f616999e83` | 02:10:54–02:10:59 | 3 / 0 / 0 |
| `subprocess-complete-repeat.log` | `3c3ccd7e4796f94d70674a2060761592b5afb46fa869de459bb5596bceece782` | 01:12:01–01:12:13 | 520 / 0 / 0 |
| `final-delivery-artifact-inspection.log` | `8168e2859e714f0031f2f404d3cd382e066997a98ccefd40c09fcc757d53b6f4` | 02:16:15–02:16:15 | structural commands, not tests |
| `runtime-final-tests.log` | `724b25f2e4a619435ae1aedf8278c6a9eba23ff2a32882d78a64d8ef2ef9e15b` | 00:08:04–00:08:15 | 431 / 0 / 3 |

Current locked runner/runtime tests have 26 result groups:
**326 runner passes** (179 library, 35 component, 18 context, four headless,
three input-boundary, 33 loop, 28 native and 26 subprocess) plus **140 runtime
passes**. Other binary/doc targets have zero cases; three live cases remain
ignored in that offline command. Separate live execution yields **329 runner /
469 combined passes** across those two commands. Workspace has 63 result
groups; repeated supervision has twenty groups of 26. Coverage overlaps and
must not be counted as additional distinct requirement cases. The earlier
combined log remains runtime provenance only.

Quality evidence names the same formatter check, warnings-denied all-target
runner/runtime Clippy, runner/runtime/Kvist build and configured locked
workspace test commands. It prints finished Clippy/build/test profiles and no
formatter/Clippy failure; its inventory remains absent, so exact compiled-input
binding is weaker than the component/live linkage. Reported toolchains remain
Rust/Cargo 1.99.0, not an MSRV-1.95 execution.

### Last initial-label finding: resolved within its concrete scope

**No discrepancy observed for current initial collapsed-label construction.**
The prior unwrapped-initial-placeholder finding is resolved, not retained as a
current defect. Current design explicitly requires initial width-aware
construction with the same head/continuation distinction, before resize.
The record reports that **both middle-run and trailing-run collapse branches**
use `collapse_placeholder_rows(self.content_width())`. The helper wraps the
label immediately, marks only its first fragment as Placeholder and every
remaining fragment as PlaceholderContinuation.

`initial_collapsed_placeholders_fit_the_current_width` passes in both component
and workspace logs. The record describes resizing before inserting two
reasoning runs with a normal `mid` row, collapsing at width eight, checking
measured rows through inner width six and revealing exact
`firstmidsecond`. Unlike the older resize-only test, this addresses initial
construction and both separated runs without a subsequent resize being needed.

Current marker/reveal assertions also remain passing:
`wrapped_placeholders_keep_distinct_reasoning_runs_in_order`,
`revealed_reasoning_reflows_at_the_current_width` and
`evicted_placeholders_do_not_restore_the_wrong_reasoning`. Missing exhaustive
fonts, widths or combined eviction/resize permutations is an **evidence
limit**, not a demonstrated current duplicate-head or initial-unwrapped-label
defect. Row eviction can leave orphan label continuations after discarding
their head/run; reveal removes those continuations. Block-atomic retention,
perfect label-block fidelity and universal terminal behavior are not inferred.

### Remaining concrete discrepancies versus policy/evidence limits

**Actionable, record-reported fitting code-indentation mismatch:** the prior
code-continuation indentation question is already within this review's changed
layout scope, not a new legacy-feature audit. The current record now expressly
says code continuation rows “do not repeat original line indentation/gutter.”
Gutter repetition is not itself a consumer requirement, but repeating source
leading whitespace on continuations is required by the unchanged requirements/
contract. Thus the record does not satisfy that promise even for an indentation
prefix that fits the viewport. This is distinct from the impossible
overwide-prefix combination. Classification is **observed mismatch based on
the record**, not a personally reproduced failing execution; no supplied
fitting-indented-code continuation case resolves it.

**Policy arbitration, not a fabricated implementation exception:** overwide
indentation still conflicts with simultaneous exact indentation and
never-exceed-width promises. F4 still exposes unreconciled global protected-
artifact/no-shell authority versus the local writable-workspace/Bash scope.
Both remain **contradictory intent** for explicit human disposition.

**Older separate observed mismatches:** F9's runtime trajectory bounds/
no-follow/privacy/durability posture and C4's unsafe signal installation remain
open. Unchanged runtime identities and passing replay cases do not resolve
those older constraints or newly certify all runtime behavior.

**Acknowledged evidence/assurance limits:** physical indivisible glyph/font
behavior, styled-run whitespace/cross-style segmentation fidelity, source
recovery after row eviction, every combined retention boundary and exact
multi-column Markdown copying remain unestablished. Row guards do not bound
intermediate peak allocation, hidden/raw/pending/replay/queued data or aggregate
session disk/I/O. Arbitrary callbacks/kernel operations remain cooperative;
escaped descendants, concurrent writers and crashes are not universally
controlled. These are not relabeled as tested defects merely because universal
fixtures are absent. Companion-tool qualification and runtime multi-choice
integrity remain separate limitations.

### Carried-forward resolutions, live linkage and structural gates

Bounded capture/preflight, finish handling, cadence, growth-safe configuration,
concrete toolchain fixes, lifecycle/cancelled-multicall and live-caller scopes
retain their previous positive statuses. C1/C3/C5, C6's following-row boundary,
replay navigation/offsets, code/table tail reachability, current-width reasoning
reveal and per-mutation visible retention remain supported by current record/
named results. Initial labels now join those resolved concrete scopes.
No universal or unrelated legacy compliance claim follows.

Current live build/hash/run evidence again binds the matching source inventory
to the recorded executed caller:

- `/opt/target/debug/deps/live_llama-91e0887ff0465c78`:
  `e25362395cd443dc8335c881ce9f18984806658f48b49c9c7f40ba86f10ea1a2`.
- `/opt/target/debug/agent-runner-file-tool`:
  `f077d76c3c168ee8b2510f2560d26bff7927e115700583a677638602a8799300`.
- `/opt/target/release/kvist-sandbox-runner`:
  `db631ad7a514a31c0b57f74815ef612f224b25ff8b662affb0a64d9af7fda1b3`.
- `/usr/bin/bwrap`:
  `d9498f8b15b1c69e09791badee317d56f33abd359a306d6e96534f136381ad74`.

The same three explicit ignored live cases pass in 2.21 seconds at
`http://127.0.0.1:9931`, model `Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL`,
effort `none`. Their finite cap-64/native-sentinel/network/read-edit-read
assertions are unchanged in scope; neither server/weights identity nor
real-terminal layout execution or independent binary attestation is supplied.

The exact-input-linked 02:16:15 inspection records actual
`kvist component validate agent_runner`, `kvist component validate agent_runtime`
and `kvist doctor` results reporting structurally valid version-one documents/
current project. `kvist status` still reports **both components stale**;
`kvist task next agent_runner` and `kvist task next agent_runtime` reject them
as stale. Queue progress, suggested Git/acceptance commands and operational
journals are not compliance evidence or executed instructions here.

Security-report access is deliberately excluded; this review does not assess
the audit or infer that it was absent/failed. Test-before-code chronology,
human arbitration and accepted revisions remain unestablished.

**Bounded conclusion:** initial-label width construction is resolved within
the recorded mechanism and named finite test. The previously raised fitting
code-continuation indentation promise remains an actionable record-reported
discrepancy; F4/overwide indentation require arbitration, and older F9/C4 and
explicit assurance limits remain visible. No access/execution blocker
prevented this comparison. Completion only appends this report; it is not
approval, acceptance, security evidence or blanket certification.

## Fitting-prefix delivery snapshot — 2026-10-02, inspection 02:26:50 UTC

### Permitted inputs and independently derived provenance

This appendix preserves every preceding report byte. Only the same twelve
global/local intent and IMPL paths, this report for append preservation, and
the six logs below were read. No source, test source, manifest, queue,
Git/history/diff, other report/review, security report or other agent output
was accessed. No tests, agents, structural commands or live calls were
executed by this reviewer.

Independent hashing matches **all twelve current input identities** in
`delivery-qualified-artifacts.log`. Only two differ from the preceding
provenance table, relative to `/opt/proj/kvist/`:

| Current input | SHA-256 |
| --- | --- |
| `agent_runner/DESIGN.md` | `8e0acacd3ee74f3df78d11268879cf6abd3ac34029bb251b8f86cee8258385f0` |
| `agent_runner/IMPL.md` | `73e0420e5dc2f219a4ae2b0c8bc581d74cb442f807b9251567d30300ecef50cf` |

The other ten exact paths/hashes are unchanged from earlier tables, including
runtime IMPL
`5dd818a0e47cc63785bacfffaaf3820c9bdbf55ed1c8f7f01d075fb899da52b9`.
Requirements, consumer contracts and global constraints remain unchanged.
The runner record declares its fresh source-only boundary; this reviewer
does not independently certify that derivation context.

All **36 printed runner inventory entries** independently equal both current
component/live logs. The reconstructed sorted-line aggregate is
`b8467755254888b355f5904c1fdbc598b62d12e9b438de0fcd376f9dff117af8`,
matching the record. All **27 runtime identities** equal both current logs,
the retained record and earlier `runtime-final-tests.log`. The repeated
subprocess log's five supervision identities also match current inventory.
No printed source/manifest paths were opened. This is internal snapshot
linkage, not independent execution authentication.

Evidence paths below are relative to exactly
`/home/stefan/.copilot/session-state/b5e05792-9383-4aff-87cc-d325bf06e011/files/`.
Digests and totals were independently calculated; intervals are reported UTC
on 2026-10-02.

| Evidence | SHA-256 | UTC interval | Passed / failed / ignored |
| --- | --- | --- | --- |
| `runtime-delivery-qualified.log` | `160031bfcf491dde59964ae6abfe91b85cbe3c0581c68b3777b7d42df192ece7` | 02:21:47–02:21:58 | 467 / 0 / 3 |
| `delivery-qualified-gates.log` | `80e2981febe5a4a41c9f1f794d928a911db0640499dc9e63ddb3a05b0eb40ed4` | 02:21:47–02:22:25 | 1184 / 0 / 3 |
| `live-delivery-qualified.log` | `cfaf987d285794d8ccd6023f55e5388432949c6e083cc026aa43bb9d3826f5c0` | 02:21:47–02:21:52 | 3 / 0 / 0 |
| `subprocess-complete-repeat.log` | `3c3ccd7e4796f94d70674a2060761592b5afb46fa869de459bb5596bceece782` | 01:12:01–01:12:13 | 520 / 0 / 0 |
| `delivery-qualified-artifacts.log` | `96b29b6a7af4e45ef639f3502370f4d464707135ebc86ad5ed8c4a822df10e36` | 02:26:50–02:26:50 | structural commands, not tests |
| `runtime-final-tests.log` | `724b25f2e4a619435ae1aedf8278c6a9eba23ff2a32882d78a64d8ef2ef9e15b` | 00:08:04–00:08:15 | 431 / 0 / 3 |

Current locked component execution has 26 result groups: **327 runner passes**
(180 library, 35 component, 18 context, four headless, three input-boundary,
33 loop, 28 native, 26 subprocess) plus **140 runtime passes**. Other binary/
doc targets have zero cases. Separate live execution yields **330 runner /
470 combined passes**, without rewriting the offline command's three ignored
results. Workspace has 63 groups; repeated supervision has twenty 26-case
groups. These overlapping runs do not create additional distinct requirement
coverage. The earlier combined log remains runtime provenance only.

Quality evidence again names formatter check, warnings-denied locked
runner/runtime all-target Clippy, runner/runtime/Kvist build and configured
locked workspace tests. Finished Clippy/build/test profiles and no formatter/
Clippy failure are printed. Its source inventory is absent, retaining the
exact compiled-input linkage limitation. Toolchains remain Rust/Cargo 1.99.0;
MSRV-1.95 execution is not established.

### Last fitting code-continuation mismatch: resolved in bounded fitting scope

**No discrepancy observed for the repaired fitting-prefix mechanism.** The
previous record's categorical absence of repeated code indentation is not
current behavior. Current design states that highlighted code repeats
source-leading whitespace when it fits, retains the gutter only on the first
row, and explicitly leaves excessive-indentation policy unresolved.

The current record describes taking each collected code line's exact leading
whitespace into styled continuation-prefix spans and passing complete
highlighted ranges through `wrap_styled_line_with_prefix`. That wrapper clones
the prefix when beginning continuation rows; the gutter is not the prefix.
This directly addresses the previously observed missing mechanism rather
than relying only on a new test name.

`code_continuations_preserve_fitting_source_indentation` passes in both
component and workspace logs. Its recorded inspected assertion renders two
leading spaces followed by `abcdefghijklmnop` at width eight, checks multiple
rows through eight cells, checks every continuation begins with two spaces,
and checks complete body after removing gutter/spaces. This supports the
concrete fitting-indent repair without certifying every whitespace or parser
construct.

**Fitting must include room for content, not merely the prefix alone.**
The record reports that prefix spans are neither clipped nor independently
required to leave room for the next indivisible grapheme. A prefix narrower
than the viewport can still leave insufficient capacity for that grapheme.
Such an over-capacity prefix/content combination is not the tested genuinely
fitting case. Further final empty-prefix wrapping of an overflowing
intermediate row can repartition it without reapplying the source prefix.
The record also excludes separately resolved physical tab stops and arbitrary
nested decoration/font cases.

Those excessive-prefix/indivisible-cell/final-repartition limits prevent a
universal “preserves all indentation” claim; they do not reestablish the old
“never repeats indentation” defect. No concrete failure of the recorded
genuinely fitting mechanism is established by the permitted inputs. Missing
universal cases remain **evidence limits**, not newly demonstrated defects.
The unchanged broad consumer wording is not silently narrowed or accepted by
this bounded positive observation.

### Carried-forward resolutions and remaining categories

**Resolved bounded scopes:** fitting code indentation joins the previous
capture/preflight, finish handling, cadence, configuration growth bounds,
concrete toolchain, worker/cancelled-multicall/live-caller, incomplete-fence,
host-turn flag, startup-directory, resize-provenance, replay-offset,
code/table-tail, current-width reasoning reveal, initial-label and
head/continuation correspondence repairs. Current records and named runs
support those scopes; this does not certify the whole UI or older runtime.

**Remaining actionable record-reported discrepancies:** older **F9** trajectory
bounds/no-follow/privacy/durability posture and **C4** unsafe signal installation
remain open against unchanged runtime intent. These are separate older-runtime
issues, not the runner's repaired diagnostic history or current fitting code
wrapper. No additional concrete implementation defect is established in the
changed fitting-prefix scope by this comparison.

**Human policy arbitration:** overwide indentation still conflicts with exact
continuation prefixes and unconditional never-exceed-width guarantees.
Prefix-plus-indivisible-content capacity reinforces that boundary problem.
**F4** global protected-artifact/no-shell rules versus the local writable
workspace/Bash scope remain unreconciled. The design's acknowledgement is not
an approved exception to consumer/global authority.

**Assurance/evidence exclusions:** physical/font/tab/cross-style segmentation,
every prefix/decorated or collapse/resize/eviction combination, source/style
recovery after row-not-block eviction and exact Markdown copying are not
universally demonstrated. Intermediate prefix copies/render vectors and
hidden/raw/pending/replay/queued data are not bounded by the 5000 retained-row
guard; aggregate disk/I/O quotas are absent. Callback/kernel checks remain
cooperative; escaped descendants, concurrent writers and crashes are not
universally controlled. Companion-tool qualification and runtime multi-choice
integrity remain separate limitations. No arbitrary legacy feature has been
added to the review.

### Current live and structural evidence is not acceptance

The supplied live sequence again builds the named target without running it,
prints matching source and executable identities, then executes that same
ignored-test target. Current logged identities are:

- `/opt/target/debug/deps/live_llama-91e0887ff0465c78`:
  `08b82e8fd163951e364509aebe6904fd745bfc628fe5aec483e01cb7f912e381`.
- `/opt/target/debug/agent-runner-file-tool`:
  `3a7264d97aadd70a89fcf08d7fa97ea57f8e09d3d6fc64ec6a2ab6c3378f9f97`.
- `/opt/target/release/kvist-sandbox-runner`:
  `db631ad7a514a31c0b57f74815ef612f224b25ff8b662affb0a64d9af7fda1b3`.
- `/usr/bin/bwrap`:
  `d9498f8b15b1c69e09791badee317d56f33abd359a306d6e96534f136381ad74`.

Endpoint/model/effort remain `http://127.0.0.1:9931` /
`Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL` / `none`. All three named live cases pass
in 2.80 seconds within the previously stated cap-64/native-sentinel/network/
read-edit-read scopes. No authenticated binary attestation, real-terminal
layout trial or live server/weights identity is supplied.

The current-input-hashed 02:26:50 artifact log records actual component
validation for runner/runtime and doctor results reporting valid version-one
documents/current project. The same `kvist status` invocation reports **both
components stale**; both named `kvist task next` invocations reject them as
stale. Queue progress, suggested Git/acceptance commands and operational
journals are not used as compliance evidence or executed instructions.

Security-report access remains deliberately excluded; no audit result is
assessed or inferred absent/failed. Test-before-code chronology, human
arbitration and formal acceptance remain unestablished.

**Bounded outcome:** the last fitting-prefix mismatch is resolved within the
current mechanism and named assertion. Remaining concrete older F9/C4 issues,
overwide-indent/F4 policy arbitration and explicit assurance limits remain
visible. There was no access/execution blocker. Completing this append is not
approval, acceptance, security evidence or blanket certification.
