# Kvist Independent Review Runbook

This runbook is the repeatable review procedure for Kvist's root component and
for generated Kvist projects. It preserves the separation between observed
implementation behavior and the intended component contract.

## Preconditions

1. Start from a clean checkout with the intended artifact set present.
2. Run the documented quality gate and `kvist doctor .`; resolve any state
   other than `current` before review.
3. Identify the review scope: component directory, immediate parent contract,
   and root contract.

For the root component, the clean-checkout verification is:

```bash
cargo run --locked -- doctor .
cargo run --locked -- tree .
cargo run --locked -- spec validate src/SPEC.md
```

## Clean-slate implementation-record pass

The documenter may read only implementation source, tests, dependency manifests,
and generated non-Markdown configuration needed to understand behavior. The
documenter must not read the component's `SPEC.md`, `TODOS.yaml`, existing
`IMPL.md`, root contract, architecture specification, prior reviews, or Git
history.

The documenter writes or proposes `IMPL.md` using observed behavior only. It
must state public behavior, guarantees/limits, failure paths, and known
limitations. The implementer reviews only formatting and file placement, not
the documenter's behavioral conclusions.

## Source-blind compliance pass

The compliance reviewer may read only the target `SPEC.md`, the newly produced
`IMPL.md`, the immediate parent contract, and `ROOT_CONTRACT.md`. The reviewer
must not read implementation source, tests, manifests, Git history, or prior
compliance conclusions.

The reviewer records each requirement as compliant, mismatched, deferred by an
approved phase boundary, or underspecified. A mismatch record includes the
component path, requirement text, observed behavior text, severity, and owner.

## Arbitration and retention

The project architect owns arbitration. They may:

1. Request redesign and implementation changes.
2. Explicitly approve a specification change.
3. Manually resolve a documented trade-off.

Do not silently modify `SPEC.md` or `IMPL.md` to erase a mismatch. Retain the
final `IMPL.md`, the review record, and any approved arbitration decision in
version control. Do not retain raw provider transcripts, credentials, or
temporary review workspaces.
