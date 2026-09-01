<!-- kvist-compliance-review-version: 1 -->
# Source-Blind Compliance Review

## Scope and method

This is an independent, source-blind review of the delivered `sa-18` prompt,
setup, output, and reasoning slice. The reviewer did not inspect production
source, prior review content, Git state, history, diffs, root source or tests,
or project-level intent. Evidence was limited to this component's
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, fresh `IMPL.md`,
and child tests, plus the supplied final gate and security results. The
immediate parent `src/CONTRACT.md` was used only for the parent-facing prompt
and setup boundary.

`TODOS.yaml` records `sa-18-write-tests`, `sa-18-implement-code`, and
`sa-18-security-audit` as completed; this review task remains pending until
its status is transitioned by the authorized workflow.

## Evidence and comparison

| Reviewed guarantee | Source-blind evidence and assessment |
| --- | --- |
| Clean prompt output | The implementation record states that plain `run` output is provider content without framing, and JSON captures stdout while suppressing stderr. `cli.rs` tests parse the entire stdout as exactly `{"content":"answer"}` and verify no stderr, including a provider stderr progress write. This conforms to AR-REQ-OUTPUT-PRESENTATION and the child contract. |
| JSON encoding | The record specifies loss-tolerant decoding with U+FFFD. The child JSON test supplies invalid stdout byte `0xFF` and verifies the sole object is `{"content":"\uFFFD"}`. This confirms one valid JSON content result rather than a partial or invalid object. |
| Setup qualification | The record and design specify bounded capture through `run_supervised_capture`, with no provider output forwarded to setup presentation. The fixed prompt is exactly `Reply with exactly: OK`; child CLI and llama-server setup tests respectively record that exact argument/request. The failed-qualification test verifies no persistence and no extra test-prompt, second-authority, or interactive save-after-failure question. |
| Force and cancellation | The record states that `--force` saves a failed non-cancellation qualification with a visible warning, while cancellation is returned unchanged and never saves, even with force. The child test verifies forced persistence after a failed qualification; the child cancellation test verifies cancellation of a conventional probe and no fallback prompt. The supplied behavior record is the evidence for the force-plus-qualification-cancellation branch; no source was inspected. |
| Profile and command-template selection | The record describes direct argv rendering, complete JSON-string `{prompt_json}`, standalone-option removal for absent context/effort, and rejection when requested effort lacks `{reasoning_effort}`. Child command tests verify JSON escaping, rendering/removal behavior, and rejection of silently ignored effort; a CLI test verifies profile-selected per-prompt `high` effort. This conforms to AR-REQ-PROMPT-COMMAND. |
| Direct reasoning behavior | The record separates provider-supplied reasoning from answer text and identifies it as such. Direct-transport tests verify distinct Ollama unary fields and ordered reasoning/text stream events; the CLI rejects the incompatible JSON plus separate reasoning display combination. This conforms to the bounded, non-chain-of-thought presentation boundary described by the requirements and contract. |
| Exact model content | Unary and streaming `model` CLI tests verify stdout equals the provider text exactly (`local response` and `streamed response`), without a synthesized newline or wrapper. |
| Endpoint policy | The record specifies HTTP-only numeric loopback endpoints with an explicit port and rejects names, including `localhost`. Direct and Rig tests reject HTTPS, `localhost`, ordinary hostnames, credentials, and provider paths; the direct test also rejects a query. This supports the required numeric-only loopback policy. |
| Reasoning-effort transport | The direct transport test enumerates all seven values—`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, and `max`—for both llama-server and Ollama. It verifies the exact llama-server `reasoning_effort` value and Ollama `think` mapping (`false` for `none`, otherwise the exact string). |
| Rig pre-I/O rejection | The record states Rig rejects requested reasoning effort before runtime/provider I/O and rejects reasoning display before transport construction. Child Rig tests use an unserved loopback listener and verify both pre-I/O failures; the preflight-cancellation test likewise verifies cancellation before network access. |

## Gates and security

Supplied final results state that formatting, all-target/all-feature checking,
Clippy with warnings denied, and workspace all-feature tests pass. The supplied
definitive security-specialist result found no exploitable vulnerabilities.
No dependency manifest changed, and this review makes no provider-promotion
claim.

## Verdict

**`sa-18` verdict: conforming.** The delivered slice satisfies the reviewed
prompt presentation, setup qualification, command-template, direct reasoning,
endpoint, and Rig rejection requirements on the stated source-blind evidence.

This is not an overall certification of every planned `agent-runtime`
capability. The queue still defers transactional workspaces and promotion
(`sa-02`), strict Linux isolation (`sa-03`), native runtime/broker and host
authorization work (`sa-04`, with the broader layered-runtime review sequence
also incomplete), and later platform work. Those are explicit component-scope
deferrals, not discrepancies in the conforming `sa-18` slice. Remaining
uncertainty is limited to the source-blind evidence boundary: tests use local
fixtures rather than real provider installations, and the supplied gate and
security outcomes were not independently rerun by this reviewer.
