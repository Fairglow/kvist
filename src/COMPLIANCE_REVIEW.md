<!-- kvist-compliance-review-version: 1 -->
# Independent Compliance Review

## Scope and method

This is a clean-slate, source-blind review of the delivered root `src`
prompt/setup slices. I compared the root `REQUIREMENTS.md`, `CONTRACT.md`,
`DESIGN.md`, `TODOS.yaml`, `ROOT_CONTRACT.md`, the fresh root `IMPL.md`, the
immediate child `agent_runtime/CONTRACT.md`, and the relevant root tests
(`prompt.rs`, `wizard.rs`, and `json_outputs.rs`). I did not inspect production
source, this report's prior contents, Git history or diffs, child internals or
child intent/evidence, VISION, ARCHITECTURE, ADRs, runbooks, or chat history.
The fresh `IMPL.md` was treated as observed evidence, not as a compliance
claim.

The supplied final gates report passing format, all-target/all-feature checks,
Clippy with `-D warnings`, and all-feature workspace tests. The supplied
definitive security review found no exploitable vulnerabilities. No dependency
manifest change or provider promotion is claimed.

## `agn-prompt-ux` delivered slice

The `agn-prompt-ux` test and implementation tasks are marked completed in the
queue; its security task is also marked completed. The following requirements
are supported by the fresh implementation record and root tests:

* **Role-scoped model selection:** a per-invocation model selects from the
  configured models for the selected role. The JSON prompt test selects a
  non-default model and observes that model's result rather than the default.
  The implementation record also records developer, architect, and
  security-reviewer role selection and per-attempt model resolution.
* **Typed reasoning effort:** all declared values (`none`, `minimal`, `low`,
  `medium`, `high`, `xhigh`, and `max`) are accepted when the selected command
  exposes `{reasoning_effort}`. A request fails before provider execution when
  the selected command lacks that placeholder. This preserves runtime
  authority rather than injecting an unsupported option.
* **Plain output:** the observed text result is provider content only, with no
  synthetic completion trailer. The test asserts exact `answer\n` output.
  Interactive status labeling is separated to stderr as specified by the
  contract and implementation record.
* **JSON output and replacement decoding:** JSON mode captures one selected
  result and emits exactly one object with `content`. The invalid-byte test
  observes U+FFFD replacement. The result is escaped as JSON and no provider
  stream is forwarded into stdout; the successful JSON prompt test also
  observes empty stderr.
* **Bounds and malformed input:** oversized prompt files are rejected before
  provider execution, and configured provider output is bounded with an
  explicit failure when exceeded. Prompt files must be regular, non-link,
  bounded UTF-8 inputs. The implementation record additionally reports
  bounded supervision and lossy UTF-8 decoding for captured output.
* **Shell metacharacters:** the literal-content test passes `$(...)`, `;`, and
  other metacharacters through unchanged and verifies that no marker file is
  created. This is consistent with the contract/design requirement for
  shell-free command splitting. The provider may itself intentionally invoke a
  shell; that provider choice is not attributed to Kvist interpolation.
* **Authority separation:** host `prompt` execution requires explicit
  `--allow-host-execution`; setup qualification is a separate, narrowly scoped
  acknowledgement and is not a later prompt authorization. Role/model
  selection, prompt presentation, task policy, verification, and sandbox
  policy remain root responsibilities, while reusable provider supervision
  remains behind the child contract boundary. No host fallback is claimed for
  sandboxed task execution.

**Verdict — `agn-prompt-ux`: conforming delivered slice.** No discrepancy was
found in the reviewed obligations. This verdict is limited to the slice above;
it is not a certification of the entire root component or of provider
semantics outside the observed boundary.

## Relevant delivered setup slices

The earlier prompt-input/additive-setup slice (`agn-input-setup`) and the setup
behavior included in `agn-prompt-ux` are assessed separately from future
review/proposal/traceability work.

Evidence supports:

* bounded positional, file, redirected-stdin, and editor prompt acquisition,
  with conflicting explicit sources rejected;
* fixed qualification prompt `Reply with exactly: OK`, verified by the setup
  test that records the argument sent to the provider;
* failed qualification preventing persistence by default;
* `--force` permitting persistence after a non-cancellation qualification
  failure, with a visible warning;
* cancellation remaining terminal even with `--force`, with no configuration
  persisted;
* additive configuration updates preserving existing comments, unrelated
  settings, and existing models, followed by complete validation and atomic
  replacement;
* saved standalone profile selection materializing the selected profile's
  name and command into every chosen Kvist role. The role-binding test checks
  all three selected roles, so this is not merely an unresolved profile
  reference; and
* the corrected global-JSON setup boundary. Fresh `IMPL.md` and the
  end-to-end wizard test show setup transcript/status on stderr, qualification
  stdout and stderr not forwarded, and stdout containing exactly one result
  object even when the provider writes both streams.

The setup contract also states that choosing an already saved reusable profile
does not generate or execute a new qualification command. That behavior is
consistent with the saved-profile boundary and avoids silently expanding host
authority.

**Verdict — delivered setup behavior: conforming for the reviewed obligations.**
The prior JSON setup discrepancy is fixed and is not retained as a finding.
The setup acknowledgement remains deliberately narrow: it covers only the
generated qualification command and does not authorize subsequent host prompt
execution. Qualification is not represented as sandbox isolation.

## Security and quality gates

The supplied security specialist found no exploitable vulnerability in the
reviewed change. The tests exercise the principal boundary cases requested for
this slice: source conflicts, size limits, link rejection, unsupported effort,
invalid UTF-8, output exhaustion, literal metacharacters, qualification
failure, force behavior, cancellation, configuration preservation, saved-role
binding, and JSON stream separation. The supplied formatter, all-target and
all-feature checks, Clippy `-D warnings`, and workspace all-feature tests pass.
These results are corroborating evidence, not proof that every provider or
platform behavior is covered.

## Uncertainties and evidence limits

Provider-specific protocols, child runtime internals, and child tests were not
reviewed; only the declared child contract and root-facing evidence were
available. The review therefore does not certify external provider behavior,
network semantics implemented by a provider, or unobserved operating systems.
The root requirements and design identify Linux as the executable support
boundary. Queue task status is recorded as project state and was not changed
by this report; a pending compliance-review task is not itself evidence of a
functional defect.

## Deferred scope and overall verdict

The advisory document-review/receipt and acceptance workflow
(`REQ-ADVISORY-DOCUMENT-REVIEW`), observed-intent proposal/comparison workflow
(`REQ-OBSERVED-INTENT-PROPOSAL`), and contract-clause traceability workflow
(`REQ-CONTRACT-VERIFICATION`) are explicitly described by the root intent as
planned capabilities. They are not implemented claims of these delivered
slices and remain deferred. Project-level acceptance of VISION,
ARCHITECTURE, root intent, ADRs, or referenced schemas is likewise deferred.
No missing future workflow is misclassified as a failure of the conforming
prompt/setup behavior.

**Overall verdict:** the reviewed `agn-prompt-ux` and relevant setup slices are
compliant with the supplied root intent and observed evidence, with no
high-confidence security finding. The verdict is bounded to those delivered
slices; the explicitly deferred review, proposal, traceability, and provider
promotion scopes remain outside compliance.
