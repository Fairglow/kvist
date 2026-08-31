# Final Artifact-Model Redesign Compliance Review

**Review date:** 2026-08-31

**Artifact-model redesign verdict:** **COMPLIANT**

**Whole-product verdict:** **NOT FULLY COMPLIANT; PRE-EXISTING BLOCKERS REMAIN**

## Review basis, scope, and independence

This is a repeated independent, source-blind compliance pass over the current
uncommitted artifact-model redesign for `kvist.engine` and `agent-runtime`.
The reviewer was separate from implementation, clean-slate implementation
documentation, and the security audit.

The review used only:

- `VISION.md`, `ARCHITECTURE.md`, and `ROOT_CONTRACT.md`;
- root and child `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, and fresh
  `IMPL.md` records;
- `src/TODOS.yaml` and `src/agent_runtime/TODOS.yaml` solely for revisions,
  traceability, ordering, and review-state provenance;
- permitted test sources under `tests/**` and
  `src/agent_runtime/tests/**`;
- both Cargo manifests for package, feature, and test-command identity;
- `docs/standards.md` and ADR 0001; and
- the previous `COMPLIANCE_REVIEW.md` only to reassess its stable findings and
  retain unresolved limitations.

The reviewer did not read production Rust source, source-bearing Git history or
diffs, build artifacts, or prior agent conversations. No behavior is claimed
unless it appears in a fresh independent `IMPL.md` record or permitted test
source. Tests were not rerun during this documentation-only pass; test source
is cited as executable evidence rather than as a fresh run result.

This report does not self-certify either implementation context and is not a
replacement security audit.

## Verdict boundaries

The **artifact-model redesign** comprises:

- separate `REQUIREMENTS.md`, `CONTRACT.md`, and `DESIGN.md` authority;
- five-artifact component candidacy, completeness, discovery, and reporting;
- component-neutral implementation records;
- independent local and immediate-parent contract revisions;
- variable-depth nearest-parent contract resolution;
- status staleness attribution and non-propagation rules;
- root and immediate-parent read-only sandbox context materialization;
- artifact-related CLI and package naming;
- root-contract execution-approval binding;
- safe publication into reverse-discovery `.kvist` metadata paths; and
- the redesign's ordered test, implementation, security, and compliance
  provenance.

The redesign is compliant on the permitted evidence.

This does not certify unrelated or older whole-product surfaces. Prepared
attempt recovery, reverse-discovery traversal/source-size bounds, standalone
runtime limits, and pending promotion reviews remain separately classified
below.

## Reassessment of prior stable findings

### CR-ART-001 — Child-component discovery rule

**Previous result:** High, discrepant, blocking

**Current result:** Closed; compliant

`ARCHITECTURE.md` now defines one-artifact candidacy and five-artifact
completeness consistently with `src/CONTRACT.md`, `src/IMPL.md`, and
`tests/discovery.rs`. Incomplete candidates remain diagnosable without treating
ordinary artifact-free namespace directories as components.

### CR-PROV-001 — Terminal review provenance

**Previous result:** High, discrepant, blocking

**Current result:** Closed for the artifact-model redesign

`src/TODOS.yaml` now contains an atomic ordered chain:

1. `artifact-model-write-tests` — completed;
2. `artifact-model-implement-code` — completed;
3. `artifact-model-security-audit` — completed; and
4. `artifact-model-compliance-review` — in progress and dependent on the
   completed security audit.

The tasks reference the artifact-model, component-document,
revision-provenance, execution-boundary, and compliance clauses. The current
in-progress state is appropriate while this independent report is being
written; completion must be recorded by the normal queue workflow after the
report is accepted.

Unrelated pending child security/compliance tasks remain promotion gates for
their own capabilities. They do not invalidate this root redesign chain.

### CR-LIFE-001 — Task readiness and prepared-attempt recovery

**Previous result:** High, discrepant, blocking

**Current result:** Split

- **Transitive readiness:** Closed. The fresh root `IMPL.md` records one shared
  transitive readiness predicate for `task next`, transition to in-progress,
  and automatic `task run` selection.
- **Prepared-attempt recovery:** Retained as `WP-LIFE-001`. The fresh record
  still observes no command dedicated to replaying or reconciling an
  interrupted prepared entry.

The retained recovery discrepancy is a whole-product lifecycle blocker, not an
artifact-model redesign discrepancy.

### CR-SBX-001 — Runner timeout and environment guarantees

**Previous result:** High, discrepant, blocking

**Current result:** Closed; compliant

The fresh root record states that runner launch clears the host environment,
adds only allowlisted values, creates a dedicated process group, and kills and
waits for that group on timeout or combined output exhaustion.

Permitted tests provide focused evidence:

- `task_run_executes_successfully_and_transitions_completed` injects an
  unapproved ambient variable and verifies that the runner observes it as
  unset;
- timeout and output-limit task tests exercise bounded blocked outcomes and
  durable evidence; and
- the fresh implementation record reports a sleeping-descendant timeout
  fixture and process-group cleanup.

No host fallback is claimed or observed.

### CR-ONB-001 — Reverse-discovery safety and bounds

**Previous result:** Medium, discrepant

**Current result:** Split

- **Unsafe `.kvist` metadata path publication:** Closed for the redesign.
  `src/IMPL.md` records initial path validation, post-creation revalidation,
  revalidation before each artifact publication, and atomic create-new writes.
  `reverse_discover_rejects_a_symlinked_metadata_directory` proves that a
  linked metadata directory is rejected without writing through it.
- **Traversal depth, entry count, and source-file size:** Retained as
  `WP-REV-001`. The fresh record still observes no explicit bounds and records
  complete reads of regular Rust and Python files.

The retained resource-bound discrepancy predates and is broader than safe
artifact publication.

### CR-EVID-001 — Independent revision evidence

**Previous result:** Medium, partial

**Current result:** Closed; compliant

`status_attributes_contract_and_design_changes_independently` covers the
separate local contract and design causes.
`parent_requirements_and_design_do_not_stale_a_child` proves that parent
internal changes stale the parent without staling its child. Existing status
and acceptance tests cover local requirements, parent contract, and
`../../CONTRACT.md` paths through transparent namespace directories.

The evidence now covers all four mismatch classes and the required
non-propagation rule.

### CR-EVID-002 — Immediate-parent read-only sandbox mount

**Previous result:** Medium, partial

**Current result:** Closed; compliant

`child_task_run_mounts_the_nearest_parent_contract_read_only` creates a child
below a transparent namespace, uses `../../CONTRACT.md`, and verifies the
nearest parent source path, fixed
`/workspace/context/PARENT_CONTRACT.md` destination, context inclusion, and
read-only access. Root-contract read-only mounting remains covered by the root
task-run fixture.

### CR-IMPL-001 — Root-specific child implementation-record heading

**Previous result:** Low, discrepant

**Current result:** Closed; compliant

Both fresh `IMPL.md` files use `# Component Implementation Record`. The root
record states that this component-neutral heading is the production validation
rule, and permitted child fixtures use the same heading.

### CR-AR-001 — Standalone runtime limits

**Previous result:** Medium, partial

**Current result:** Retained as `WP-AR-001`; outside the redesign

The child record still states that standalone `agent-run run` sets no total
attempt timeout and standalone `agent-run model` does not bridge process
signals into its cancellation token. Library supervision and embedding-host
cancellation remain bounded and tested. This is not an artifact-model
regression, but it prevents an unrestricted whole-product compliance claim.

## Security-audit closure evidence

Queue provenance records the independent artifact-model security audit as
completed before this review. This compliance pass did not repeat that audit.

Fresh source-blind evidence demonstrates closure of the newly recorded
artifact-boundary remediations:

- execution approval now binds the exact bounded, regular, non-link
  `ROOT_CONTRACT.md` digest; and
- reverse discovery now rejects and repeatedly revalidates the `.kvist`
  publication directory.

`approval_rejects_changed_root_contract_before_sandbox_probe` changes the root
contract after approval and verifies refusal before sandbox request creation
with the task queue unchanged.
`reverse_discover_rejects_a_symlinked_metadata_directory` verifies refusal
without writes through the link.

The allowed queue metadata records the security task as completed, and the
fresh records/tests expose no remaining high-confidence vulnerability in the
artifact-model redesign scope. This statement does not promote unrelated child
providers or replace their pending audits.

## Material-clause traceability

### `kvist.engine`

| Requirement | Redesign result | Current evidence and disposition |
| --- | --- | --- |
| `REQ-ARTIFACT-MODEL` | Compliant | Initialization, discovery, status, records, and tests consistently use the root project artifacts and adjacent five-artifact component set. One-artifact candidacy and five-artifact completeness are now aligned. |
| `REQ-COMPONENT-DOCUMENTS` | Compliant | Deterministic three-document templates, conflict preflight, structural validation, version/order/uniqueness/content diagnostics, bounded reads, no rewrite, and line-aware CLI/JSON failures are recorded and tested. |
| `REQ-DISCOVERY-STATUS` | Compliant for redesign | Lexical bounded component discovery, link rejection, transparent namespaces, stable artifact order, all state classes, read-only status, and text/JSON reporting agree. Whole-product reverse-discovery bounds are separate. |
| `REQ-REVISION-PROVENANCE` | Compliant | Independent requirements/contract/design digests, nearest-parent contract paths, all local stale causes, parent-contract propagation, and parent requirements/design non-propagation are recorded and tested. |
| `REQ-TASK-QUEUE` | Compliant | Strict version-one YAML, deterministic serialization, bounded task fields, stable IDs, dependencies, timestamps, requirement locators, and ordered task-kind ancestry agree. The redesign queue chain is traceable. |
| `REQ-TASK-LIFECYCLE` | Partial whole-product compliance | Current/VCS/lock/transition/atomic/evidence and transitive-readiness behavior agree. Dedicated prepared-attempt reconciliation remains absent under `WP-LIFE-001`. |
| `REQ-EXECUTION-BOUNDARY` | Compliant for redesign execution context | Approval binds root contract, runner, policy, project/worktree, limits, and authenticated user state. Tests and the record cover denied network, explicit mounts, cleared environment, bounded process-group cleanup, and no host fallback. |
| `REQ-AGENT-INTEGRATION` | Compliant | Shell-free invocation, explicit host acknowledgement, sandbox/host distinction, bounded redacted output, role selection, and retained Kvist policy authority agree. |
| `REQ-COMPLIANCE` | Compliant for redesign | Fresh clean-slate records, this separate source-blind pass, a completed prior security task, durable discrepancies, and an ordered review chain satisfy the redesign gate. |
| `REQ-CONVERSION-IMPORT` | Compliant for redesign publication; partial whole product | Existing files are preserved, drafts are explicit, generated artifacts are validated, ambiguous/link-like metadata is refused, and publication is no-clobber. Traversal/source-size bounds remain `WP-REV-001`. |

### `agent-runtime`

| Requirement | Result | Current evidence and disposition |
| --- | --- | --- |
| `AR-REQ-PROMPT-COMMAND` | Compliant | Bounded single-source prompts, shell-free rendering, documented placeholders, JSON encoding, and fresh typed retry context agree with the record and tests. |
| `AR-REQ-SUPERVISION` | Compliant as a library; partial standalone | The library bounds output, configured idle/wall behavior, retries, cancellation, stream retention, and process-group cleanup. Standalone `run` omits a total attempt timeout under `WP-AR-001`. |
| `AR-REQ-PROFILES` | Compliant | Strict bounded TOML, safe canonical/legacy resolution, unrelated-content preservation, complete validation, and synchronized atomic replacement agree. |
| `AR-REQ-SETUP` | Compliant | Caller-provided I/O, bounded advisory probes, maintained templates, explicit selection, acknowledgement, and supervised qualification agree. |
| `AR-REQ-MODEL-TRANSPORT` | Compliant as a library; partial standalone | Direct and optional Rig transports retain explicit bounded requests, responses, deadlines, cancellation, identities, usage, finish reasons, errors, and inert tool intents. Standalone signal bridging remains absent. |
| `AR-REQ-NATIVE-RUNTIME` | Deferred/planned | Native loop, broker, host authorization, and execution backend remain explicitly unpromoted. Tool intents cannot cause effects in the implemented transport boundary. |
| `AR-REQ-ADAPTER-BOUNDARY` | Compliant; promotion-specific reviews pending | Canonical public types remain component-owned and Rig remains exact-pinned, optional, private, and translated at the boundary. Pending provider reviews govern promotion, not artifact-model compliance. |

## Focus-area conclusions

| Focus area | Result |
| --- | --- |
| Artifact hierarchy | Compliant. Candidate discovery and five-file completeness are explicit and consistent. |
| CLI and package naming | Compliant. Root commands use `component`; the child uses package `agent-runtime`, crate `agent_runtime`, and binary `agent-run`, with deliberate bounded legacy profile fallback. |
| Five-artifact discovery | Compliant. Required names, order, incomplete/invalid reporting, lexical traversal, and transparent namespaces agree. |
| Independent revisions | Compliant. Local requirements, contract, design, and nearest-parent contract revisions remain independently attributable. |
| Variable-depth parent paths | Compliant. Safe ancestor-only paths and computed `../../CONTRACT.md` repair/status/context behavior are tested. |
| Status staleness | Compliant. Every local cause, parent-contract propagation, parent-internal non-propagation, precedence, stable output, and no-write behavior are evidenced. |
| Sandbox context | Compliant for redesign. Root and nearest-parent contracts are fixed-destination read-only mounts; the component is the only writable mount. |
| Runner cleanup and environment | Compliant on fresh observed evidence. Ambient environment is cleared and timeout/output exhaustion kill and wait for the runner process group. |
| Approval provenance | Compliant. Approval binds exact `ROOT_CONTRACT.md` bytes and refuses a post-approval change before execution or mutation. |
| Reverse-discovery metadata path | Compliant. Existing unsafe path types are rejected and the destination is revalidated before no-clobber publication. |
| Standards posture | Compliant. Standards are described as adopted, tailored, aligned, or deferred without unsupported formal-conformance claims. |
| Independent review rules | Compliant for redesign. The queue has the required ordered chain, security precedes this pass, and this report remains source-blind. |

## Remaining whole-product findings

### WP-LIFE-001 — No dedicated prepared-attempt reconciliation

**Severity:** High

**Classification:** Discrepant

The root requirements and contract require explicit recovery for an ambiguous
prepared attempt. The fresh root record still observes only fencing plus an
orphan-lock unlock command; it does not observe a command that reconciles,
commits, rolls back, or otherwise resolves the prepared journal state.

**Disposition:** Whole-product blocker. Not caused by, and not concealed by,
the artifact-model redesign verdict.

### WP-REV-001 — Reverse-discovery traversal and source reads are unbounded

**Severity:** High

**Classification:** Discrepant

The root quality requirements require explicit traversal and input bounds.
The fresh record still observes no reverse-discovery depth, entry-count, or
source-file-size bound and records complete reads of regular Rust and Python
files.

**Disposition:** Whole-product resource-safety blocker. Safe `.kvist`
publication is compliant, but it does not resolve unbounded input discovery.

### WP-AR-001 — Standalone runtime omits total timeout and signal bridging

**Severity:** Medium

**Classification:** Partial

The reusable library provides configured wall-time and cooperative
cancellation mechanisms. The standalone `agent-run run` command sets no total
attempt timeout, and standalone `agent-run model` does not translate process
signals into its cancellation token.

**Disposition:** Blocks a complete standalone-runtime compliance claim; does
not block the root artifact-model redesign.

### WP-EVID-001 — Directory-form runner evidence remains narrow

**Severity:** Medium

**Classification:** Partial

The root record states that directory-form runners are hashed and copied, but
integration evidence is concentrated on executable-file runners.

**Disposition:** Retain as a verification limitation until the supported
directory-runner behavior is directly exercised or removed from the supported
surface.

### WP-PROMO-001 — Unrelated child promotion chains remain pending

**Severity:** Informational as to this redesign

**Classification:** Deferred/promotion-gated

The child queue retains pending security/compliance work for the rename,
direct/Rig provider promotion, setup, and other future capabilities.

**Disposition:** Those capabilities must not be represented as independently
promoted. Their pending state is not a discrepancy in the completed root
artifact-model chain and does not reverse this redesign verdict.

## Accepted deferrals

- A general dependency-contract graph beyond the immediate parent remains
  planned.
- Native loop, typed broker, host authorization, and execution backends remain
  planned and unpromoted.
- Hosted providers, broader interchange/schema export, and non-Linux execution
  remain deferred.
- `agent-runtime` intentionally provides host-process supervision, not a
  sandbox or host-owned durable compliance store.
- Direct and optional Rig transports intentionally remain loopback HTTP only.
- Pre-release compatibility and migration are not promised.

## Final verdict

**Artifact-model redesign: COMPLIANT.**

All prior redesign-specific blockers are closed on the current permitted
evidence. The separated authority model, component hierarchy, CLI identity,
five-artifact discovery, independent revisions, parent-path handling, status
semantics, sandbox context, root-contract approval binding, safe metadata
publication, standards posture, and independent review provenance agree across
intent, fresh observed records, queue metadata, and tests.

**Whole product: NOT FULLY COMPLIANT.**

The blocking discrepancies are `WP-LIFE-001` (prepared-attempt recovery) and
`WP-REV-001` (reverse-discovery traversal/source-size bounds). `WP-AR-001` and
`WP-EVID-001` remain material partial guarantees, while `WP-PROMO-001` keeps
unrelated unreviewed capabilities unpromoted.
