# Kvist Independent Review Runbook

This runbook is the repeatable review procedure for Kvist's root component and
generated Kvist projects. It preserves separation between approved component
intent, observed implementation behavior, and independent compliance.

## Preconditions

1. Start from a clean checkout with the complete artifact set present.
2. Run the documented quality gate and `kvist doctor .`; resolve any state
   other than `current` before review.
3. Identify the target component, its `REQUIREMENTS.md`, `CONTRACT.md`,
   `DESIGN.md`, test evidence, nearest ancestor component `CONTRACT.md` when
   present, and `ROOT_CONTRACT.md`. Transparent namespace directories do not
   become components or interrupt parent lookup.
4. Ensure the implementer, clean-slate documenter, and compliance reviewer are
   separate review contexts. The implementer must not certify the work.

For the root component, the clean-checkout verification is:

```bash
cargo run --locked -- doctor .
cargo run --locked -- tree .
cargo run --locked -- component validate src
cargo run --locked -- status . --only-documents
```

## Clean-slate implementation-record pass

The documenter may read only implementation source, tests, dependency
manifests, and generated non-intent build configuration needed to understand
observed behavior.

The documenter must not read:

- `REQUIREMENTS.md`, `CONTRACT.md`, or `DESIGN.md`;
- `TODOS.yaml` or a prior `IMPL.md`;
- `VISION.md`, `ARCHITECTURE.md`, or `ROOT_CONTRACT.md`;
- prior reviews, ADR rationale, agent chat history, or Git history.

The documenter writes or proposes a fresh `IMPL.md` from observed evidence
only. It states public behavior, guarantees and limits, state effects, failure
paths, and known uncertainty. Tests are evidence of exercised behavior, not a
substitute for observing the implementation. The implementer may check only
formatting and file placement, not change the documenter's behavioral
conclusions.

## Source-blind compliance pass

The compliance reviewer may read only:

- the target `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`;
- the newly produced `IMPL.md` and bounded test evidence;
- the nearest ancestor component `CONTRACT.md`, when the target is a child; and
- `ROOT_CONTRACT.md`.

The reviewer must not read implementation source, manifests, Git history,
prior compliance conclusions, or private parent and peer artifacts. The
nearest ancestor component contract is the only implicit propagated component
context.

The reviewer records each requirement, contract guarantee, and relevant design
claim as `compliant`, `mismatched`, `approved-deferred`, or `underspecified`.
A finding identifies the component path, stable locator, intended text,
observed behavior and test evidence, severity, and owner. Optional native
schemas count as contract evidence only when `CONTRACT.md` references their
exact path and dialect/version.

## Arbitration and retention

The project architect owns arbitration and may:

1. request redesign and implementation changes;
2. explicitly approve an intent-document change; or
3. record a reasoned exception or approved deferral.

Do not silently modify requirements, contract, design, or `IMPL.md` to erase a
mismatch. Retain the fresh `IMPL.md`, review record, and approved arbitration
decision in version control. Do not retain raw provider transcripts,
credentials, secrets, or temporary review workspaces.

This pre-release artifact model has no backward-compatibility or migration
path. Review only the current artifact set; do not reinterpret retired files
or command output as current evidence.
