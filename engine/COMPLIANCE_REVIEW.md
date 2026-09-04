<!-- kvist-compliance-review-version: 1 -->
# Component Compliance Review

> Historical evidence notice: this review predates the one-way move from the
> root component at `src/` to `engine/`. Its original review boundary is
> retained, and this report does not certify the migrated layout.

## Review boundary and independence

This is an independent, source-blind compliance review of the root component
`src/`. The reviewer did not implement or document this work. Only the
permitted artifacts were read: `src/REQUIREMENTS.md`, `src/CONTRACT.md`,
`src/DESIGN.md`, the newly generated `src/IMPL.md`, and `ROOT_CONTRACT.md`.
Source, tests, manifests, child private artifacts, Git history, TODO queues,
prior compliance reports, reviews, and architecture/vision were not read.

Focus: consumer-visible behavior for **assisted model setup** (`agent setup`
and the model-selection aspects of `prompt` under REQ-AGENT-INTEGRATION), with
an additional pass over overall record consistency.

## Bounded evidence relied upon

The following execution evidence was supplied and treated as bounded, external
corroboration (not read from source):

- `cargo fmt`/`check`/`clippy` passed for the workspace, all targets, all
  features.
- Workspace all-feature tests passed: 101 tests, 0 failures.
- Targeted root wizard tests passed, including: provider catalog model choice
  flowing into all selected roles with exact command-byte preservation, JSON
  setup stdout isolation, and qualification failure/force/cancellation paths.
- Installed Copilot/Gemini live catalog probes passed in the child runtime.

`agent-runtime` is an opaque child dependency; several assisted-setup behaviors
are contractually delegated to it and are therefore not directly observable in
the root `IMPL.md`. Where the root record correctly attributes a behavior to
the opaque runtime and the bounded evidence corroborates it, the claim is
treated as compliant-by-delegation rather than a root defect.

## Method

Each assisted-model-setup obligation in `REQUIREMENTS.md` (REQ-AGENT-INTEGRATION),
`CONTRACT.md` (`agent setup`, `prompt` model selection), and `DESIGN.md` (agent
setup state machine) was compared against the observed behavior in `IMPL.md`
and the bounded evidence. Each item is classified as **compliant**,
**mismatched**, **approved-deferred**, or **underspecified**.

## Assisted model setup findings

### C-1 Mandatory fixed-prompt qualification — compliant (delegated)

