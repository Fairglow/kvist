# Skott ADR 0015 and prompt-steering diagnostic comparison

## Scope

NEW source-blind comparison of only Skott's REQUIREMENTS, CONTRACT, DESIGN,
independently derived IMPL and ROOT_CONTRACT. No implicit parent contract exists.
No other artifacts, source, commands, tests or delegation were used.
**Diagnostic only:** not formal compliance, acceptance, receipt, waiver or
whole-component certification.

R/C/D/I denote those Skott documents; Root denotes ROOT_CONTRACT.
Agreement concerns IMPL's account, not this reviewer's implementation inspection.
Execution results are caller-supplied.
Bounded follow-up reread the changed submission paragraphs; source/IMPL are unchanged.

## Clause/status/evidence

| Intended clauses/headings | Status | IMPL/evidence comparison |
| --- | --- | --- |
| R “Shared registry configuration acceptance (ADR 0015)”; C “Shared registry configuration (ADR 0015)”; D “Shared registry realization (ADR 0015)” | Observed agreement; evidence gap | I “Configuration and shared profiles”: references, Sav validation, duplicate rejection, pinning and configuration-free setup agree. Delegated qualification, enablement and setup authority/billing disclosures are not independently established. |
| R RUN-REQ-MODEL-SELECTION; C “Model selection and activation” | Observed agreement; underspecification | I “Configuration and shared profiles”: native-only local defaults reject invalid bindings/duplicate defaults, yield to explicit/active choices, never auto-select external profiles; headless skips discovery. Multiple active matches use configuration order, an unspecified tie-break. Sole-model precedence is textually different but cannot change the selected model. |
| R RUN-REQ-SHARED-RUNTIME; D “Shared runtime steering delivery (ADR 0015)” | Observed agreement; evidence gap | I “Shared native runtime and real steering paths”: Sav adapters retain Skott-owned tools, recording, errors and presentation. Shared frontend mechanisms do not independently verify delegated algorithms. |
| R RUN-REQ-PROMPT-STEERING; C “Prompt steering” | Observed agreement; evidence gap | I “Shared native runtime and real steering paths”: mailbox classification preserves owner/context, restores rejected drafts and prevents selection changes replacing active work. Actual-entrypoint coverage does not establish every ordering, backoff, tool-boundary, shutdown and budget-continuity target. |
| C “Optional private execution controls”; D “Optional control socket ownership” | Observed agreement; evidence gap | I “Independent inspection and explicit controls”: opt-in sandbox/private-log guards, operator-only capability, shared mailbox/token and cleanup agree. Embedding obligations are explicitly caller-owned; socket internals remain unobserved. |
| R RUN-REQ-SHARED-RUNTIME; C “Independent diagnostic inspection”; D “Internal structure” | Observed agreement; underspecification | I “Independent inspection and explicit controls”: bounded configuration/tool-free attachment and non-cancelling detachment agree. Journal/socket identity is not cross-validated; association guarantees are unspecified. Caller PTY smoke exercised g/Enter guidance, c cancellation and q detachment through correlated exchanges without journal changes. |
| C “Independent diagnostic inspection”; Root “Non-negotiable architecture” | Observed agreement; qualification gap | I “Process execution and opaque external backends”: terminal-owned HOST UNCONFINED acknowledgement, mandatory logs, single supervision, no retry/headless/native overrides or invented steering agree. Internal permissions/tools/network remain opaque; real-agent qualification is absent. |
| R RUN-REQ-LIFECYCLE, RUN-REQ-HEADLESS; C “Runtime hardening extension” | Observed agreement; bounded assurance | I “Recording lifecycle and confidentiality”: synchronized dispatch, per-prompt journals/dispositions and held-directory/no-clobber recording agree. Records may contain secrets; unknown effects are not replayable. Ordinary interactive logs remain workspace-accessible; required/control-enabled logs require stronger separation. |

## Clarification and discrepancies retained for arbitration

- **Prior wording conflict resolved:** C “Behavioral guarantees” and
  D “Interactions and state” now restrict replacement to idle/bootstrap
  submissions. Active submissions preserve the worker as guidance; changed
  selections apply later. This agrees with C “Prompt steering” and I “Shared
  native runtime and real steering paths.” The clarification resolves the
  intent conflict, not outstanding execution-evidence gaps.
- **Root authority mismatch:** Root “Non-negotiable architecture” prohibits
  agent writes to intent, queues and evidence. R “Quality requirements and
  constraints” and C “Runtime hardening extension” grant a fully writable
  workspace; I “Observed requirements and constraints” reports no
  protected-artifact exclusion. Authority disclaimers do not resolve this conflict.
- **Other material allowed-clause mismatches:** I “Observed requirements and
  constraints” reports a 180-second preparation bound versus R
  RUN-REQ-RELIABILITY/D “Tool-chain advertisement and gating” at 30 seconds.
  I “F4 — Medium: long native host shell commands lose their original script”
  conflicts with R RUN-REQ-RELIABILITY's executable command bound.
  R RUN-REQ-REVIEW requires default mutation disabled, but C “Independent review
  extension” retains writable shell authority. These are unresolved targets or
  intent conflicts, not approved deferrals.

## Limits and later execution

Earlier caller evidence was 1743 passing tests. The final post-selector complete
workspace run finished **2026-10-24T00:00:55Z: 1746 passed, zero failed, 14 existing
ignores, 104 successful target summaries**. Caller reports all post-selector
strict workspace all-target Clippy, fmt, release binaries, ordinary workspace
docs, strict Sav/Skott docs and actual filesystem-only three-binary catalogue
checks passed; earlier catalogue smoke found one external row without false
empty advice. Existing maerg strict-whole-workspace rustdoc warnings remain.
See [separately reported execution evidence](0015-validation.md), not read here.
These supplement I's dated limits; aggregate green results resolve none of the
independent findings, uncertainties or preconditions. No real Copilot/remote
inference or qualification occurred. The integrated rerun is no longer pending.

Caller verified a clean committed snapshot but reports pre-existing Skott
lifecycle-write-tests completed_at/updated_at inconsistency, invalid root
state/VCS readiness not checked, and unavailable Kvist-managed toolchain cargo
path. Host gates repair none. No queue/toolchain inspection occurred.
Discrepancies remain for human arbitration under Root “Change and compliance rules.”
