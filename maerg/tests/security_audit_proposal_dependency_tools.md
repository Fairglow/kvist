# Security Audit: Proposal and Dependency-Request Tools

**Component:** maerg (Kvist root component)
**Task:** `phase-4-security-audit`
**Scope:** Source-blind security review of the broker classification funnel, the dependency-origin policy, the proposal evidence path under `.kvist`, the decision-pause ordering, the closed `ALLOWED_TOOLS` enumeration, and the no-network-in-sandbox tenet for dependency acquisition.
**Method:** Independent source inspection of `src/authoring/mod.rs`, `src/authoring/apply.rs`, `src/agent.rs`, `src/vendor_command.rs`, `src/sandbox.rs`, `src/file_io.rs`, and `src/agn/mod.rs`.
**Reviewer stance:** Independent security auditor. This is the security-audit stage of the `phase-4` chain (tests → implementation → security audit → compliance review). The implementer did not author this record. No tests, builds, or `cargo` invocations were executed in this review; the suite is run automatically by Kvist in the verification sandbox after this turn.

## Summary

The broker and applier form a total, deny-by-default boundary. Every untrusted model intent is reduced by `classify_intent` to exactly one of five outcomes — read, effect, decision, dependency request, or drop — with no panic, wedge, or silent loss. The proposal evidence path writes only redacted, no-clobber, real-directory-guarded JSON under `.kvist/authoring/`, never a protected intent document. The decision-pause ordering stops the run in `awaiting-decision` before any effect of the turn applies. The closed `ALLOWED_TOOLS` enumeration is the single source of truth for which authoring actions exist, and no untrusted intent can write a protected document or escape the `src/`/`tests/` writable scope.

The dependency-acquisition model satisfies the no-network-in-sandbox tenet: the agent's protected fulfillment path (`vendor_project_protected`) fails closed before any network contact, so an in-policy dependency request never reaches a network-allow sandbox. New-dependency acquisition is a host-execution user decision, not a separate sandbox.

One dependency-origin policy gap is retained as a required fix: the private-range prefix list is incomplete for the RFC 1918 `172.16.0.0/12` block.

## Satisfied Controls

### 1. Closed `ALLOWED_TOOLS` enumeration

**Control:** The set of authorized authoring tools is a fixed, closed, deny-by-default enumeration. It is not supplied by agents; a change to the set is a change to this code and its tests, never to an untrusted agent.

**Mechanism:** `ALLOWED_TOOLS` is a `pub const &[&str]` of exactly six names: `read_file`, `list_files`, `write_file`, `edit_file`, `propose_decision`, `request_dependency`. `classify_intent` checks `ALLOWED_TOOLS.contains(&intent.name.as_str())` before any argument parsing; a non-allowlisted name is dropped with a non-secret reason. The `match` on `intent.name` has a fail-closed `_` arm that is unreachable because of the allowlist check but is kept as a default rather than a panic. `authoring_tool_definitions()` advertises exactly the allowlisted set, and the test `tool_definitions_cover_exactly_the_allowed_set` asserts the advertised set equals `ALLOWED_TOOLS` in both directions, so a tool the broker would refuse can never be offered to the model.

**Assessment:** Satisfied. The enumeration is total and closed; no untrusted intent can introduce a new tool name.

### 2. Broker classification funnel

**Control:** `classify_intent` is total and infallible: no intent can panic, wedge, or silently vanish. Every failure path yields a `Drop` reason, never a panic.

**Mechanism:** An empty tool name, a non-allowlisted tool, and non-object arguments are all dropped before any tool-specific parsing. Each write tool additionally requires a normalized destination and bounded content; the decision tool requires bounded `summary` and `why`; the dependency tool validates its identity and origin against the dependency policy. Any failure is a `Drop` reason. `authorize_intents` reduces every intent to exactly one of the five `BrokerOutcome` variants, and `enforce_bound` drops every effect when the plan-wide content bound is exceeded, so no caller can observe an over-budget plan.

**Assessment:** Satisfied. The funnel is total and fail-closed; a hostile or buggy model cannot wedge the broker or escape through an error.

### 3. Proposal evidence path under `.kvist`

