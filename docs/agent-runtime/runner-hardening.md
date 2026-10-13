# Security-first workspace agent

This delivers the near-term context, coding-tool, lifecycle and headless
recommendations from the [upstream comparison](upstream-agent-comparison.md).
The implementation remains Kvist-owned Rust: no upstream engine, execution
defaults, licensing terms or credential store was imported.

## Authority comes first

`skott` is a workspace coding agent, **not the maerg's protected task
broker**. The selected workspace is writable, including its intent documents.
It exposes no maerg authorization, acceptance, canonical-evidence or promotion
API. That is not protection against changing those files through the workspace
shell. The independent comparison found a conflict between this local design
and the root contract's protected-artifact and shell rules
([F4](runner-intent-review.md#f4--p1--contradictory-intent-global-protected-artifact-and-shell-rules)).
Explicit human scope arbitration is required before claiming global conformity;
no exception or acceptance has been silently recorded. Use the maerg's
protected broker for governed task authoring.

Default tools run through the separately installed Linux/Bubblewrap runner.
Authoring tools have no network; model HTTP runs on the host against explicitly
configured numeric-loopback endpoints. Missing isolation is an error, never a
host fallback. The existing interactive `--allow-host-execution` opt-out is
explicitly unconfined, labeled in the UI and model instructions, and single-turn
unless `--host-turns` is supplied. Headless execution forbids that opt-out.

The shell denylist is defense in depth, **not isolation**. A shell inside the
declared writable workspace can change that workspace broadly. Native file
tools do not grant more authority than that shell.

## Installation and llama-server use

Install both runner binaries together:

```sh
cargo install --locked --path skott --bins
```

The main executable needs `skott-file-tool` beside it. The helper is a
small filesystem primitive, not a sandbox when invoked directly. Do not place
either it or the installed enforcement runner inside the writable workspace.
Configure `[sandbox].runner` and `.backend` for the independently installed
`galla-runner` and Bubblewrap. Writable workspace parents are required
for private temporary payload staging; filesystem-root workspaces are rejected.

Configuration discovery uses `./skott.toml`, then the per-user
`skott/config.toml`, then `XDG_CONFIG_DIRS` (default `/etc/xdg`).
The repository example is named **`skott.toml`**, so select it explicitly:

```sh
skott --config skott.toml --effort none
skott --config skott.toml --headless --json --effort none \
  --context-limit 8192 --response-reserve 1024 \
  --max-run-secs 180 --max-run-tokens 100000 \
  'Read the relevant file, make one exact preimage-bound edit, then verify it.'
```

Use a model with a suitable chat template and functioning tool calling.
`none` was used for the local qualification; provider-supported reasoning
effort remains configurable. A truncated answer or proposal fails explicitly:
increase the output reserve within the model window, or reduce the task/read
size, rather than treating partial output as completion.

Headless plain stdout contains only a successful final answer. `--json`
emits ordered envelopes `{schema_version:1, sequence:N, event:...}`, including
scope metadata and a final `run_summary` with `disposition`. Startup failures
or broken output destinations may prevent a terminal envelope. Unsuccessful
runs exit nonzero; cancelled runs exit 130. Human diagnostics use stderr.
Plain text escapes terminal controls except LF/tab; JSON retains original text.

## What improved

| Area | Delivered behavior |
| --- | --- |
| Context | Complete canonical request preflight including system instructions and full schemas; enforced provider output reserve; complete tool-group compaction preserving systems/current goal/newest group; explicit irreducible overflow |
| Coding | Native bounded reads, listing, literal discovery/search and exact single-occurrence SHA-256 edits; stable pagination and CRLF/missing-newline preservation |
| Staging | Mode-0700 temporary directories and mode-0600 payloads outside the workspace; read-only payload/helper grants; cleanup on every return; no provider-ID staging paths |
| Loop | Validated terminal reasons and call identities before effects; paired rejection/interruption results; repeated-argument correction/circuit breaker; previous answers cannot survive failed followups |
| Limits | Whole-prompt wall and estimated-token budgets across attempts, backoff and tools; enforced per-request generation cap; cancellable clipped retry waits |
| Processes | Shared nonblocking stdin/output supervision, exact combined capture cap, owned process-group cleanup and finite drain/reap windows; retained pipes fail explicitly |
| Recording | Fallible synchronized pre-effect dispatch; separate result records, content hashes and process flags; explicit terminal dispositions and scope/policy/limit provenance |
| Lifecycle/UI | Owned joined workers; retained-sender/full-queue teardown; persistent scope label; safe fences and styled cell wrapping; complete code/table tails; notices survive resize and revealed reasoning reflows; bounded no-follow history and large replay viewports |
| Providers | Existing llama-server and Ollama retained; output cap translated to `max_tokens` / `options.num_predict`; explicit llama finishes preserved and missing finishes marked unknown; malformed streamed fields rejected; cadence tracks decoded generation, not heartbeat bytes |
| Startup | Bounded configuration reads and payload-free diagnostics; canonical existing-directory overrides in both modes; executable interpreter gates; pure registries advertise only Generic without probing |

Token estimates are byte-aware heuristics, **not exact tokenizer guarantees**.
The shared token budget charges complete estimated input plus reserved output
for every attempt, including retries; it is not provider billing accounting.
Production startup uses explicit CLI/model context or bounded selected-model
serving-capacity discovery; it has no assumed 8192-token context fallback.
The initial output reserve is explicit CLI/model configuration or
`min(8192, max(1, context/4))`. Default prompt limits remain 1800 seconds and
1,000,000 estimated attempt tokens; sandbox loops cap at 50 turns. Length
recovery can enlarge the output reserve before effects, sharing those limits.
Compaction triggers near 75% and targets 65%, retaining outstanding goals and
complete tool groups rather than discarding history after a fixed number of
turns. Provider usage may be unavailable; journals use null, and UI/streamed
estimates are explicitly distinguished from measured usage.

Normal sandbox PATH Cargo forces `--offline --locked` against the private,
read-only vendor snapshot. Provision a matching `Cargo.lock` separately;
missing or stale locks fail rather than being created or updated by the build.
Explicit alternate Cargo paths are not rewritten, and standalone authoring
still permits workspace edits. Bounded compaction summaries may lose older
references; this is not a guarantee of complete retained history.

Unsafe Rust is not categorically forbidden: it must be necessary, minimally
scoped, justified by documented safety invariants, independently reviewed, and
covered by targeted verification. The runtime currently uses two unsafe
`sigaction` calls for signal-handler installation. Its native tests exercise
installation and forwarding, not a general proof of signal safety.

Built-in process supervision polls idle pipes every 5 ms, permits 250 ms of
post-exit/termination draining and bounds reaping to one second. Cleanup
failures are explicit. Escaped host descendants that close every owned pipe
and uninterruptible kernel work are not claimed terminated. Injected
collaborators and synchronous model callbacks must cooperate with cancellation;
the library cannot preempt arbitrary caller code.

Sandbox preparation streams identities through regular no-follow descriptors
in 64-KiB blocks, rejects files above 256 MiB, and honors prompt cancellation.
Workspace scanning caps 1,000,000 entries, depth 128 and 32 MiB of charged
directory-path bytes (not total allocator memory); both stages share a 30-second cooperative preparation
limit. Exceeding a limit fails closed with guidance to narrow the workspace.
This is not an atomic filesystem snapshot or a substitute for the installed
namespace boundary.

File-tool schemas and exact bounds are in
[the component contract](../../skott/CONTRACT.md#native-file-tool-inputs-and-results).
Read digests cover whole bounded files. Native JSON stdout fits 7000 bytes;
the combined model-facing result with process metadata fits 8 KiB. Prefer
small pages and the returned continuation offsets. Exact edits reject stale,
absent, multiple and overlapping matches, never fuzzy-guess a replacement.
Replacement preserves mode bits, not ownership/ACLs/xattrs or hard-link aliases.
Preimage revalidation is **not atomic CAS against arbitrary external writers**.

## Records, privacy and interrupted effects

Journals are private local operational records, **not maerg evidence**.
Argument/output values are represented by hashes/shape in the journal.
Diagnostic transcripts may contain sensitive user/model/tool text and bound
each text item to 64 KiB. They are not guaranteed complete or secret-free.

Interactive logs default to `.skott/runs`, which the workspace agent
can modify; held descriptors prevent path redirection, not tamper-proof storage.
Use an external private `--log-dir` when integrity matters. Interactive
`--no-logs` is deliberate and visible in help; it is forbidden headlessly.
Headless logs must be non-linked/private and outside the writable workspace,
defaulting to `$XDG_STATE_HOME/skott/runs` or
`$HOME/.local/state/skott/runs`.

History/replay is advisory display only. It rejects nonregular/link paths,
caps files at 5 MiB before and during reading, and caps directory enumeration.
JSON recording preserves original text; human terminal rendering escapes
controls. A dispatch without a result represents an **unknown effect**.
Inspect the workspace; do not automatically replay it.

Visible transcript retention is 5000 rows; this is not an aggregate memory
quota. Very wide source indentation still conflicts with the unconditional
panel-width promise and needs explicit policy arbitration. Indivisible glyphs
wider than a viewport and font/emulator differences are also not universally
qualified. See the independent comparison for the exact remaining scope.

## Qualification and remaining scope

The [review and qualification record](runner-hardening-review.md) records
actual gates, independent reviews and any discrepancies. The live local tests
qualified bounded streaming plus real read/edit/verify with
`Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL` at `http://127.0.0.1:9931`.
A separate model-free trial verified confined native writes/reads, unavailable
host-private files and denied access to a host-local TCP listener.

```sh
KVIST_LIVE_GALLA_RUNNER=/path/to/installed/galla-runner \
  cargo test --locked -p skott --test live_llama \
  -- --ignored --test-threads=1
```

Optional `KVIST_LIVE_LLAMA_ENDPOINT` and `KVIST_LIVE_LLAMA_MODEL` select the
server/model. These tests are opt-in; missing isolation must fail, not skip
into host mode. Ollama is covered by deterministic transport fixtures, not a
live qualification claim.

Automatic resume/replay, remote TLS/credentials, arbitrary MCP/plugin tools,
parallel effectful tools, fuzzy/merge-aware edits, automatic VCS promotion and
LSP daemons remain deliberately deferred. They need explicit capability,
ownership, credential, effect-recovery and approval designs before integration.
These are not hidden compatibility shims or promises of present support.
