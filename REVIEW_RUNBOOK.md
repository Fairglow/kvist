# Kvist Review Runbook

This runbook distinguishes two review purposes:

1. advisory review of human-authored controlled intent before acceptance; and
2. independent compliance review after implementation.

Advisory findings are nonbinding and may be wrong or overly exacting. They
exist to expose possible shortcomings for human acknowledgement. They do not
determine compliance. Compliance review remains the later, independent
comparison between intended and observed behavior.

## Preconditions

1. Start from a clean checkout with the complete artifact set present.
2. For compliance review, run the documented quality gate and
   `kvist doctor .`; resolve any state other than `current` before review.
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

## Advisory document review

The target acceptance gate covers exactly:

- local `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md`;
- a canonical projection of task definitions in `TODOS.yaml` containing `id`,
  `title`, `description`, `context`, `purpose`, `expected_outcome`, `kind`,
  `depends_on`, and `requirements`, while excluding all `component` metadata
  and task lifecycle fields;
- `ROOT_CONTRACT.md`; and
- the immediate parent `CONTRACT.md`, when present.

The first two bullets are digest-bound review targets; the root and parent
contracts are bounded context. Do not add peers, parent requirements or design,
source, tests, implementation records, prior reports, or chat history.
Generated intent drafts are reviewed like human-written intent. Generated
evidence, including `IMPL.md`, compliance reports, review reports, status, and
attempt logs, is exempt.

The planned review command is separate from `component accept` and uses the
existing shell-free, bounded, approved or acknowledged agent execution path.
Kvist will write a versioned receipt and redacted bounded report beneath the
component's `.kvist/reviews/`. A receipt records exact target digests and
scope, reviewer and authoring context identity when known,
provider/profile/model/tool identity, Kvist version, timestamp, report digest
and path, and later acknowledgement or exception. The model cannot mint the
receipt, and raw transcripts or secrets are not retained.

The human may accept every finding, reject every finding, or change nothing.
No finding or severity threshold blocks acceptance. When project review is
required, target acceptance requires a current receipt plus explicit
acknowledgement, or an exception recording actor, timestamp, reason, and exact
target digests. A visible project `[review] required = false` setting disables
the per-bundle gate as a deliberate persistent opt-out. Review defaults to
required when the section or field is absent, and generated project
configuration states `required = true` explicitly. If review is required but no
agent is configured, use an explicit exception; never treat absence as a pass.

Current `component accept` does not enforce this gate. It structurally
validates and records revisions only. Project-level review of `VISION.md`,
`ARCHITECTURE.md`, `ROOT_CONTRACT.md`, ADRs, and referenced native schemas
awaits a separate project-level acceptance surface. The present parent-context
rubber-duck review is useful advisory input but is not a Kvist review receipt.

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

An advisory document-review receipt or report is not a compliance record and
cannot satisfy this arbitration or retention requirement.

This pre-release artifact model has no backward-compatibility or migration
path. Review only the current artifact set; do not reinterpret retired files
or command output as current evidence.