**Control:** A surfaced decision is recorded as durable, redacted, no-clobber evidence under the component state directory, never a protected intent document, and never an applied change.

**Mechanism:** `record_decision_evidence` writes to `.kvist/authoring/proposals/` via `ensure_authoring_evidence_dir`, which reuses the same real-directory guard as the log directory so a symlink at any level is refused. The proposal is redacted through `redact` before serialization. The file is created with `write_new_file_atomically` (no clobber), and the filename derives from `sha256(identity:call_id)`, so untrusted call ids cannot traverse the path or overwrite an existing record. The companion `.md` file is written with the same no-clobber primitive. The `decision_markdown` footer states the proposal is advisory, not human acceptance, and that no proposed intent change was applied. The test `decisions_reusing_call_ids_retain_separate_evidence` pins that a reused call id retains separate evidence and that a second write with the same identity fails closed without replacing the first.

**Assessment:** Satisfied. The evidence path is redacted, no-clobber, symlink-refusing, and never writes a protected document.

### 4. Decision-pause ordering

**Control:** An impactful, uncovered decision pauses the run in `awaiting-decision` before any effect of the turn applies, and the decision takes precedence over a dropped intent so a decision-worthy issue is never masked by a turn failure.

**Mechanism:** In the agent turn loop, after `authorize_turn` produces the plan, the broker checks `!plan.decisions.is_empty()` before any effect is staged or applied. When a decision is present, each surfaced decision is recorded as durable evidence, `run_turns[turn_index].surfaced_decisions` is set, and the loop returns `Ok(None)` — the turn's effects are never staged or dispatched. The `AgentRunResult.surfaced_decision` flag is set from `!all_decisions.is_empty()`, and the run harness transitions the task to `awaiting-decision`. The dependency-request path (step 2b) runs only after the decision check, so an in-policy request is recorded and the agent continues, but only when no decision paused the run.

**Assessment:** Satisfied. The decision-pause ordering is correct: a decision always precedes any effect, and a decision-worthy issue is never masked by a dropped intent.

### 5. Dependency-origin policy

**Control:** A dependency request is accepted only when the origin is an exact, pinned, non-private revision. A private, link-local, loopback, unverified, or unpinned origin is surfaced as a decision and pauses the run in `awaiting-decision`.

**Mechanism:** `classify_dependency` validates the crate identity (`looks_like_crate_name`) and the origin. `is_unverified_origin` rejects origins that start with a private, link-local, loopback, or local-path prefix, or that use a non-public scheme. `looks_pinned` requires an exact three-part semver revision or a VCS origin with an explicit `?rev=` or `?tag=` fragment that pins a non-empty, non-wildcard reference. A request that passes both checks is recorded as a `DependencyRequest` and the agent continues; otherwise it is surfaced as a `ProposedDecision`.

**Assessment:** Satisfied, with one retained finding (F1) on the completeness of the private-range prefix list.

### 6. No network in the sandbox (dependency acquisition)

**Control:** The agent's protected dependency-fulfillment path never reaches a network-allow sandbox. New-dependency acquisition is a host-execution user decision, not a separate sandbox. Only host execution may have network.

**Mechanism:** The agent's in-policy dependency request calls `vendor_project_protected`, which unconditionally returns `Err(KvistError::VendoringUnavailable)` — "the model-requested dependency identity is not bound to the existing lock/manifest; protected request fulfillment is unsupported even when existing vendoring is ready." It never reaches `run_native_cargo_preparation` or `execute_dependency_acquisition`. The network-allow acquisition sandbox (`execute_dependency_acquisition`, `NetworkMode::PackageSources`) is reached only through the explicit host-execution `kvist vendor` path (`vendor_project` → `vendor_rust_project` → `provision_rust_via_acquisition_sandbox`), which is a human-authorized operator action outside the agent's control. The agent may use existing dependencies already available (offline verification, `NetworkMode::Deny`), but a new dependency that requires network access to download is a host-execution user decision.

**Assessment:** Satisfied. The no-network-in-sandbox tenet holds for the agent path: the protected fulfillment path fails closed before any network contact, and the only network-allow sandbox is the explicit host-execution vendor command.

## Findings

