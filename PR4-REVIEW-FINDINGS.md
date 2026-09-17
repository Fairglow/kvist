# PR#4 review findings and fix plan (temporary working document)

Status legend: [ ] pending · [~] in progress · [x] done · [-] declined (rationale given)

## High importance

- [x] H1. ADR number collision + content contradicts implementation
  - `docs/decisions/0007-brokered-model-transport-and-isolated-effects.md` collides
    with existing `0007-interactive-shell-and-library-relegation.md` and describes a
    rig-core transport decision that this branch reversed (rig removed entirely).
  - Fix: rename to `0009-brokered-model-transport-and-isolated-effects.md` and rewrite
    to describe the actual implementation: native direct transport as the sole model
    transport, rig-core removed, brokered effect loop, liveness probe + bounded retry
    under a shared wall-clock budget, non-loopback commands refused. No historical
    rig artifacts or misleading claims.
- [x] H2. Brokered effect loop not wired (model output cannot produce changes)
  - `execute_host_turn` sent `tools: Vec::new()`, returned text only; `authoring`
    module and `InvalidAuthoringIntent` were dead code; comment in `execute_agent`
    described behavior that did not exist.
  - Fix: model turn now advertises the closed authoring tool set (`write_file`,
    `edit_file`); `authorize_turn` reduces untrusted intents to `CheckedIntent`s;
    each authorized effect is applied by the kvist binary itself inside the effect
    sandbox (`authoring-apply`, read-only intent mount at `/workspace/authoring/intent.json`,
    toolchain grant on argv[0]); dropped intents fail the turn (fail-closed); effects
    recorded in trajectory + agent log; `edit_file` semantics made complete
    (`destination` + unique `replace` + `content`); no-follow opens in the applier.
- [x] H3. Non-loopback commands ran unsandboxed on the host (+ double execution)
  - Any command without an `http(s)://` URL was spawned as a plain host subprocess,
    then re-executed in the sandbox on success. Security regression vs main.
  - Fix: fail closed. The selected model command must target a numeric loopback
    model gateway; anything else is a turn failure with a clear, actionable error
    (`AgentCommandNotModelGateway`). Removed `run_host_subprocess`, `reap_terminated`,
    `StreamCapture`, and the command re-execution path entirely.

## Medium importance

- [x] M4. Model-phase wall-clock could reach ~3x the configured timeout
  - Deadline was per-attempt; probe time was extra.
  - Fix: `TurnBudget` — one shared budget (profile timeout) covering liveness probe,
    every turn attempt, and retry backoff; per-attempt transport deadline is the
    remaining budget. CONTRACT/DESIGN updated.
- [x] M5. `--stream` silently dead (`stream_output` never read)
  - Fix: when `stream_output` is set, the host turn uses `transport.stream()` and
    relays text deltas to stdout; assembled turn still feeds the log/evidence.
    Retries only when nothing was emitted yet.
- [x] M6. Evidence regressions
  - stderr merged into stdout (structured `AgentExecutionRecord.stderr` always empty)
    -> redacted separately now; `ModelTurn.usage` discarded -> token accounting
    surfaced; failed turn left an empty log while the blocked reason pointed at it
    -> failure reason recorded in stderr/log; trajectory `state_mutated: success`
    lied for text-only turns -> now reflects actually applied effects; dead
    `.kvist/runs/*.json` run-record read removed (brokered path never writes it).

## Low / polish

- [x] L10. `CheckedIntent` for `edit_file` was not a complete execution spec
  - Fix: `replacement` field carried on `CheckedIntent`; applier re-derives the
    result from the current file and verifies `content_identity` before writing.
- [x] L11. Symlink TOCTOU in authoring module
  - Fix: classification uses `symlink_metadata` (symlink destinations are dropped);
    applier walks the destination no-follow (real directories, regular non-symlink
    final file, `create_new` for creates).
- [x] L12. `expect`/`unreachable!` in `classify_intent`
  - Fix: defensive `Err` paths instead; no panics remain in the broker.
- [x] L13. Dogfood mock model thread was detached and unstopable
  - Fix: `LocalModel` owns a `JoinHandle` + stop flag; `Drop` unblocks `accept` with
    a sentinel connection and joins.
- [-] L14. Committed attempt journal contains machine-absolute paths
  - Declined: `sandbox_runner/.kvist-attempts/sandbox-runner-reverification-security-audit.jsonl`
    is genuine durable evidence of a real attempt (repo convention commits journals);
    rewriting it would falsify the record. The absolute path is the true log location
    on the machine where the attempt ran.
- [x] L15. No `engine/TODOS.yaml` entries traceable to this change
  - Fix: added `brokered-turn-*` queue (write-tests + implement-code completed;
    security-audit + compliance-review pending, ordered per project rules).
- [x] L16. Contract drift: `ConnectionReset` retried in code but absent from
      CONTRACT/DESIGN retry lists -> updated, plus budget/effect-loop documentation.
- [x] L17. `kvist.toml.example` agent section confusing (`# TODO:` commands)
  - Fix: rewritten to explain the loopback-gateway requirement and the synthesized
    default command accurately.

## Validation

- cargo build --workspace; cargo clippy --workspace --all-targets; cargo fmt --check;
  cargo test --workspace (incl. 60-test dogfood boundary suite); new tests for:
  retry loop, shared budget, non-loopback refusal, effect loop dispatch (engine),
  real-runner in-sandbox effect application + symlink refusal (protocol), edit_file
  authorization semantics, no-follow applier, tool definitions, streaming relay.
- The 40-test `engine/tests/task_commands.rs` integration suite was migrated to the
  brokered design: agent `command_template`s now point at a hermetic in-process
  loopback mock gateway (`MockGateway`; text, tool-call, SSE-stream, delayed, and
  echo-prompt variants) standing in for the local model, so the suite exercises the
  full supervised path (effect loop, shared budget/timeout, lock lifecycle and
  unlock, streaming relay, redaction) without a real model. Dead fake-runner cases
  from the in-sandbox-agent era were removed.
- Final gate run: `cargo test --workspace` — 684 passed, 0 failed; clippy and
  `cargo fmt --check` clean.
- Note: `engine` shows as `stale` in `kvist status` because its recorded intent
  revision hashes predate the CONTRACT/DESIGN updates in this branch (and the
  engine was already stale at HEAD). `kvist component accept engine` must run
  after review; it is currently blocked in this checkout because `kvist.toml`
  is gitignored (machine-local config, pre-existing condition), which the
  acceptance path requires to be VCS-tracked.
