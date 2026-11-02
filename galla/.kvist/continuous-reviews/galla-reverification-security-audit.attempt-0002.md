# Galla: human decision needed

## What is paused?

Task `galla-reverification-security-audit` is `awaiting-decision`.
It depends on `galla-implement-code`; that dependency's status was not inspected.
The task says earlier placeholder output was not valid independent evidence.
Its purpose is to obtain genuine independent findings and retain their disposition.
This summary does not establish that the current report fulfills that purpose.

**You are deciding how to investigate and resolve the concerns below—not whether
to certify Galla, accept every allegation, or mark the audit complete.**
The retained report requests a decision, principally because of fragments F2 and F4.
There is no automatic pending/reset/completion transition from this summary.

## Evidence and limits

- Raw report: [attempt-0002 JSON](galla-reverification-security-audit.attempt-0002.json).
- Task: `galla-reverification-security-audit`; attempt: `attempt-0002`.
- Recorded `source_digest`:
  `sha256:d2eee41cd97691034dce728c27cffa67c0fb0e8881d3c76065a954e7612043c2`.
- The digest is copied attribution, not an independently checked source identity.
- JSON fields: `observed: null`, `decision_required: true`,
  `human_acceptance: false`. The report aggregates five source-only fragments.
- This presentation used only that JSON, Galla's REQUIREMENTS, CONTRACT and
  DESIGN, and the single paused task entry.
- No implementation or test source was read; no tests, commands exercising
  Galla, or vulnerability investigation were performed.

**No implementation, security, or test-result facts were independently verified.**
These are unverified model findings that may be wrong, incomplete, or fragmentary.
An assertion about a test in the report is not evidence that the test was run.
This Markdown is a manually opened reading aid, not a protected receipt,
acceptance record, compliance result, or certification.

## The main concerns, in plain English

### 1. Can dependency downloads bypass the intended boundary? (F2)

The model alleges that `PackageSources` lacks `--unshare-net`, allowing direct
network access around the proxy. It also describes an HTTP-only proxy without
HTTPS CONNECT support and a fixed 4096-byte read, alleging truncation or
request-confusion risks.

If accurate on the active execution path, this could undermine source-restricted
downloads or prevent required HTTPS sources from working safely.
However, an absent flag in a fragment is not proof that no equivalent isolation
exists elsewhere. Protocol limitations alone do not prove an exploitable bypass.
Needed later: the actual namespace/proxy wiring, direct-egress denial evidence,
and bounded HTTPS, redirect, address-pinning and malformed-request tests.

### 2. Is the active cache path using the promised safeguards? (F2, F1)

F2 alleges that `inspect_and_promote_cache` omits manifest checksums, copies
non-atomically, performs only a magic check, and does not use the tested promotion
primitive. It also alleges that file/scratch limits inspect only immediate files,
allowing nested files to evade limits.
F1 alleges that same-content generation reuse checks bytes but not permissions,
potentially accepting writable or setuid files as immutable.

If accurate and reachable, these would threaten cache integrity or resource bounds.
But the contract both claims atomic promotion and says the command boundary never
invokes the descriptor-relative primitive; DESIGN describes that primitive's use.
Those statements need reconciliation with the active path, not an assumption
that a tested helper protects production or that the allegation proves poisoning.
Needed later: reachable publication/reuse paths, recursive accounting, manifest
and mode checks, collision/tamper tests, and host non-mutation evidence.

### 3. Are approved source identities actually enforced? (F4)

The model alleges that `validate_source` accepts `sha256(name)` as an alternative
to the origin-bound registry identity, then returns the derived identity.
If accurate, accepting an incorrect submitted identity would conflict with the
existing identity-mismatch rejection obligation; it is not a new feature choice.
The fragment does not establish the full downstream authority or exploit impact.

F4 also flags a fallback from `parse_production` to `origin::parse` and duplicate
grant destinations being collapsed in a `BTreeMap`.
It explicitly lacked the origin module and earlier uniqueness checks.
Missing context is not proof that unsafe origins or duplicate grants are accepted.
Needed later: full validation ordering and field-substitution, duplicate-grant,
and forbidden-origin rejection evidence.

