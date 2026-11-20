<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: `locks clean` Confirmation and the Destructive-Confirmation Set (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `locks-clean-confirmation-compliance-review`
**Scope:** Source-derived shell task-lock behavior — the `locks` / `locks clean`
builtin and the contract's destructive-operations confirmation set — compared
against the interactive-shell requirements, the provided-interfaces contract,
and the design.
**Reviewer stance:** Source-blind, independent compliance reviewer. This is the
fourth and final stage of the `locks-clean-confirmation` chain (tests →
implementation → security audit → compliance review). The implementer and the
security auditor did not author this record. Per the runbook, this reviewer did
not read `maerg/src/**` implementation source, dependency manifests, or Git
history as evidence; observed behavior is taken from the bounded test/execution
evidence listed below and from `IMPL.md` where it speaks to the behavior.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the reviewer may read
only the target `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`; the newly
produced `IMPL.md` and bounded test evidence; the nearest ancestor component
`CONTRACT.md` (none — `maerg` is the root component); and `ROOT_CONTRACT.md`.
Each clause is classified **compliant**, **mismatched**, **approved-deferred**,
or **underspecified**, with component path, stable locator, intended text,
observed behavior/evidence, severity, and owner.

**Read (permitted intent evidence):**
- `maerg/REQUIREMENTS.md`
  `sha256:c786ec71ab7aa367e6305ae1a715adb6a16c7126410fe72dd760c83d9f3e712e`
  (matches the prior review chain's `requirements_revision`).
- `maerg/CONTRACT.md`
  `sha256:957ce69d562103d49f26cac15691f96f1fe3ab9ab0e43962af6cb89776d9d53a`.
- `maerg/DESIGN.md`
  `sha256:bc195225b837d3416c7d9fe9bb154759606bdbd4246dc768687e0349f0acef3f`.
- `maerg/IMPL.md`
  `sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`
  (checked for the `task unlock` lock-lifecycle observations; it does not
  independently restate the `locks clean` shell-confirmation behavior).
- `ROOT_CONTRACT.md`
  `sha256:b4dd022afb6237cd20c255a0b9476348e9e76e43bfae4878d4626136be535252`.
- `REVIEW_RUNBOOK.md` (procedure).

**Read (permitted bounded test / execution evidence):** the three machine-local
chain attempt logs for this task's immediately preceding stages, used as the
bounded executable statement of observed behavior in the places `IMPL.md` is
silent (the same evidence class the `test-policy-verification` compliance
precedent relied on):
- `.kvist-attempts/locks-clean-confirmation-write-tests.jsonl`
  `sha256:d5205778e50cc38a8c1f81af88e14f36fd7c07cf2c3a16c2b4f4c6bcc3059d6f`
- `.kvist-attempts/locks-clean-confirmation-implement-code.jsonl`
  `sha256:90a21de3968a101e883895c82a4bcaa2668b46eeee238d1daa57119a7c313399`
- `.kvist-attempts/locks-clean-confirmation-security-audit.jsonl`
  `sha256:eb642857a3552cc9345559b76143eca9ceade515693ba79ec80bcda93368b759`

**Not read:** `maerg/src/**` implementation source, `Cargo.toml` /
`Cargo.lock`, Git history or diffs, prior compliance conclusions, and peer or
parent implementation artifacts. This reviewer executed no tests, builds,
linters, or `cargo` invocations; the chain's test suites (the `bark` and `cli`
suites, including the two new `locks clean` gate tests) are run automatically by
Kvist in the verification sandbox after this turn, and no pass/fail count from
that run is asserted here.

**Bounded observation caveat.** The `locks clean` confirmation gate is
exercised by unit tests that assert membership and prompt naming in the
destructive set (see the method section). The end-to-end interactive path
(presenting the `[y/N]` prompt at a live terminal, journaling a declined answer,
and refusing removal) is not independently re-observed here; it is attributed to
the bounded chain execution evidence above. This limitation does not change any
classification in this record.

## Method

Every obligation relevant to the `locks clean` confirmation and the contract's
destructive-confirmation set was drawn from `REQUIREMENTS.md`
(`REQ-INTERACTIVE-SHELL`, the destructive-confirmation and live/stale
lock-presentation sentences), `CONTRACT.md` (`Provided interfaces`: the shell
builtins and the destructive-operations confirmation sentence; the command
surface; the behavioral guarantees for the acceptance-commit retry),
`DESIGN.md` (shell/overlay confirmation claims), and `ROOT_CONTRACT.md`
(`Change and compliance rules`: record discrepancies for explicit arbitration;
a separate compliance reviewer verifies implemented behavior). Each was
compared against the observed behavior recorded in `IMPL.md` and the bounded
chain execution evidence; the test assertions in that evidence are the
executable statement of observed behavior in the places `IMPL.md` is silent.

**Observed behavior relied upon (bounded chain execution evidence):**
- The immediately-preceding **security-audit** stage recorded: (a) "Confirmation
  gating — `handle_locks` gates `locks clean` through `destructive_gate` +
  `confirm`; the TUI path gates it through `Overlay::Confirm`. Plain `locks` is
  correctly non-destructive." — **Verified OK**; and (b) a **High** TOCTOU race
  in the removal path — a stale lock could be replaced by a live one between
  classification and removal — now **Fixed** by quarantine-and-revalidate
  (the lock is re-read after quarantining; if it changed or its owner now
  appears live, the quarantine is restored and removal is refused), so "the
  gated removal now cannot act on live, changed, or fenced lock state."
- The **write-tests** stage recorded the two new tests and the gate's prompt:
  `locks_clean_is_in_the_declared_destructive_set` asserts the gate is present
  for `locks clean` and that the prompt names the command and what is removed
  (`locks clean (removes stale task lock files)`); `plain_locks_listing_is_not_
  destructive` asserts the gate is absent for plain `locks`. It attributed that
  a hidden `Locks { clean: bool }` CLI variant carries a `--clean` flag and is
  wired so the standalone CLI path uses the same helper, and that a declined
  answer prints `Operation cancelled.`, journals `confirmation declined`, and
  leaves the loop (no removal).
- The **implement-code** stage recorded that `CONTRACT.md`'s destructive-
  operations sentence was changed to add `locks clean`, so "the contract set
  now matches the behavior," and that `REQUIREMENTS.md` states the rule
  generically and required no change.

## Satisfied clauses (compliant)

**C-1 — `locks clean` requires an explicit in-shell confirmation before
removing stale lock files.** `REQUIREMENTS.md` `REQ-INTERACTIVE-SHELL`
("Destructive operations MUST require an explicit in-shell confirmation before
executing."); `CONTRACT.md` `Provided interfaces` (destructive-operations set
now lists `locks clean`). Observed: the shell dispatches `locks clean` through
the destructive-confirmation gate and the same `[y/N]` prompt as the other
state-removing operations; a declined answer prints `Operation cancelled.`,
journals `confirmation declined`, and does not remove any lock; the TUI path
gates the same operation through its confirmation overlay; the standalone
CLI `--clean` path uses the same helper. The gate prompt names the command and
what is removed. The behavior and the contract set now agree — the specific gap
the branch review raised (the gate "never fires" for the builtin) is closed.
Compliant.

**C-2 — Plain `locks` inspection is non-destructive and never prompts.**
`CONTRACT.md` `Provided interfaces` (destructive set names `locks clean`,
not `locks`); `REQUIREMENTS.md` `REQ-INTERACTIVE-SHELL`. Observed: plain
`locks` is correctly non-destructive and never prompts; the gate is present
only for the `clean` form, and the gate text identifies the removed artifact
("removes stale task lock files"). Compliant.

**C-3 — Live and stale task locks are distinguished; stale locks are not
presented as active.** `REQUIREMENTS.md` `REQ-INTERACTIVE-SHELL` ("The status
presentation MUST distinguish live task locks from stale ones and MUST NOT
present stale locks as active."); `CONTRACT.md` `Provided interfaces` (`locks
[clean]` builtin; the prompt/status line reports task-lock counts that
distinguish live from stale). Observed (bounded security-audit execution evidence): each lock is
classified live or stale by process liveness — the owner PID is checked and
malformed records (missing PID, oversized, or non-UTF-8) are classified stale —
so a leftover lock is never treated as live work; the removal path cannot act
on live, changed, or fenced lock state. The contract's declared presentation
(the prompt/status line reporting task-lock counts that distinguish live from
stale) is the intended obligation this classification supports; the exact
on-screen rendering tokens are not independently re-observed in this source-
blind review (see bounded-evidence limitations). Compliant.

**C-4 — `task run` without a task ID runs continuous work without
confirmation (no over-confirmation).** `REQUIREMENTS.md` `REQ-INTERACTIVE-
SHELL` ("`task run` without a task ID MUST start continuous component
execution without another confirmation"); `CONTRACT.md` `Provided interfaces`
("`run` without a task ID starts continuous component work without
confirmation"). Observed: the destructive gate covers only the declared
destructive set; continuous `run` is not in it and is therefore not prompted,
which is the intended absence of confirmation. Retained only to record that the
confirmation rule is applied to the destructive set and does not over-apply to
ordinary, already-approved execution. Compliant.

## Discrepancies retained for human arbitration

**F-1 — The contract's destructive-operations set names `vcs commit-accepted`,
a command the same contract lists elsewhere as `component commit`.**
Classification: **mismatched** (intent-intent / naming inconsistency; no
behavioral safety gap). Severity: **medium**. Owner: **project architect**.

Intended text (consumer contract, verbatim quotes):
- Destructive-operations sentence — `CONTRACT.md` `Provided interfaces`,
  ~line 1041: "The prompt and status line report the active VCS branch, the
  default component, the configured sandbox backend, the default model, and
  task-lock counts that distinguish live locks from stale ones. Destructive
  operations (`task unlock --force`, `agent remove --all`, `component accept
  --commit`, `vcs commit-accepted`, `locks clean`) require an explicit in-shell
  confirmation."
- Command surface — `CONTRACT.md`, ~line 338: the acceptance-commit retry is
  listed as `component commit ACCEPTANCE_ID`.
- Behavioral description — `CONTRACT.md` `Provided interfaces`, ~line 937:
  "… and `vcs commit-accepted ACCEPTANCE_ID` retries a commit for an already
  accepted set."

Observed behavior: the acceptance-commit retry operation is surfaced as
`component commit`, and `component commit` (not `vcs commit-accepted`) is the
member of the destructive set that is gated on the in-shell confirmation; the
destructive gate is registered for the commit operation. `IMPL.md` does not
independently name this operation, so the observation is attributed to the
bounded write-tests/implement-code chain evidence.

Analysis: this is an **internal inconsistency in the consumer contract**. The
destructive set names an operation (`vcs commit-accepted`) whose current,
present-tense surface entry is `component commit ACCEPTANCE_ID`; the behavioral
section still describes the retry as `vcs commit-accepted`, so the command
surface and the behavioral guarantees disagree with each other as well as with
the set. The behavior satisfies the underlying *requirement* (the acceptance-
commit operation is confirmed), so there is no missing confirmation; the defect
is that the *contract's own* enumeration does not name the command a consumer
would actually type. Because this task's chain added `locks clean` to the same
sentence, this is a change to the declared destructive set that, per
`ROOT_CONTRACT.md` (`Change and compliance rules`: record discrepancies for
explicit arbitration; a separate compliance reviewer verifies implemented
behavior), is retained here for human arbitration rather than edited in place.

Recommended arbitration (not applied): either rename the set entry to `component
commit` to match the present surface (and align the behavioral sentence), or
retain `vcs commit-accepted` as a documented alias and add an explicit mapping
note. Both are contract-only changes; the observed behavior is already correct
for the present surface.

## Underspecified

**F-2 — The requirement does not specify which operations are "destructive."**
Classification: **underspecified**. Severity: **low**. Owner: **project
architect**.

Intended text: `REQUIREMENTS.md` `REQ-INTERACTIVE-SHELL` states, "Destructive
operations MUST require an explicit in-shell confirmation before executing," but
neither defines "destructive operation" nor lists which commands are covered.
The only normative enumeration is the `CONTRACT.md` destructive-operations set.
The contract is the intended consumer source, so this is not a behavioral gap;
it means any future addition to or removal from the set is a *contract* change
that must itself go through the arbitration/acceptance path and cannot be
inferred from the requirement. `IMPL.md`/`DESIGN.md` add no independent
destructive-operation list. This is recorded so the contract enumeration is
understood to be the sole binding list and so its drift is traceable to a
decision, not an assumption.

## Observations (informational, no reclassification required)

**O-1 — The standalone CLI `locks --clean` confirmation path is covered by the
requirement but not separately declared in the contract surface.**
`CONTRACT.md` `Provided interfaces` declares the confirmation for the shell
builtin `locks [clean]`. The bounded evidence attributes a hidden standalone
`Locks { clean: bool }` CLI variant (a `--clean` flag, wired through the same
helper) so the non-interactive `--clean` path also prompts. The requirement's
generic "destructive operations MUST …" mandate is satisfied by this; the
contract's provided-interfaces surface simply does not name the standalone
`--clean` path. Informational only: either extend the surface to declare the
standalone path's confirmation, or treat the generic requirement as covering it.
No behavior change is implied.

## Bounded-evidence limitations (context, not findings)

- The two new `locks clean` tests assert gate membership and prompt naming, not
  an end-to-end PTY interaction (presenting the prompt, reading an answer,
  journaling the decline, refusing removal). The end-to-end path is attributed
  to the bounded chain execution evidence, not independently re-observed here.
- The standalone CLI `--clean` confirmation (O-1) is attributed from the
  write-tests stage, not independently re-observed.
- The requirement's generic "destructive operations" mandate was verified
  against the contract's enumeration (the declared set), per the runbook's
  requirement-vs-contract comparison; no independently maintained list of
  destructive operations exists to cross-check it (see F-2).

## Verdict

The **confirmation behavior** for `locks clean` **matches its declared
contract** (C-1 through C-4): the builtin prompts on the destructive
confirmation before removing stale locks, plain `locks` never prompts, live and
stale locks are distinguished with stale never shown as active, and continuous
`task run` is correctly not over-confirmed. The contract's destructive-
operations set **does** include `locks clean`, closing the specific gap the
branch review raised.

Two items are retained for human arbitration:
1. **F-1 (mismatched, medium):** the contract's destructive set names `vcs
   commit-accepted`, which the same contract surfaces as `component commit` —
   a stale name / internal inconsistency in the consumer contract (behavior is
   correct for the present surface).
2. **F-2 (underspecified, low):** the requirement does not enumerate which
   operations are "destructive," so the contract set is the sole normative list
   and its changes are contract changes requiring arbitration.

One informational observation (**O-1**) notes the standalone `--clean`
confirmation is covered by the requirement but not separately declared in the
contract surface.

No implementation, requirement, contract, or `IMPL.md` file was modified in this
review; per `REVIEW_RUNBOOK.md`, the mismatch is retained — not silently altered
— for explicit arbitration by the project architect.
