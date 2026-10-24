# ADR 0015 — Maerg diagnostic intent/evidence comparison

## Scope and preconditions

Read only maerg `REQUIREMENTS.md` (R),
`CONTRACT.md` (C), `DESIGN.md` (D), source-derived `IMPL.md` (I), and
`ROOT_CONTRACT.md` (Root); no implicit parent contract. No source,
tests, queues, history, earlier reviews, peers or execution.
**Diagnostic only: not formal compliance, acceptance, a receipt,
a waiver, or whole-maerg certification.**

Caller verified a clean committed snapshot, but reports **pre-existing**
`kvist doctor .` Skott lifecycle-write-tests `completed_at`/`updated_at`
inconsistency, invalid root state/VCS readiness not checked, and unavailable
Kvist-managed toolchain cargo path. Workflow readiness is not established;
host-toolchain validation repairs none of these. Not independently investigated.

## Clause/status/evidence comparison

I headings identify reported observations.

| Intent clause/headings | Status | I heading and diagnostic evidence |
| --- | --- | --- |
| R **Shared registry configuration acceptance (ADR 0015)**, **REQ-SHARED-RUNTIME**; C **Shared registry configuration (ADR 0015)**; D **Shared registry realization (ADR 0015)** | Observed agreement; evidence gap | **Shared selection and pinned authority**: typed snapshots, source digests, approval binding, native-only protected selection and external refusal. Registry/qualification internals and exact registry-byte digest coverage remain unestablished. |
| R **REQ-EXECUTION-BOUNDARY**, **REQ-AGENT-INTEGRATION**; Root **Non-negotiable architecture** | Observed agreement; evidence gap | **Conversation and effect ownership**: host transport, broker authorization, pinned runner/policy, sandbox effects—not host agent-command execution. **Remaining observed scope and workflow limits** excludes delegated transport/OS enforcement. |
| C **Live task diagnostics and controls**; D **Implemented proposal and dependency-request tools (phase 4)** | Observed agreement; mismatch elsewhere | **Observed requirements and constraints**: five local artifacts, exact root/parent mounts, bounded no-follow UTF-8 reads, retained context, explicit overflow refusal—not general reading. |
| R **REQ-MULTI-TURN-EXECUTION**; C **Live task diagnostics and controls**, **Behavioral guarantees** | Observed agreement; evidence gap | **Observed requirements and constraints**, **Conversation and effect ownership**: shared time/generation budgets, cumulative reservations, conservative interrupted/missing-usage charges, retained known totals—not exact billing. |
| R **REQ-SHARED-RUNTIME**; C **Live task diagnostics and controls**; D **Internal structure** | Observed agreement; evidence gap/underspecification | **Observation and explicit controls**, **Redaction and live records**: required live events, default-off controls, pinned context, supplemental guidance. **Remaining observed scope and workflow limits** excludes delegated synchronization/races; controls exclude verification. Clarify “protected run” scope. |
| C **Live task diagnostics and controls**, **Errors and failure semantics** | Observed agreement; evidence gap | **Steering, cancellation, and retained outcomes**: pre-dispatch checks, uncertain effects, nontransactional writes, error/cancellation finalization attempts. Initialization/storage/crashes can leave incomplete evidence; delegated process-group cancellation remains unestablished. |
| R **REQ-DEPENDENCY-REQUEST**; C **Live task diagnostics and controls**; D **Implemented proposal and dependency-request tools (phase 4)** | Unmet target; observed fail-closed agreement | **Protected provisioning and verification**: fulfillment refusal, no host fallback, separate readiness inspection. Automatic exact fulfillment remains unmet, not an approved deferral. |
| R **REQ-SUPERVISED-EXECUTION**, **REQ-ACCEPTED-VCS-COMMIT**; Root **Change and compliance rules** | Partial agreement; mismatch | **Command and workflow surface**: implementation success needs human disposition; other kinds can complete directly. **Remaining observed scope and workflow limits**: incomplete accepted-change enumeration. |

## Important discrepancies and human arbitration

- **Read/write scope:** R **REQ-AGENT-READ-SCOPE**, R **Quality requirements and constraints**
  and C **Behavioral guarantees** claim whole-project bounded reading; I
  **Observed requirements and constraints** reports no general read tool.
  R **REQ-COMPONENT-WRITING-SCOPE** and C promise whole-component-minus-exclusions
  writes; I reports `src/`/`tests/`. D **Whole-component write scope
  (phase 4b, deferred)** does not waive R/C.
  Root **Non-negotiable architecture** also constrains context; reconcile
  authorized reads versus implicit prompt propagation.
- **Conflicting behavior descriptions:** C **Behavioral guarantees** and D
  **Planned multi-turn agent execution** describe single-turn current behavior,
  `read_file`, nonfatal dropped intents and final-effect-only persistence.
  I **Conversation and effect ownership** instead describes a multi-turn,
  four-tool loop and sequential effects. C also says Bubblewrap is not
  integrated. Arbitrate contradictory current/target claims.
- **Dependency classification:** I **Remaining observed scope and workflow limits**
  reports private HTTPS numeric origins accepted, contrary to C
  **Behavioral guarantees** and D's phase-4 origin policy. Refusal prevents
  fetch, not the classification mismatch.
- **Authority/evidence:** arbitrate separate human finalization, omitted
  source/nested/deletion acceptance changes and missing task-level error records.
  Lower-level retention is not complete workflow evidence. I **Redaction and
  live records** distinguishes profile/environment-only metadata filtering;
  universal secrecy under C **Security and authority** remains unestablished.

## Execution evidence and limits

Final caller-reported post-selector execution evidence, separate from I,
completed **2026-10-24T00:00:55Z**; see
[0015-validation.md](0015-validation.md) (not inspected):
`CARGO_TARGET_DIR=/opt/target cargo test --offline --locked --workspace --no-fail-fast --quiet`
passed: 1,746 tests, zero failed, 14 existing ignored, 104 successful target summaries.
Strict workspace all-target Clippy, formatting, workspace release binaries,
ordinary workspace rustdoc and strict Sav/Skott rustdoc passed. Maerg
strict-rustdoc warnings preclude claiming a strict whole-workspace pass.
Actual filesystem-only three-binary shared-catalogue checks passed; no real
Copilot/remote qualification or inference. I predates these gates, labeling older
evidence. Aggregate success proves neither individual clauses, races, crash
durability nor remote services. Targets remain for arbitration; intent/I unchanged.