### 4. Other leads, with important qualifications (F1, F3, F5)

- F3 alleges non-NUL control characters in URL paths, rejection of legitimate
  digit-containing hexadecimal-looking hostnames, and an IPv4-compatible IPv6
  classification gap. Actual accepted production requests remain unverified.
- F3 raises Bubblewrap selection through PATH/file checks, executable identity
  races, shallow symlink checks, an unexplained destination rule containing
  `unapproved`, and untested process-supervision settings. It lacked later checks.
- F1 notes same-user staging interference, but also reports no-follow,
  no-replacement and descriptor/identity mitigations. No successful attack is shown.
- F5 saw tests only, not enforcement code. Its assertions cannot verify confinement.
  Missing dependencies, types, modules and truncated fragments are evidence gaps,
  not demonstrated defects in the complete component.

## What is already required, and what needs a design choice?

Existing intent requires fail-closed enforcement, namespace isolation and strictly
mediated egress; canonical HTTPS production sources excluding forbidden addresses;
independently derived, origin-bound identities with mismatch rejection; bounded
resources; and immutable, checked, normalized, atomic cache construction.
See REQUIREMENTS' acquisition/production sections, CONTRACT's behavioral/security
sections, and DESIGN's algorithms/security sections. These are obligations already
written down, not optional protections to trade away to finish this task.

Actual mismatches require remediation or explicit human arbitration.
How to wire the proxy, implement safe HTTPS transport, connect cache primitives,
or temporarily reject an unenforceable request needs a coherent implementation plan.
Changing supported behavior or guarantees would instead be a deliberate product
and intent decision, with consumer consequences—not merely “accepting the audit.”

## Coherent alternatives and consequences

1. **Approve a bounded investigation/remediation plan (recommended).** Preserve
   existing guarantees; map each allegation to the active path and obligation,
   distinguish disproved claims from gaps, then add targeted tests and fixes.
   Where safe enforcement cannot be established, fail closed. Availability may
   remain limited while work and independent review are completed.
2. **Request clarification before authorizing changes.** Ask for a narrower,
   evidence-backed explanation of F2/F4 and cache wiring. The task stays paused;
   this avoids premature fixes but does not resolve the enforcement uncertainty.
3. **Reject or redirect a proposal.** Identify unsupported claims or unsuitable
   mechanisms and request a different plan. Rejecting model findings does not
   establish safety or remove the independent-audit obligation.

The first approach is advisory: it prioritizes claimed trust-boundary failures
without treating fragmentary output as truth. Approving this plan is **not**
approving the findings, final implementation, security posture, or task completion.
Do not weaken security obligations simply to close the task.

## What to do now

Choose an option and give the development assistant an explicit instruction, e.g.:

> Approve a bounded follow-up plan, not the findings or implementation. Resolve
> F2/F4 and cache-path uncertainty against current intent; propose tests and fixes,
> preserve fail-closed guarantees, and return unsupported claims for human review.

For clarification while the task remains paused:

> Explain which F2/F4 claims have complete-path evidence, which lack context, and
> what evidence would distinguish a real mismatch from a false positive. Propose
> the smallest follow-up scope; do not change intent or task state yet.

To reject/redirect:

> I do not approve the proposed conclusion or mechanism. Record my rationale and
> propose an alternative preserving the existing security obligations; keep paused.

With human review, record the decision rationale and accepted changes in the
relevant intent/TODOS artifacts, as appropriate; do not fabricate dispositions.
Only if intent changes, review those changes and run `kvist component accept galla`.
That structural acceptance is not security certification or approval of findings.
There is no decision-approval or clarification command being introduced here.

After resolution and any required acceptance, explicitly resume this task with:

```sh
kvist task run galla galla-reverification-security-audit
```

Bare continuous execution does not choose an `awaiting-decision` task.
Do not automatically reset it, mark it pending, or mark it complete.
