# Continuous execution: scoped implementation observation

This independent evidence record describes current Maerg source and tests only.
It is not a review receipt, acceptance, compliance certification, security
assurance, or certification that generated model reports are true.
No intent artifacts, prior implementation records/reviews, peer implementations,
Git history/diffs, or session history were inspected. No tests were rerun.
References below are repository-relative source locations.

## Public execution behavior

- `task run COMPONENT_DIR [TASK_ID]` requires the component argument; an exact
  ID runs only that task, with no item-prefix or special `all` expansion.
  Omitting the ID enters continuous execution. Both normal and JSON dispatch
  use this selection (`maerg/src/cli.rs:472-485,1048-1072,1584-1605`;
  `maerg/src/task_commands.rs:5193-5224`).
- Continuous execution holds one component operation lock, re-reads the queue,
  and chooses the first declared pending task whose direct and transitive
  dependencies are completed. Item policy additionally confines selection to
  the active inferred item. No ready task produces a retained paused ledger and
  blocker/status diagnostic, not automatic unblocking
  (`task_commands.rs:292-315`; `task_commands/continuous.rs:44-318`).
- Successful implementation work undergoes sandboxed verification. Continuous
  mode then marks execution completed without human finalization; an explicitly
  selected implementation task instead remains pending human disposition.
  Successful test/review tasks transition to completed; surfaced decisions
  transition to awaiting-decision; execution/verification failures block the
  task and stop later scheduling (`task_commands.rs:4446-5162`).
- Final execution writes an awaiting-approval marker listing review paths and
  queue/source digests, and explicitly reports that nothing was automatically
  accepted. The continuous path does not assign acceptance IDs
  (`task_commands/continuous.rs:212-230,606-637`;
  `maerg/tests/task_commands.rs:1940-2064`).

## Policies, retained state, and recovery

- `[task_run].commit` strictly accepts `item` (default), `component`, or `manual`.
  This policy is included in authenticated execution approval; changing it
  requires reapproval (`maerg/src/config.rs:517-534`;
  `task_commands.rs:5475-5506,5589-5596,6192-6209`).
- Item membership uses exact lifecycle suffixes and longest matching prefixes,
  including implementation subtasks under a test stem. Unfinished interleaved
  items are rejected. Item commits follow completion of all inferred members;
  component policy commits only at final completion; manual policy skips Git
  cleanliness preflight and automatic commits, but still inventories owned files
  (`task_commands/continuous.rs:83-86,126-163,320-477`).
- `.kvist/continuous.json` is atomically replaced, directory-synced,
  version-checked, and HMAC-authenticated using the approval secret. It records
  policy/definition identities, phase, active/completed items, baseline inventory,
  expected HEAD/effects, and a prepared checkpoint
  (`task_commands/continuous.rs:7-42,479-574`).
- Non-paused resumes reject changed recorded effects; paused resumes permit
  inspected edits and definition changes if no checkpoint is pending. A pending
  checkpoint prevents policy-identity changes and is finished before scheduling.
  Completed task statuses are retained rather than replayed
  (`task_commands/continuous.rs:90-182,336-390`).
- Unresolved attempt journals fence further execution. Per-task preparation binds
  approval, runner, write scopes, queue digest, and operation-lock identity.
  Recovery is explicit and restricted: `execution-did-not-start` requires
  authenticated pre-launch failure evidence; removing a stale lock does not
  resolve an uncertain attempt (`task_commands.rs:1403-1485,2250-2445,4592-4628`).
- SIGINT is checked at continuous safe boundaries and bridged to active model
  cancellation by a 25-ms watcher. Cancellation during execution can block the
  current task; it is not completion or rollback
  (`maerg/src/agent/cancellation.rs:1-53`;
  `task_commands/continuous.rs:165-193`; `maerg/tests/task_commands.rs:2448-2491`).

## Exact checkpoint ownership

- Owned paths are only component `src/**`, `tests/**`, `TODOS.yaml`, `IMPL.md`,
  and `.kvist/continuous-reviews/**`; the ledger, attempt journals, live logs,
  intent files, and unrelated staging are outside checkpoint ownership.
  Automatic mode requires owned worktree/index content and modes to match HEAD
  initially (`task_commands/checkpoint.rs:25-34,129-148,606-629`).
- Checkpoints hash raw bytes without clean filters, build a tree using an
  isolated index, preserve executable modes/deletions, honor configured signing,
  suppress Git hooks/fsmonitor, and add the Copilot coauthor trailer. A prepared
  commit is persisted before publication (`checkpoint.rs:388-429,714-826`;
  `continuous.rs:336-364`).
- Publication validates the exact parent, changed paths/blobs, worktree,
  branch, and staging. It acquires an exclusive index lock, copies the current
  index, changes only owned entries, and advances HEAD through a prepared
  compare-and-swap ref transaction before installing the copied index.
  Retrying an already-advanced ref is idempotent. Stale index-lock recovery
  requires matching commit/device/inode ownership and a demonstrably dead PID;
  unknown or live locks are retained (`checkpoint.rs:860-1159`).

## Independent model phases and controls

