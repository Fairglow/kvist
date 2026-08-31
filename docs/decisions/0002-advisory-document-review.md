# ADR 0002: Advisory document review before acceptance

## Status

Accepted

## Context

Human-authored intent can contain omissions, contradictions, ambiguous
requirements, unsafe assumptions, or weak verification plans. An AI review can
surface these issues before acceptance, but model findings can also be wrong,
nitpicky, inconsistent, or biased by incomplete context. Treating a score,
severity, or model verdict as an approval oracle would transfer authority away
from the human and confuse document quality review with independent
implementation compliance.

Kvist also needs deterministic local acceptance, strict component context,
durable inspectable evidence, explicit behavior when no agent is configured,
and no hidden network or subprocess work. Current `component accept`
structurally validates intent and records revisions; it does not enforce AI
review.

## Decision

The target workflow requires an advisory AI review opportunity before
acceptance of the local component intent bundle when project review is
required. The initial bundle contains exact digests of:

- `REQUIREMENTS.md`;
- `CONTRACT.md`;
- `DESIGN.md`; and
- a canonical projection of task definitions in `TODOS.yaml`, excluding
  all `component` metadata and per-task lifecycle state. The projection contains
  only `id`, `title`, `description`, `context`, `purpose`, `expected_outcome`,
  `kind`, `depends_on`, and `requirements`.

Generated intent drafts are included. Generated evidence such as `IMPL.md`,
compliance reports, review reports, status, and attempt logs is exempt. The
gate does not govern arbitrary Markdown. Project-level `VISION.md`,
`ARCHITECTURE.md`, `ROOT_CONTRACT.md`, ADRs, and referenced native schemas need
a later project-level acceptance surface.

When project review is required, acceptance requires either a current receipt
for the exact bundle digests plus explicit human acknowledgement or an explicit
exception. A project `[review] required = false` setting is a visible
persistent opt-out that disables the per-bundle gate. A per-bundle exception
records actor, timestamp, reason, and exact digests. If review is required but
no agent is configured, an exception is required; absence is never an implicit
pass. Review defaults to required when the section or field is absent, and
generated project configuration states `[review] required = true` explicitly.

Review feedback is advisory and nonbinding. No finding, score, or severity
threshold requires a change, blocks acceptance, or determines compliance. The
human may acknowledge the report and accept unchanged documents.

`component accept` remains deterministic and local and never starts an agent or
makes a network call. A separate planned review command uses the existing
shell-free, bounded, approved or acknowledged agent execution path. Its context
is limited to the local intent set and canonical task projection,
`ROOT_CONTRACT.md`, and the immediate parent `CONTRACT.md`. Peer artifacts and
parent internals are excluded. Separate authoring and reviewing contexts are
preferred, but recorded provenance is not proof of independence or quality.

Kvist writes versioned receipts from bounded execution evidence under
component-local `.kvist/reviews/`; model output cannot mint them. A receipt
binds target digests and scope, reviewing and authoring context identity when
known, provider/profile/model/tool identity, Kvist version, timestamp, a
redacted bounded report digest and path, and acknowledgement or exception.
Raw transcripts and secrets are not retained.

Review receipts and reports are VCS-trackable but are not members of the
five-artifact component set. They do not trigger component candidacy or
staleness, enter task context, or count as compliance evidence. A receipt
records provenance and the review opportunity; it is not a correctness,
independence, quality, or compliance certificate.

## Consequences

- Humans receive review feedback before accepting controlled component intent
  while retaining full authority to reject the feedback.
- Acceptance can block only on missing current review evidence,
  acknowledgement, or exception—not on report content.
- Exact digests prevent a review of one bundle from being reused for changed
  intent.
- Canonical task projection avoids self-invalidating receipts when mutable
  provenance is updated.
- Projects without a configured agent remain usable through explicit,
  auditable exceptions rather than silent bypass.
- Review evidence adds durable files and schema/versioning work but remains
  separate from component discovery, task state, and compliance.
- Project-level review and acceptance require a later dedicated surface.
- Current CLI behavior is unchanged until the planned queue, implementation,
  tests, security audit, and compliance review are complete.

## Rejected alternatives

- **Make findings or severity thresholds block acceptance:** rejected because
  model judgments are fallible and human authority is non-negotiable.
- **Treat a receipt as compliance evidence:** rejected because document review
  examines intended content, while compliance independently compares intent,
  observed implementation, and test evidence.
- **Run review inside `component accept`:** rejected because acceptance must
  remain deterministic, local, and free of hidden agent or network activity.
- **Implicitly pass when no agent is configured:** rejected because it hides
  the absence of review; an explicit exception is required.
- **Exempt generated intent drafts:** rejected because generation does not make
  proposed requirements or design authoritative or reliable.
- **Review every Markdown file:** rejected because Kvist does not own every
  repository document and the initial enforceable scope must be precise.
- **Store raw transcripts:** rejected because they may contain secrets,
  irrelevant context, or unbounded provider data; bounded redacted reports and
  evidence-bound receipts are sufficient.
- **Require proven reviewer independence:** rejected because provenance can be
  recorded but cannot prove cognitive independence or review quality.
