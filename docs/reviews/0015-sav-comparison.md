# ADR 0015 — Sav diagnostic source-blind comparison

## Scope and status

**DIAGNOSTIC ONLY: not a formal compliance pass, acceptance, receipt, or waiver.**
Only Sav's `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `IMPL.md`, and
`ROOT_CONTRACT.md` were read. No implicit parent exists. No source/test source,
queues, history, prior reviews, peers, commands or delegation. Agreement reflects
the observer's record, not whole-component certification.

## Preconditions and execution evidence

The orchestrator reported a clean committed snapshot before the original comparison.
Nevertheless, `kvist doctor .` reports a **pre-existing** Skott
`lifecycle-write-tests` `completed_at`/`updated_at` inconsistency, invalid root
state/VCS readiness not checked, and an unavailable Kvist-managed toolchain Cargo
path. Host-toolchain builds do not repair these preconditions.

Earlier integrated execution evidence reported workspace tests:
**1743 passed, 0 failed, 14 existing ignored, 104 successful target summaries**,
using `CARGO_TARGET_DIR=/opt/target cargo test --offline --locked --workspace
--no-fail-fast --quiet`. Strict workspace all-target Clippy, formatting,
workspace release binaries, ordinary workspace rustdoc, and strict Sav/Skott
rustdoc passed. Filesystem-only Sav/Skott/Kvist catalogue smoke passed; no real
Copilot/remote inference or qualification occurred.

The caller reports complete Sav package success after the selector change:
257-byte rejection failed before implementation; exact 256-byte acceptance and
invalid-before-connect coverage now pass. These uninspected reports update
IMPL's pending-package statement. The earlier 1743 passes predate that change.
The final post-selector workspace run finished **2026-10-24T00:00:55Z**:
**1746 passed, 0 failed, 14 existing ignored, 104 successful target summaries**.
Post-selector strict workspace all-target Clippy, fmt, release binaries, ordinary
workspace docs, strict Sav/Skott docs and actual filesystem-only three-binary
catalogue checks also passed. Existing maerg strict-whole-workspace rustdoc
warnings remain. See [separately reported execution evidence](0015-validation.md)
(not read here). Doctor's recorded pre-existing blockers remain unchanged.
Aggregate green results resolve none of the findings, uncertainties or
preconditions below and establish no clause-level certification.

## Clause/status/evidence comparison

R/C/D denote Sav requirements/contract/design; I denotes IMPL headings.

| Intended clause or heading | Status | Independent recorded evidence |
| --- | --- | --- |
| R `AR-REQ-SHARED-RUNTIME`; C “Shared registry configuration”; D “Shared registry realization” | Observed agreement | I “Shared registry, setup, and pinned resolution”: tagged backends, whole-definition precedence, fail-closed sources, disabled-selection refusal, source digest, no probes. Snapshot is not executable authentication. |
| Same clauses: capability separation, credential references, private/link-safe persistence | Agreement plus gaps | Same I heading records three distinct capability lists and synchronized atomic writes. Credential rejection is heuristic; ancestor traversal and concurrent updates are not fully protected. Unknown identifiers' policy-enablement semantics and universal inline-secret exclusion are not established. |
| R `AR-REQ-SETUP`; C “Behavioral guarantees” | Observed agreement; real-provider gap | Same I heading and “Representative existing helpers”: shared catalogs, custom fallback, fixed qualification prompt, bounded output, force-after-failure without capability-test promotion. Exact `OK` equality is neither checked nor explicitly required. |
| R `AR-REQ-SHARED-RUNTIME`; C “Shared registry configuration”; D “Algorithms and decisions” | Observed agreement; incompatibility resolved | I “Shared registry…” and “Re-inspected mechanisms”: one nonblank, control-free ASCII, 256-byte validator for native parsing, explicit validation and inference; boundary acceptance and invalid-before-connect checks. Distinct 1024-byte external bounds are not a native incompatibility. |
| R `AR-REQ-SHARED-RUNTIME`, `AR-REQ-NATIVE-RUNTIME`; C “Shared native runtime API”; D “Shared native runtime extraction” | Observed agreement; broader target unresolved | I “Native model/runtime interfaces” describes injected executor/recorder, no implicit fallback, host-neutral loop. It does not establish the full reusable typed broker/selected-backend realization promised elsewhere. |
| R “Shared native runtime verification”; C “Shared native runtime API” | Observed agreement; coverage gaps | I “Native execution, accounting, and controls” and “Test-source coverage…” describe bounded correlated guidance, pre-claim edits, quiet/backoff interruption, provisional discard, skipped-result pairing, final arbitration, shutdown and preserved budgets. Dedicated preparation-failure/boundary and some timing-interleaving tests were not found. |
| R `AR-REQ-SHARED-RUNTIME`: required recording; C runtime API and “Live diagnostic journals and private controls” | Conditional agreement | I “Native execution…” and “Bounded live journal and local IPC”: recording failure stops work; delivery precedes requests; checkpoints synchronize. Optional/no-op hooks leave durability to hosts, not runtime construction. |
| Same observation clauses | Observed agreement; boundary limits | I “Observation and control interfaces” and “Bounded live journal…” describe versioned bounded journals, explicit incomplete/terminal states and private capability controls. Queue acknowledgment is not delivery; trusted callbacks, same-UID exposure and unredacted payloads remain limits. |
| R `AR-REQ-SUPERVISION`; C observed-supervision clause | Agreement plus mismatch | I “Opaque supervision and cancellation”: literal argv, no observed-API retries, fallible observation, cleanup, no tool authority. “Unresolved independent finding” identifies singular global signal ownership allowing concurrent runs to lose forwarding/interrupt state. |

## Material discrepancies and arbitration

1. **Executor wording reconciled:** D “Security and resource design” now agrees
   with C “Shared native runtime API” and I “Native model/runtime interfaces”:
   Sav uses the injected executor; protected hosts bind `galla-runner`.
   The prior ownership contradiction is withdrawn. Protected-host binding and
   broader typed-broker targets remain unverified, not approved deferrals.
2. **Redaction target:** R `AR-REQ-NATIVE-RUNTIME` requires redacted runtime
   events; I “Native execution…” and “Bounded live journal…” record no generic
   secret redaction and full outcomes reaching host recorders. Host ownership
   needs a precise obligation; the target is not demonstrated fulfilled.
3. Retain concurrency and persistence/credential uncertainties for arbitration.
   I's weaker trajectory API leaves R's journal guarantee breadth underspecified.

ROOT “Change and compliance rules” requires arbitration and separate verification.
Neither intent nor IMPL is changed; no promotion, approval, or waiver is granted.