- Security tasks receive a source/test audit frame. Compliance tasks first
  request source-only observations, persist an attempt-specific observation,
  replace `IMPL.md` with that observation record, then request comparison using
  local intent, root/optional parent contract, the new record, and verification
  evidence—not a raw source field. Observation evidence survives comparison
  failure; final reports are attempt-specific plus latest-per-task files
  (`continuous.rs:646-817`).
- Every generation constructs fresh system/user messages, advertises no tools,
  selects no tool, rejects returned tool calls, and requires strict nonempty
  JSON `{report, decision_required}`. Decision flags are OR-preserved through
  aggregation and observation/comparison results. They gate execution status,
  not human acceptance (`maerg/src/agent/review.rs:197-332,347-516`;
  `continuous.rs:777-798`).
- Oversized evidence is UTF-8-fragmented without dropping input bytes; comparison
  fragments retain shared observed/test evidence and split intent. Fresh reports
  are reduced/synthesized, with failure if bounded reduction cannot progress.
  All requests, retries, and both compliance phases share one task timeout and
  optional cumulative token budget. Unknown/failed usage consumes conservative
  reservations (`agent/review.rs:197-345`; `agent/accounting.rs:1-94`).
- Explicit `--allow-control` enables cancel only during review; guidance is
  rejected without recording its text. Required redacted diagnostics/live events
  record limits, generation results, controls, and disposition; recording failure
  fails the review. Redactions combine all role values and allowed sandbox
  environment values (`agent/review.rs:36-194`; `agent/live.rs:20-174`;
  `task_commands.rs:5432-5463`).

## Bounds, shell behavior, and evidence limitations

- Inventories are bounded to 4,096 entries, depth 64, 1 MiB/file and 8 MiB
  material; encoded source frames also cap at 8 MiB, ledgers at 192 MiB, indexes
  at 64 MiB. Checkpoints reject links/hardlinks, non-UTF-8 paths, and unsupported
  file modes; support 40/64-character Git IDs and linked worktrees. Traversal
  depends on Unix descriptors and Linux `/proc/self/fd`
  (`task_commands.rs:3696-3702`; `checkpoint.rs:152-360,697-712`).
- Review requests use configured context limits or a provisional 32,768-token
  context, response reserves, and at most 65,536 evidence bytes/request.
  Intermediate/final reports cap at 1,024/8,192 characters. Role output limits
  bound each transport response and combined serialized review log, not total
  ledger/report/live-journal disk consumption (`agent/review.rs:142-173,203-245,347-516`;
  `maerg/src/config.rs:261-262`).
- Shell `run` requires a component, forwards to task dispatch, and does not ask
  run confirmation. Focus insertion edits the real Reedline buffer using shared
  component-slot parsing; explicit/partial values win, deletion is remembered
  until the next submission, quoting and dash-leading paths are handled.
  Paste/history use a suspend-and-edit path that clears earlier undo history;
  insertion itself is undoable (`bark.rs:625-651,1125-1135,1393-1437,1683-1697`;
  `bark/prefill.rs:33-169`; `bark/completion.rs:286-340`).
- Material gaps: inferred items are not checked for all four lifecycle roles;
  queue validation checks predecessor roles only for tasks that exist
  (`continuous.rs:393-466`; `task_queue.rs:851-896,1068-1088`). Paused edits do
  not automatically invalidate completed reviews or rerun verification
  (`continuous.rs:90-124,195-230`).
- Source frames cover `src/tests` descendants, not root manifests/build inputs;
  binary content is digest-only. Filename exclusions do not remove embedded
  intent, templates, fixture text, or nested component content. Verification
  context selects the last successful recorded verification per command among
  completed implementation tasks in declared order, flags scope freshness, and
  does not reject stale evidence; stdout/stderr were bounded to 16,384 bytes
  (`continuous.rs:838-966`; `task_commands.rs:4882-4911`).
- Model-input evidence is not scrubbed by output redaction. Review diagnostics
  apply substring redaction after JSON encoding, so JSON-escaped values need not
  match their original configured substrings (`agent/review.rs:146-152,404-425,484-516`).
  Checkpoint Git/signing subprocesses have bounded capture but no timeout or
  cancellation checks (`checkpoint.rs:431-467,978-1033`).
- Read tests cover scheduling, policy counts, refusal/decision stops, failed
  checkpoint retry, explicit selection, fragmentation request sizing, malformed
  reports, ledger forgery, cancellation, Git ownership/recovery, and real PTY
  editing (`maerg/tests/task_commands.rs:1940-2491`;
  `checkpoint/tests.rs:71-412`; `maerg/tests/shell_smoke.rs:277-533`).
  Mock model responses and structural validation do not establish truthful
  findings, semantic synthesis coverage, actual full-component authoring, or
  recovery from every abrupt-crash interval.
- Host-attributed verification, not personally executed: `cargo test --offline
  --locked --quiet -p kvist --lib`: 521 passed, one pre-existing ignored, no
  failures; `--test task_commands`: 65 passed; `--test dogfood_boundary`: 78
  passed; `--test shell_smoke`: 2 passed. Dogfood test source was not inspected.