- Locator: REQUIREMENTS §REQ-AGENT-INTEGRATION ("automatically qualify ... with
  the fixed minimal prompt `Reply with exactly: OK`"); CONTRACT `agent setup`
  ("always runs the generated provider command with the fixed prompt ... before
  role configuration is persisted"); DESIGN ("delegates ... mandatory
  qualification to the runtime").
- Observed: IMPL `agent setup` collects the provider profile through the opaque
  runtime; DESIGN attributes qualification to the child runtime. The exact
  prompt bytes are not observable at the root boundary. Bounded evidence
  confirms qualification runs (wizard qualification failure/force/cancellation
  tests; live catalog probes in the child runtime).
- Severity: none (compliant). The exact fixed-prompt string is a child-owned
  detail; see U-1 for the observability note.

### C-2 Catalog-first, custom-entry-last model choice — compliant (delegated)

- Locator: REQUIREMENTS §REQ-AGENT-INTEGRATION ("MUST first present bounded
  provider-advertised model choices ... manual model entry available only
  through an explicit final custom choice"); CONTRACT `agent setup`; DESIGN
  ("runtime returns numbered provider model choices with a final custom-entry
  escape hatch").
- Observed: IMPL treats catalog and selection as child-owned ("collects a
  provider profile through the opaque runtime"). Bounded evidence confirms
  provider catalog model choice flowing into all selected roles and passing
  Copilot/Gemini live catalog probes in the child runtime.
- Severity: none (compliant); ordering guarantee is child-owned (see U-2).

### C-3 Exact selected-command-byte preservation into roles — compliant

- Locator: CONTRACT `agent setup` ("stores that exact selected command for the
  requested roles"; "does not parse or reinterpret provider catalog
  descriptors"); DESIGN ("preserving its command bytes during role
  materialization").
- Observed: IMPL `agent setup` assigns the model to developer, architect,
  security-reviewer, or all roles, updating/appending matching model entries
  while preserving the rest of the TOML document and validating the result.
  Bounded evidence explicitly covers "exact command-byte preservation" across
  all selected roles.
- Severity: none (compliant).

### C-4 Failed-qualification persistence gate and `--force` override — compliant (delegated)

- Locator: REQUIREMENTS §REQ-AGENT-INTEGRATION ("Failed qualification MUST
  prevent configuration persistence unless `--force`; that override MAY persist
  the failed profile only after displaying an explicit warning and MUST NOT
  override cancellation"); CONTRACT `agent setup`; DESIGN ("failed qualification
  exits before those steps unless `--force`"; "cancellation remains terminal";
  "setup never offers an interactive save-after-failure bypass").
- Observed: IMPL states `--force` is passed only to opaque-runtime profile
  collection "as the root-observed failed-qualification override." Bounded
  evidence covers qualification failure, force, and cancellation paths.
- Severity: none (compliant). The "explicit warning" surface is child-owned
  (see U-3).

### C-5 JSON-mode stream isolation — compliant

- Locator: REQUIREMENTS §REQ-AGENT-INTEGRATION ("setup prompts and status MUST
  be written to standard error, qualification output MUST NOT be forwarded, and
  standard output MUST contain exactly one valid result object"); CONTRACT
  `agent setup`; DESIGN ("wizard receives standard error as its interaction
  writer ... leaving standard output exclusively for the dispatcher's single
  result object").
- Observed: IMPL `agent setup` — "JSON mode sends wizard interaction to stderr
  so stdout remains machine-readable." Bounded evidence includes a passing JSON
  setup stdout-isolation test.
- Severity: none (compliant). Suppression of qualification-command output is
  bounded-capture behavior in the child (see U-4).

### C-6 Acknowledgement scope limited; no later prompt authorization — compliant

- Locator: REQUIREMENTS §REQ-AGENT-INTEGRATION ("Invoking setup MUST count as
  acknowledgement for those bounded discovery commands and the qualification
  command only and MUST NOT authorize later host prompt execution"); CONTRACT
  `agent setup` and `prompt` ("host execution requires
  `--allow-host-execution`").
- Observed: IMPL `prompt` still "Requires `--allow-host-execution`; without it
  the opaque runtime supplies the error," so a later prompt is not authorized by
  setup. Consistent with the stated scope.
- Severity: none (compliant); explicit acknowledgement scope wording is not
  restated at the root boundary (see U-5).

### C-7 Saved reusable-profile path skips qualification — compliant

- Locator: CONTRACT `agent setup` ("A setup invocation that binds an already
  saved reusable runtime profile does not generate or execute a new
  qualification command"); DESIGN ("Selecting an already saved reusable runtime
  profile skips provider collection and qualification").
- Observed: IMPL `agent setup` distinguishes the two paths ("Either collects a
  provider profile through the opaque runtime or loads a named saved runtime
  profile"); test evidence notes standalone-profile materialization. The skip is
  not explicitly asserted at the root boundary (see U-6) but is not
  contradicted.
- Severity: none (compliant).

### C-8 `prompt` model selection within role and typed reasoning effort — compliant

- Locator: REQUIREMENTS §REQ-AGENT-INTEGRATION ("Explicit prompts MUST allow a
  configured model to be selected within the chosen role and MAY apply a typed
  reasoning effort only when the selected model command declares support");
  CONTRACT `prompt` ("Model selection is limited to the configured models for
  the selected role"; reasoning effort "fails if the selected command lacks an
  explicit `{reasoning_effort}` placeholder").
- Observed: IMPL `prompt` selects role configuration, supports an optional model
  and typed reasoning effort; detailed acquisition/rendering is delegated to the
  opaque runtime. Workspace test evidence covers typed reasoning efforts. The
  "limited to configured models for the role" and placeholder-gated failure are
  not explicitly restated at the root boundary (see U-7) but are consistent with
  delegation.
- Severity: none (compliant).

## Overall record-consistency findings

### C-9 Clean-slate derivation attested — compliant

- Locator: REQUIREMENTS §REQ-COMPLIANCE; ROOT_CONTRACT ("A clean-slate
  documenter ... must verify implemented behavior").
- Observed: IMPL "Evidence boundary" attests derivation from implementation,
  tests, and the manifest only, excluding intent, architecture, prior records,
  reviews, and version-control evidence; `agent_runtime/` treated as opaque.

### C-10 Target workflows correctly not claimed as implemented — compliant

- Locator: REQUIREMENTS (advisory review, observed-intent proposal,
  contract-verification are "target requirements, not claims about the current
  CLI"); CONTRACT/ROOT_CONTRACT (`component accept` "structural validation and
  revision recording only").
- Observed: IMPL `component accept` records revisions and clears staleness
  without spawning agents or network I/O and does not modify Markdown; IMPL does
  not claim review receipts, `propose intent`, or contract-clause traceability.
  Consistent with the pre-release enforcement note.

### C-11 Platform, edition, and safety constraints — compliant

- Locator: REQUIREMENTS quality constraints (edition 2024, no unsafe,
  Linux-only executable support); ROOT_CONTRACT.
- Observed: IMPL reports edition 2024, `unsafe` forbidden in the library,
  compile-time non-Linux failure, and Linux-gated executable surface.

## Discrepancy register

No **mismatched** items were found within the assisted-model-setup scope. All
open items are **underspecified** at the root observation boundary because the
behavior is contractually delegated to the opaque `agent-runtime` child and is
corroborated only by bounded child/wizard evidence. None are compliance
failures; each is recorded for human awareness.

| ID | Stable locator | Intent | Observed IMPL / evidence | Class | Severity | Owner |
| --- | --- | --- | --- | --- | --- | --- |
| U-1 | REQ-AGENT-INTEGRATION; CONTRACT `agent setup` | Fixed prompt is exactly `Reply with exactly: OK` | IMPL delegates qualification to opaque runtime; exact bytes not observable at root; wizard qualification tests + live probes pass | underspecified | info | clean-slate documenter (root record scope) / child `agent-runtime` |
| U-2 | REQ-AGENT-INTEGRATION; DESIGN | Catalog presented first; manual entry only via final custom choice | IMPL: catalog is child-owned; evidence shows catalog choice flowing to roles + live catalog probes pass | underspecified | info | child `agent-runtime` |
| U-3 | REQ-AGENT-INTEGRATION; CONTRACT | `--force` persists failed profile only after an explicit visible warning | IMPL: `--force` passed to runtime as override; warning surface not observable at root; force/cancellation tested | underspecified | info | child `agent-runtime` |
| U-4 | REQ-AGENT-INTEGRATION; DESIGN | Qualification-command output MUST NOT be forwarded in JSON mode | IMPL: JSON wizard interaction to stderr; qualification bounded-capture is child-owned; JSON stdout-isolation test passes | underspecified | info | child `agent-runtime` |
| U-5 | REQ-AGENT-INTEGRATION | Setup acknowledgement covers discovery + qualification only, not later prompt | IMPL: later `prompt` still requires `--allow-host-execution`; explicit scope wording not restated at root | underspecified | info | clean-slate documenter (root record scope) |
| U-6 | CONTRACT `agent setup`; DESIGN | Binding a saved reusable profile skips qualification | IMPL distinguishes the two paths but does not explicitly assert the skip; standalone-profile materialization tested | underspecified | info | clean-slate documenter (root record scope) |
| U-7 | CONTRACT `prompt` | Model selection limited to configured models for the role; reasoning effort fails without `{reasoning_effort}` placeholder | IMPL: optional model + typed reasoning effort, delegated rendering; restriction/placeholder-gate not restated at root; typed-effort tests pass | underspecified | info | clean-slate documenter (root record scope) |

## Conclusion

Every assisted-model-setup requirement, contract clause, and design claim in
scope is satisfied by the observed record or by contractually-delegated child
behavior corroborated by the supplied bounded evidence (clean fmt/check/clippy,
101/0 workspace tests, targeted wizard tests covering catalog-to-role
byte-preserving selection, JSON stdout isolation, and
qualification/force/cancellation, plus passing Copilot/Gemini live catalog
probes in the child runtime). No **mismatched** or **approved-deferred**
discrepancies were identified in scope. The seven **underspecified** items are
root-boundary observability limits for behavior owned by the opaque
`agent-runtime` child; they are informational and do not indicate
noncompliance.

**Promotion is NOT blocked** on compliance grounds for the assisted-model-setup
scope. Recommended (non-blocking) follow-up: if a future documentation pass can
observe the child boundary, tighten the root `IMPL.md` and/or child record to
make U-1 through U-7 independently verifiable. These findings are retained for
human arbitration and were not resolved by altering any intent or
implementation artifact.
