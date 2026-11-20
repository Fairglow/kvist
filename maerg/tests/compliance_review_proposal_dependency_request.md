# Compliance Review: Proposal and Dependency-Request Tools (Phase 4)

**Task:** `phase-4-compliance-review`
**Component:** maerg (Kvist root component)
**Kind:** source-blind compliance pass (REVIEW_RUNBOOK.md, "Source-blind compliance
pass")
**Status:** record complete — discrepancies retained for human arbitration; no
intent document was modified and no implementation was certified

## Method and inputs

Per the source-blind pass, this review read only:

- `maerg/REQUIREMENTS.md` (`REQ-DECISION-SURFACING`, `REQ-DEPENDENCY-REQUEST`,
  and the `REQ-DECISION-AUTOMATION` context around decision application);
- `maerg/CONTRACT.md` (the brokered-authoring behavioral guarantee, the
  "Shared Rust dependency mechanics" admission paragraph, the `task run`
  continuous-execution clause, and the decision command surface);
- `maerg/DESIGN.md` (the "Planned dogfooding execution boundary" and the
  "Implemented proposal and dependency-request tools (phase 4)" subsections);
- `maerg/IMPL.md` (the observed closed authoring vocabulary, the
  "Protected provisioning and verification" section, and the observed scope
  notes);
- `ROOT_CONTRACT.md` ("Change and compliance rules");
- bounded test evidence: `tests/dogfood_boundary/workflow.rs`,
  `tests/agent.rs`, `tests/task_commands.rs`, `tests/live_runtime.rs`
  (assertion names and observed behaviors), and the retained phase-4 security
  audit record `tests/security_audit_proposal_dependency_tools.md` (test
  evidence from the immediately preceding phase; not a prior compliance
  conclusion).

The reviewer did not read `maerg/src/**`, dependency manifests, Git history, or
any prior compliance conclusion, and this pass does not modify
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `IMPL.md`, or `TODOS.yaml`.
Test assertions are cited as evidence of exercised behavior; they are not a
substitute for observing the implementation, and they were not re-executed by
this pass (the verification sandbox runs the suite after this turn).

Classifications use the runbook vocabulary: `compliant`, `mismatched`,
`approved-deferred`, `underspecified`.

## Satisfied clauses

### Decision surfacing (REQ-DECISION-SURFACING + CONTRACT behavioral guarantee)

| ID | Clause (source) | Classification | Bounded test evidence |
|----|-----------------|----------------|-----------------------|
| S-1 | An impactful, uncovered decision stops the run, places the component in `awaiting-decision`, and never jumps to `completed` (REQ-DECISION-SURFACING; CONTRACT "the run harness transitions the task to that state and never runs verification or jumps to completed") | compliant | `workflow.rs::propose_decision_pauses_the_run_in_awaiting_decision_and_records_the_proposal` — `TODOS.yaml` task carries `status: awaiting-decision` and not `status: completed`; `task_commands.rs::resilience_repair_decision_uses_the_initial_awaiting_decision_disposition` — run stdout names `awaiting-decision`, attempt record carries `"surfaced_decision":true` and `"stop_reason":null`; `task_commands.rs::tool_only_authoring_decision_has_agent_markdown_and_response_steps` — task 0 is `AwaitingDecision` and no sandbox request was dispatched for the paused turn |
| S-2 | A surfaced decision applies no effects from the paused turn (CONTRACT "ends the run by placing the component in an awaiting-decision state"; design pause ordering) | compliant | both `workflow.rs` pause tests assert the fixture test root was never written (`maerg/tests/generated.rs` absent); `tool_only_authoring_decision_...` additionally asserts no `sandbox-request.json` exists after the pause |
| S-3 | A decision is recorded as durable evidence under the component state directory, never as a protected intent document (CONTRACT "redacted proposal under the component state directory") | compliant | `workflow.rs::propose_decision_...` — JSON proposal evidence under `.kvist/authoring/proposals/` retains the summary and the `why`; `tool_only_authoring_decision_...` — both a `.md` and a `.json` companion exist; security-audit test evidence: no-clobber, symlink-refusing write of `sha256(identity:call_id)` filenames, `decisions_reusing_call_ids_retain_separate_evidence` |
| S-4 | Proposal evidence is redacted (REQ-DECISION-SURFACING "bounded, redacted"; CONTRACT "redacted proposal") | compliant | `agent.rs::execute_agent_redacts_the_decision_patch_in_the_recorded_evidence` — a `propose_decision` patch carrying a configured secret yields `[REDACTED]` and never the raw secret in both the JSON and Markdown proposal evidence, and the run reports `surfaced_decision` |
| S-5 | The Markdown brief contains the agent-authored explanation plus host-added task/attempt and evidence references and the choice/clarify/reject/resume instructions; recommendations remain advisory (REQ-DECISION-SURFACING) | compliant | `task_commands.rs::continuous_run_pauses_on_review_decision_without_running_later_items` — the `.kvist/attention/<task>.md` brief contains the agent text, the attempt reference (`attempt-0001`), `## Choose or approve`, `## Ask for clarification`, `## Reject or redirect`, the apply command and the resume command, and the advisory notice `not human acceptance`; `tool_only_authoring_decision_...` — the agent's `why` sections (`## Alternatives`, `## Recommendation`, …) are carried into the brief |
| S-6 | A continuous run stops at an awaiting-decision point, leaves later work untouched, never auto-resolves a decision, and reports the exact state and resolving command (CONTRACT `task run` continuous clause) | compliant | `continuous_run_pauses_on_review_decision_without_running_later_items` — the decision task is `AwaitingDecision`, all later tasks remain `pending`, the run stdout names the exact Markdown brief file, and a re-run fails with an explicit `run.md` diagnostic naming the task and `awaiting-decision` while `TODOS.yaml` bytes are unchanged |
| S-7 | The agent distinguishes worthy from non-worthy issues; trivial matters do not block and are left for post-hoc advisory comparison (REQ-DECISION-SURFACING; CONTRACT "it never blocks on a trivial matter") | compliant (advisory path) | the brokered-run advisory surface returns the explicit non-blocking note for a routine `propose_decision` status (observed in a retained run log for a tool-only authoring turn: "This routine status note is not a blocking human decision…"); DESIGN documents the post-hoc `IMPL.md` comparison as the retention path. The worthiness judgment itself is model-driven and not exhaustively pinned by a bounded test — see U-3 |
| S-8 | A proposal is a no-clobber draft subject to human review and the normal advisory-review-or-exception acceptance gate; an awaiting-decision component does not accept new implementation work (REQ-DECISION-SURFACING) | compliant (see U-2) | proposal writes are no-clobber (evidence S-3); decision discussion/planning never applies unseen effects and the frozen plan applies only with its exact approval — `task_commands.rs::decision_agent_options_discussion_and_plan_never_apply_unseen_effects` (queue bytes unchanged through `options`/`discuss`/`plan`; `decision apply` invokes no model and requires the exact `--approve` digest); re-running a paused component fails explicitly (evidence S-6). The explicit "awaiting-decision refuses new implementation work" transition is pinned by the re-run refusal and `help.rs` status-transition help, not by a dedicated negative test — U-2 |
| S-9 | Accepting a proposal propagates the decision to durable intent, permits scheduled follow-up, and keeps prior records as historical evidence (REQ-DECISION-SURFACING; CONTRACT "TODOS.yaml MUST gain the tasks needed to implement it and IMPL.md MUST become stale") | compliant (mechanism pinned; see U-1) | `decision_agent_options_...` — a plan whose `disposition` is `resolve` moves the awaiting-decision task to `pending` and exercises the `decision-intent-review` phase before application; the plan JSON schema carries `documents` and `tasks` (propagation surface), and stale-requirement/stale-IMPL fencing is pinned throughout the attempt/repair tests. A plan carrying nonempty new tasks and the explicit post-acceptance `IMPL.md` staleness re-derivation are not exercised end-to-end in the bounded suite — U-1 |
| S-10 | Repair/verification decisions use the same awaiting-decision disposition as initial authoring (CONTRACT "decisions use the same awaiting-decision disposition as initial authoring") | compliant | `resilience_repair_decision_uses_the_initial_awaiting_decision_disposition` (evidence S-1) |

### Dependency requests (REQ-DEPENDENCY-REQUEST + CONTRACT brokered-authoring clause)

| ID | Clause (source) | Classification | Bounded test evidence |
|----|-----------------|----------------|-----------------------|
| S-11 | An in-policy request (exact pinned registry revision, or public VCS origin with exact pinned revision) is recorded and accepted, the agent continues without interruption, and no decision is surfaced (REQ-DEPENDENCY-REQUEST first paragraph, as limited by its recorded delivery limitation; CONTRACT "is recorded and accepted so the agent continues without interruption") | compliant (record-and-continue; fulfillment per M-1) | `workflow.rs::in_policy_dependency_request_continues_without_awaiting_decision` — `serde` at `1.0.200` is not surfaced as a decision (no proposal JSON), the request is durably recorded under `.kvist/authoring/dependencies/` with the crate identity and pinned origin retained, and the task reaches neither `awaiting-decision` nor `completed` |
| S-12 | An out-of-policy request (unpinned/wildcard, private, link-local, loopback, or unverified origin) is never applied automatically, is surfaced as a blocking decision, and places the component in `awaiting-decision` (REQ-DEPENDENCY-REQUEST second paragraph; CONTRACT "a request outside that policy is surfaced as a decision") | compliant | `workflow.rs::out_of_policy_dependency_request_pauses_the_run_in_awaiting_decision` — `serde` at `*` is not recorded as an accepted request, is surfaced as a proposal identifying "not an exact, pinned revision", pauses the run in `awaiting-decision` without completing, and applies no effects; security-audit test evidence: the private-range classifier now covers the full RFC 1918 `172.16.0.0/12` block (`rfc1918_private_ip_origins_are_surfaced_as_a_decision`, boundary addresses pinned) |
| S-13 | The request and its outcome are recorded durably as evidence (REQ-DEPENDENCY-REQUEST) | compliant | in-policy: JSON under `.kvist/authoring/dependencies/` (evidence S-11); out-of-policy: proposal JSON under `.kvist/authoring/proposals/` (evidence S-12); both tests assert retention of the crate identity and origin text in the durable bytes |
| S-14 | The protected model-requested fulfillment path fails explicitly before any provisioning effect, even when unrelated vendoring is ready; no host package-manager fallback; explicit human `kvist vendor` remains separate (REQ-DEPENDENCY-REQUEST delivery limitation; CONTRACT "Protected model-requested dependency fulfillment currently fails explicitly") | compliant | `IMPL.md` "Protected provisioning and verification" — `vendor_project_protected` refuses before inspecting paths, staging, or launching any package manager, and existing ready vendoring is not misrepresented as fulfillment; security-audit evidence — the agent path never reaches the network-allow acquisition sandbox, and the only network-allow sandbox is the human-authorized `kvist vendor` path; `workflow.rs::in_policy_dependency_request_...` — the blocked outcome after the recorded in-policy request is an acquisition-phase failure, not an applied dependency |
| S-15 | The closed, deny-by-default tool funnel routes every untrusted intent to exactly one outcome (read, effect, decision, dependency request, drop), and the advertised tool set equals the allowed set (CONTRACT behavioral guarantee) | compliant with a retained mismatch on the advertised enumeration | security-audit evidence — `classify_intent` is total and fail-closed; `tool_definitions_cover_exactly_the_allowed_set` asserts advertised == allowed in both directions; the brokered-turn compliance record's F-A (retained, unresolved) shows the CONTRACT/DESIGN enumeration text disagrees with the observed enumeration — M-1 |

## Discrepancies (retained for human arbitration)

### M-1 — The closed authoring tool-set enumeration: intent documents disagree with the observed-behavior carrier (mismatched; carries over unresolved from the prior brokered-turn pass)

- **Stable locators:** `CONTRACT.md` Behavioral guarantees (multi-turn clause:
  "The loop advertises the closed authoring tool set (`read_file`,
  `write_file`, `edit_file`, `request_dependency`, and `propose_decision`)");
  `DESIGN.md` "Planned dogfooding execution boundary" (same five-tool list);
  `IMPL.md` observed constraints ("The closed authoring vocabulary is
  `write_file`, `edit_file`, `propose_decision`, and `request_dependency` …
  These tools do not grant arbitrary shell execution or general file
  reading"); `tests/security_audit_proposal_dependency_tools.md` Control 1
  (observed `ALLOWED_TOOLS` is six names including `list_files`).
- **Observed:** the observed-behavior carrier names four tools (excluding
  `read_file` and `list_files`); the retained security-audit evidence names six
  (including both, with an advertised-set-equals-allowed-set test pin); the
  intent documents name five (including `read_file`, excluding `list_files`).
  Three different enumerations coexist for one security-relevant fact, and no
  bounded test independently re-pins the advertised list against the current
  contract text.
- **Assessment:** `mismatched`. This pass cannot record `compliant` for the
  advertised-tool-set clause while the authoritative enumeration is
  unreconciled. This is carried over from the brokered-turn compliance record's
  finding F-A; nothing in the phase-4 tool work (which added
  `propose_decision` and `request_dependency` behavior, not the read tools)
  resolves it.
- **Arbitration options (owner: project architect):** (a) re-derive the
  `IMPL.md` observation to list the actual closed set and state how
  `read_file`/`list_files` are honored, then re-run this pass; (b) if the
  advertised set is intentionally smaller or larger than the intent documents
  state, amend `CONTRACT.md`/`DESIGN.md` and the pinning tests together;
  (c) record a reasoned exception naming the authoritative enumeration.

### M-2 — In-policy `request_dependency`: declared continue-and-acquire path versus observed record-and-refuse (mismatched; retained unmet target, not an approved deferral)

- **Stable locators:** `CONTRACT.md` Behavioral guarantees ("…is recorded and
  accepted so the agent continues without interruption and acquires the
  revision through the build/verification step"); `DESIGN.md` "Planned
  dogfooding execution boundary" ("a request within policy is fetched
  automatically"); `REQUIREMENTS.md` `REQ-DEPENDENCY-REQUEST` (MUST fetch, bind
  lockfile before/after identities, promote an immutable generation, "all
  without human intervention"; delivery limitation: "exact request-to-lock
  identity binding and automatic fulfillment is unavailable … Automatic
  fulfillment is an unmet target requiring arbitration, not an approved
  deferral or a claim of compliance"); `DESIGN.md` phase-4 subsection ("An
  in-policy request … is evaluated, but automatic fulfillment is not
  implemented … The automatic design remains an unmet target for explicit
  arbitration; this note is not a compliance pass"); `IMPL.md` "Protected
  provisioning and verification".
- **Observed:** the broker records an in-policy request durably and the agent
  continues (evidence S-11); automatic fulfillment is refused before any
  provisioning effect (evidence S-14); the bounded in-policy run ends blocked
  by the acquisition phase, not completed. The contract's "acquires the
  revision through the build/verification step" clause therefore describes
  behavior the source-derived record says does not exist.
- **Assessment:** `mismatched`, with the mismatch already self-declared as an
  unmet target in both intent documents. Per the requirement's own wording
  this is not an approved deferral, so it cannot be closed as
  `approved-deferred` without a new arbitration record. The phase-4 change
  (record-and-continue plus out-of-policy surfacing) is implemented and
  tested; the acquisition clause is the residual gap.
- **Arbitration options (owner: project architect):** (a) implement and pin
  exact request-to-lock identity binding and automatic fulfillment, then
  re-run this pass; (b) amend the CONTRACT/DESIGN acquisition clause to state
  record-and-continue with explicit fulfillment refusal (matching observed
  behavior) and pin the refusal; (c) record the explicit approved deferral or
  reasoned exception the requirement text asks for.

## Underspecified / partially evidenced clauses (retained, no fix claimed)

- **U-1 — Acceptance propagation end-to-end.** "TODOS.yaml MUST gain every
  task required to implement the accepted change" and "`IMPL.md` MUST become
  stale" are pinned at the mechanism level (plan `tasks`/`documents` surface,
  `disposition: resolve` moving the task, `decision-intent-review` before
  application, stale-artifact fencing elsewhere), but no bounded test applies a
  decision plan carrying new tasks and then observes the queue gain plus the
  IMPL staleness mark. Classified `compliant (mechanism)`; retained as
  underspecified for end-to-end evidence.
- **U-2 — Awaiting-decision refusal of new implementation work.** The re-run
  of a paused component fails with an explicit diagnostic (evidence S-6) and
  the status transition surface exposes `awaiting-decision` as a fence
  (`help.rs`, `transitions.rs`), but no bounded test drives an explicit new
  implementation attempt (e.g., `task run` of another ready task or a direct
  transition into work) against a paused component. Classified `compliant` on
  the available evidence; retained for a dedicated negative test.
- **U-3 — Worthiness boundary is model-driven.** The non-blocking advisory
  response for trivial matters and the post-hoc `IMPL.md` comparison path are
  observed (evidence S-7), but the boundary "substantially alters the
  implementation and is not already covered by intent" is exercised through a
  single scripted boundary case, not an exhaustive test. No mismatch observed;
  retained as a proof limitation.
- **U-4 — Inference/verification-failure diagnostic brief.** "When inference or
  verification fails, the host MUST retain a clearly labelled diagnostic brief
  from observed errors instead of requiring another model call" is only
  adjacent in the bounded evidence: `decision_brief_write_failure_retains_the_
  original_stop_and_unresolved_attempt` pins that a brief *write* failure
  erases nothing (original stop, unresolved attempt and recovery fences
  retained). A dedicated "inference failure yields a host-rendered diagnostic
  brief without another model call" case is not in the bounded suite.
- **U-5 — Historical briefs versus current state.** "Historical briefs MUST
  remain distinguishable from the current state" is supported by the advisory
  footer on retained briefs ("not human acceptance") and by attempt-named
  retained review files (`one-compliance-review.attempt-0001.md`), but no
  bounded test asserts distinguishability across repeated briefs for the same
  task.
- **U-6 — Observed-behavior carrier conflicts with the remediated security-audit
  evidence.** `IMPL.md`'s scope notes still describe the dependency broker's
  private-origin predicate as prefix-based ("a private numeric host inside an
  HTTPS URL can be classified as accepted"), while the phase-4 security audit
  records the fix (`is_rfc1918_private_ip`, boundary-pinning test) as applied.
  The source-derived record needs re-derivation to reconcile the two; until
  then the origin-policy observation is treated as unconfirmed in both
  directions. (Related: the audit's F2 note that the `127.` prefix
  over-matches is fail-safe and requires no fix; F3, the advisory
  `PackageSources` guard, is an out-of-scope galla observation that only
  affects the human-authorized `kvist vendor` path.)

## Conclusion

The phase-4 decision-surfacing behavior is **compliant** across the tested
surface: a surfaced `propose_decision` or out-of-policy `request_dependency`
pauses the run in `awaiting-decision` before any effect, records bounded
redacted no-clobber evidence under `.kvist/authoring/`, never completes the
task, never writes a protected intent document, and presents a host-annotated
Markdown brief with explicit choice/clarification/rejection/resume
instructions; in-policy dependency requests are recorded durably and the agent
continues. Continuous runs stop at the awaiting-decision point without
auto-resolution.

Two discrepancies are retained for human arbitration, both with stable
locators, intended text, observed behavior, and bounded test evidence above:

1. **M-1 (mismatched):** the closed authoring tool-set enumeration differs
   between `CONTRACT.md`/`DESIGN.md` (five tools), `IMPL.md` (four tools), and
   the retained security-audit observation (six tools). Owner: project
   architect. Options (a)–(c) as recorded.
2. **M-2 (mismatched, self-declared unmet target):** the contract's in-policy
   "continue and acquire through the build/verification step" clause versus
   the observed record-and-refuse behavior. The requirement text explicitly
   withholds compliance certification and asks for arbitration rather than an
   approved deferral. Owner: project architect. Options (a)–(c) as recorded.

Per the retention rules, this record, the fresh `IMPL.md`, and the eventual
arbitration decision must be retained in version control; no requirement,
contract, design, or implementation record was modified by this pass, and no
raw transcripts or temporary review workspaces are retained.