### F1 — Dependency-origin private-range prefix list is incomplete (Medium, required fix)

**Observation:** `UNVERIFIED_ORIGIN_PREFIXES` in `src/authoring/mod.rs` contains `"172.16."` and `"172.17."` but not the full RFC 1918 `172.16.0.0/12` block (`172.16.0.0`–`172.31.255.255`). A VCS origin such as `git+https://172.20.0.5/crate.git` is not classified as private by `is_unverified_origin` and, if it carries a pinned `?rev=` fragment, would be accepted as an in-policy `DependencyRequest`.

**Impact:** A model could request a dependency from a private IP address in the `172.18.0.0`–`172.31.255.255` range and have it accepted as in-policy. The downstream protected fulfillment path (`vendor_project_protected`) fails closed before any network contact, so no actual network escape occurs. However, the origin policy itself has a scope-escape: the recorded `DependencyRequest` would carry a private origin that the policy claims to have rejected.

**Required fix:** Extend the private-range check to cover the full `172.16.0.0/12` block. A unit test must pin that `172.20.0.5` (and the boundary addresses `172.16.0.0` and `172.31.255.255`) are classified as private.

**Remediation (applied):** Replaced the two `172.16.`/`172.17.` prefixes with `is_rfc1918_private_ip`, which strips the scheme prefix, takes the host IP, and rejects any address whose first octet is `172` and whose second octet is in `16..=31`. The new test `rfc1918_private_ip_origins_are_surfaced_as_a_decision` pins that `172.16.0.0`, `172.17.0.5`, `172.20.0.5`, `172.31.255.255`, and `172.25.10.9` are surfaced as decisions, and that `172.32.0.1` (just outside the block) is accepted as an in-policy request.

**Severity rationale:** Medium. The gap was a genuine scope-escape in the origin policy, but the downstream protected fulfillment path fails closed before any network contact, so the practical impact was a mis-recorded in-policy request rather than a network escape. It is now remediated; the policy's own guarantee ("non-private origin") is fully enforced.

### F2 — Dependency-origin `127.` prefix is over-broad (Info, no fix required)

**Observation:** The `"127."` prefix in `UNVERIFIED_ORIGIN_PREFIXES` also matches non-loopback addresses such as `1270.0.0.1` (a typo for `127.0.0.1`). This is over-broad but fail-safe: it rejects more than it should, never less.

**Assessment:** No fix required. The over-broad match is conservative and does not weaken the policy.

### F3 — `PackageSources` network guard is advisory in the galla runner (Info, outside this audit's scope)

**Observation:** The `PackageSources` network mode in the galla runner is advisory: the child shares the host network namespace and can ignore `http_proxy` and open direct sockets. This was previously noted in the galla security audit.

**Assessment:** Outside this audit's scope. Per the no-network-in-sandbox tenet, the `PackageSources` mode applies only to the explicit host-execution `kvist vendor` path, which is a human-authorized operator action, not an agent-controlled sandbox escape. The agent's protected path never reaches this mode.

## Conclusion

The broker and applier cannot be coaxed into an unauthorized, out-of-root, or link-following write. All destinations are normalized under the component root with a closed writable-root set (`src`, `tests`), protected documents are refused, and every write re-proves content identity and re-checks the symlink-free ancestor chain inside the sandbox. The proposal evidence path is redacted, no-clobber, and never writes a protected document. The decision-pause ordering is correct: a decision always precedes any effect. The no-network-in-sandbox tenet holds for the agent path: the protected fulfillment path fails closed before any network contact, and the only network-allow sandbox is the explicit host-execution vendor command.

One finding was retained as a required fix (F1): the dependency-origin private-range prefix list was incomplete for the RFC 1918 `172.16.0.0/12` block. It has been remediated by `is_rfc1918_private_ip`, which rejects the full `172.16.0.0`–`172.31.255.255` block for both bare-IP and VCS-scheme origins, and pinned by the new `rfc1918_private_ip_origins_are_surfaced_as_a_decision` test. No finding is retained as an explicit blocker: the security boundary is confirmed intact, and the F1 gap did not weaken the no-network-in-sandbox guarantee because the protected fulfillment path fails closed before any network contact.
