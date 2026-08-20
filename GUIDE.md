# Kvist workflow guide

Kvist is a local, filesystem-native tool for human-directed, spec-driven
development. Its current interface is command-line based; future graphical and
editor interfaces operate on the same durable project artifacts. A component
directory contains a layered `SPEC.md`, a versioned `TODOS.yaml`, observed
implementation record `DOCS.md`, and source. The architecture is recursive: a
component works from its own contract, its immediate parent contract, and
`ROOT_CONTRACT.md`, not from peer implementations.

[`KVIST_Architectural_Specification_Full.md`](KVIST_Architectural_Specification_Full.md)
defines the target architecture. This guide describes the commands that exist
today and labels the required but not-yet-automated stages explicitly.

## Intended lifecycle

1. The human architect defines and validates the component contract in the
   three-layer `SPEC.md` format. The architect begins with a project `VISION`,
   then decomposes it into one or more hierarchical components. Specifications
   may be drafted manually, with agent assistance, or by an architect agent;
   the architect iterates and explicitly approves the resulting component
   boundary and contract.
2. A designer agent derives a traceable `TODOS.yaml` plan from the approved
   specification in test, implementation, security-audit, and
   compliance-review order. The architect may refine that specialized plan;
   agent and architect iterate until the architect accepts it.
3. The executor advances every accepted, ready task in the queue in dependency
   order with only the permitted component context. It can run uninterrupted
   and unsupervised; the human may choose to observe or run one task at a time,
   but does not need to intervene between tasks.
4. A clean-slate documenter derives `DOCS.md` from code without the
   specification. A separate, source-blind reviewer compares it with the
   specification. The architect arbitrates every discrepancy explicitly.

The human remains the approval authority at every stage. Kvist validates
specifications and queues, persists legal task transitions, and can invoke a
configured external agent for one task. It does **not** yet automate the
architect or designer agent roles, the interview, clean-slate documentation,
source-blind review, or arbitration loop. Do not claim a component is
compliant until the independent review is actually recorded.

## Start a project

```bash
kvist init my-project
cd my-project
kvist doctor .
kvist status .
```

`init` writes the root contract and root component artifacts only into an
uninitialized directory. `doctor` provides read-only diagnostics, and `status`
reports component state without persisting derived stale evidence.

After the architect approves a child component boundary, create and validate
its specification:

```bash
kvist spec new src/network
kvist spec validate src/network/SPEC.md
```

The designer agent then drafts the queue from that approved specification. The
architect reviews and may improve the result before accepting it. Each task
must state its purpose, context, expected outcome, requirements, dependencies,
lifecycle kind, and status. The queue is durable workflow state, not an agent
transcript.

## Inspect and revalidate work

```bash
kvist status .
kvist tree .
kvist task next .
kvist spec accept .
```

`status` detects local and immediate-parent specification digest mismatches and
reports stale components. `spec accept` is an explicit revalidation write: it
records the currently reviewed revisions and clears stale evidence for the
selected component. It does not approve an arbitrary implementation change or
replace independent compliance review.

`task next` selects the first ready task in declared order. `task transition`
performs one audited state change and records `prepared` and `committed`
attempt evidence. Both require a current project, current component, and
complete VCS tracking.

## External agents and verification

`task run COMPONENT_DIR [TASK_ID]` is an optional local subprocess integration.
It uses the `developer` profile for `test` and `implementation` tasks, and the
`architect` profile for `security-audit` and `compliance-review` tasks. The
only supported profiles are `architect` and `developer`.

The target executor runs the accepted queue uninterrupted and lets the final
independent review decide compliance. The current command-line surface exposes
only the one-task primitive, but it can run the full ready queue unattended on
POSIX shells:

```bash
while task_id="$(kvist task next .)" && [ "$task_id" != "no ready task" ]; do
  kvist task run . "$task_id" || exit $?
done
kvist status .
```

This loop selects and executes each ready task in order, stopping when no task
is ready or a command-level error occurs. The final `status` exposes a blocked
or stale result. It does not automate the clean-slate documenter or
source-blind reviewer in step 4; that independent validation remains manual
until Phase 3 automation is implemented.

Agent configuration is selected from `[agent]` in `kvist.toml`,
`.kvist/config.toml`, the user configuration path, then the system
configuration path. Template arguments are passed without a shell and may use
`{prompt}`, `{context_files}`, and `{target_directory}`. Keep every intended
argument whitespace-free or use a wrapper executable; shell pipelines,
redirection, and shell quoting are not supported.

For implementation tasks, configure and approve the repository test policy
before running:

```bash
kvist task approve-policy
kvist task run . implement-code
kvist task log . implement-code
```

The policy approval covers only `[test_policy]`, not agent configuration. Test
programs receive only the configured environment allowlist, time limit, and
output cap. Agent and test programs currently run directly on the host. They
are not sandboxed, agent execution has no resource cap, and the resolved agent
template is not approved. Run them only in a repository you trust until the
security backlog in [`TODO.md`](TODO.md) is complete.

## Documentation and review discipline

`DOCS.md` is an observed implementation record, not user-facing documentation.
Its documenter must examine source and tests without reading `SPEC.md` or the
existing `DOCS.md`. The source-blind reviewer then examines only the
specification, newly derived documentation, immediate parent contract, and
root contract. Record compliance, mismatches, deferred work, and arbitration
in version control; never edit the specification or observed documentation to
hide a disagreement.

The reusable procedure is in [`REVIEW_RUNBOOK.md`](REVIEW_RUNBOOK.md).
[`COMPLIANCE_REVIEW.md`](COMPLIANCE_REVIEW.md) records completed reviews and
the current unreviewed execution-policy discrepancy.
