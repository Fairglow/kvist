# Continuous execution: current source observation

## Evidence boundary

Derived from current Maerg Rust source and test source only. No intent documents,
queues, implementation records, previous reviews, execution logs, Git history or
diffs, or peer implementations were consulted. `bark/prefill.rs`,
`bark/completion.rs`, and `tests/shell_smoke.rs` were excluded. Immediate Maerg
helpers were read only to establish calls described below. No agents, builds,
tests, or runtime checks were run. Test assertions below are **unexecuted
assertions**, not results. This record neither certifies compliance nor performs
a security audit.

All paths below are repository-relative.

## 1. Checkpoint subprocesses, publication, and recovery

### Source observations

- Checkpoint inventory owns only component-local `src/`, `tests/`, `TODOS.yaml`,
  `IMPL.md`, and `.kvist/continuous-reviews/`. Snapshots bind the component,
  SHA-256 content digest, executable/nonexecutable Git mode, and byte length.
  Validation rejects malformed/escaping paths, conflicting file/directory
  records, invalid modes/digests, and oversized inventories. Directory traversal
  uses open directory handles, no-follow opens, and `/proc/self/fd`; files must
  be single-link regular files. Reads compare length and modification/change
  timestamps before and after.
  (`maerg/src/task_commands/checkpoint.rs:26-200,230-367`;
  shared bounds: `maerg/src/task_commands.rs:3699-3702,4263-4297`.)
- Automatic preflight requires an attached branch, initial commit, usable index,
  configured author/committer identities, and supported signing configuration.
  It compares the owned worktree snapshot and staged entries with HEAD and
  rechecks HEAD/branch. Preparation hashes raw bytes with `--no-filters`, builds
  a tree in an isolated index, and creates a commit with the expected parent,
  signing when enabled and appending the coauthor trailer. It rechecks owned
  content, staging, and HEAD before returning a serializable `PreparedCommit`;
  preparation does not itself advance the ref.
  (`maerg/src/task_commands/checkpoint.rs:379-428,482-575,619-722`.)
- Git runs without a shell, with hooks disabled, fsmonitor/split-index disabled,
  selected Git environment overrides removed, and an explicit project working
  directory. Each supervised command gets a five-minute deadline; nested
  commands on the same thread inherit the earlier deadline and cancellation
  token through a scoped thread-local control. Sequential top-level commands
  get separate deadlines. Interrupt checks cancel that token.
  (`maerg/src/task_commands/checkpoint/process.rs:27-159`.)
- Each child starts a separate process group with piped, nonblocking stdio.
  Writes, reply reads, exit observation, and pipe polling check cancellation and
  deadline; idle polls are at most 25 ms. Stdout has a caller-selected ceiling
  no greater than 16 MiB; stderr is capped at 8192 bytes. Exit is observed with
  `waitid(...WNOWAIT)` without reaping the leader. Cleanup closes pipes, verifies
  direct-child ownership, sends SIGKILL to that pinned group, and waits for the
  direct child. Supervision combines primary and cleanup failures; Drop retries
  cleanup if a child remains.
  (`maerg/src/task_commands/checkpoint/process.rs:174-447`.)
- Ref publication sends `start`, conditional `update HEAD <new> <expected>`,
  and `prepare`; it requires exact phase replies, verifies HEAD and the branch
  through nested Git calls, then sends `commit` and waits for confirmation/exit.
  The nested checks run while the supervising transaction and its inherited
  deadline remain active.
  (`maerg/src/task_commands/checkpoint/process.rs:456-504`.)
- `finish` revalidates the prepared structure, branch, expected-or-already-new
  HEAD, exact single parent, committed snapshot, and exact changed-path map.
  It acquires `index.lock`, copies the current real index, permits owned staging
  to equal the baseline (or the post-state when HEAD already advanced), and
  changes only owned entries in that copy. Thus unrelated staging is carried
  forward rather than reconstructed from HEAD. It syncs the lock file and
  rechecks content, branch, index identity/timestamps, and lock ownership before
  ref advancement; afterward it rechecks the new HEAD and renames the lock over
  the real index.
  (`maerg/src/task_commands/checkpoint.rs:724-763,883-997`.)
- An owner sidecar records commit, PID, device, and inode. Recovery removes a
  matching lock only when the recorded PID is demonstrably absent and the lock
  still matches the recorded identity; missing locks are also handled.
  Unknown/live/mismatched ownership is refused. Drop removes the uninstalled
  lock and sidecar only when their identities still match its open handles.
  (`maerg/src/task_commands/checkpoint.rs:766-881`.)

### Unexecuted assertions

`maerg/src/task_commands/checkpoint/tests.rs:72-408` asserts SHA-256 deletion
handling; consecutive commits preserving unrelated staged/unstaged/untracked
files; executable modes and literal filenames; refusal of dirty overlap and
HEAD/branch/staging drift; repeated recovery after ref advancement; malformed
prepared-state rejection; inventory bounds; filter/hook suppression; link
rejection; owned-dead versus unknown/live index locks; linked worktrees; and
signing failures preserving HEAD.

`maerg/src/task_commands/checkpoint/tests.rs:410-864` defines subprocess fixtures
and asserts deadline handling for quiet output, full stdin, and closed pipes;
cancellation and output overflow; no spawn when precancelled; direct-child
reaping; termination of group helpers while an unrelated group survives;
interrupt cancellation; blocked transaction replies and nested-call limits;
retry after an advanced ref; and signer timeout/retry preserving HEAD/index.
The helper check accepts either absent or zombie descendants, not proof that
every descendant has been reaped.

### Limits

Five minutes is a supervised-command/transaction deadline, **not** a whole
checkpoint deadline. Synchronous filesystem work, spawn, and cleanup's blocking
`Child::wait` are not independently deadline-bound. Cleanup addresses the owned
process group, not descendants that leave it. Ref publication and index rename
are separate operations: cancellation/failure can leave an advanced ref with an
old index, requiring the retained prepared record. The code reports potentially
unknown ref outcomes; it does not roll them back. Git's internal transaction
locking and crash durability were not independently examined.

## 2. Independent model review and configuration

### Source observations

- Every `request_single` constructs a new two-message request: its fixed
  phase-specific system frame and the supplied evidence as user data. It does
  not append authoring history or the selected model's optional system prompt.
  Tools are empty and tool choice is None. A typed native selection is used
  when present; otherwise the code derives a direct gateway transport from the
  selected command rather than executing that command. Unexpected returned
  tool calls are rejected without execution. JSON must contain exactly a
  nonblank report and a boolean decision flag; local report limits are checked.
  (`maerg/src/agent/review.rs:36-52,350-512`;
  selection: `maerg/src/agent.rs:142-230`.)
- Calls in one session share wall time, token accounting, cancellation, and
  diagnostics, but not a conversation. Context planning uses the configured
  context limit or provisional 32768, subtracts response reserve plus 2048,
  and derives a byte budget of three times the remainder, capped at 8 MiB.
  Response reserve is configured or derived from context (default 8192).
  Larger evidence is split at UTF-8 boundaries. Comparison fragmentation repeats
  the shared observed/test frame and fragments the `intent` value. Fragment
  reports and recursive condensations feed fresh synthesis requests; decision
  flags are OR-preserved, and nonshrinking synthesis fails instead of silently
  discarding reports. A configured context also gets an estimated-request check
  before transmission.
  (`maerg/src/agent/review.rs:200-348,394-438,540-559`.)
- The model path passes the shared cancellation token to transport and retries.
  Remaining wall time bounds transport construction; retry loops and backoff
  check the same budget/token. Token reservations include estimated input and
  bounded output; failed/unknown-usage attempts retain their conservative
  charges, while positive provider usage replaces a reservation.
  (`maerg/src/agent/review.rs:439-489`;
  `maerg/src/agent.rs:413-485,554-593`;
  `maerg/src/agent/accounting.rs:24-85`;
  `maerg/src/agent/retry.rs:25-142`.)
- A 25-ms interrupt watcher cancels the session token. Optional controls accept
  cancellation but reject guidance; the recorded guidance event omits its
  supplied text, and failure to record control cancels the review. Finalization
  drops the control server/watcher, records disposition/error, and finishes the
  live journal; unfinished sessions attempt this from Drop.
  (`maerg/src/agent/review.rs:53-100,179-198,515-523`;
  `maerg/src/agent/cancellation.rs:11-59`.)
- Diagnostic redactions include each nonempty configured literal and its
  JSON-string-escaped representation. Log values are redacted before JSON
  serialization and syncing; returned reports are redacted after parsing.
  Streaming stdout and live text use carry-aware, longest-match redaction
  across deltas. The calling review path supplies the union of role redactions
  and allowed sandbox environment values, sanitizes reports again, and records
  observation/comparison reports separately.
  (`maerg/src/agent/review.rs:146-177,484-538`;
  `maerg/src/agent/redaction.rs:10-65`;
  `maerg/src/agent/live.rs:43-105,142-185`;
  `maerg/src/task_commands.rs:5432-5453`;
  `maerg/src/task_commands/continuous.rs:657-704,769-847`.)
- Role resource parsing accepts positive timeout/output limits up to 30 days
  and 16 MiB respectively, and a positive `usize` token limit. Defaults are
  two hours and 16 MiB. Redactions accept one of `redaction_values` or nested
  `redaction.values`, reject empty lists, empty/nonstring/over-4096-byte entries
  and duplicates, and replace the role's list. Shared-registry role bindings
  allow `redaction_values` but not the nested `redaction` spelling.
  (`maerg/src/config.rs:259-262,1558-1578,1987-2083,2152-2172`.)

### Unexecuted assertions

- `maerg/src/agent/review.rs:564-585` asserts raw/JSON-escaped secret removal
  before diagnostic encoding and lossless UTF-8 fragment concatenation.
- `maerg/src/config.rs:2429-2493` asserts role redaction parsing/rejection,
  two-hour/16-MiB defaults, preservation of explicit small caps, and rejection
  of zero/overmaximum output limits.
- `maerg/tests/task_commands.rs:2340-2405` asserts small-context fragmentation
  versus larger-context request counts/sizes. These assertions do not prove
  semantic retention of every finding.
- `maerg/tests/task_commands.rs:2467-2672` asserts escaped-secret removal from
  streamed output/logs/reports, guidance rejection without recording its text,
  prompt cancellation of quiet inference, and stopping later tasks after
  control cancellation or SIGINT. The framing assertions at
  `maerg/tests/task_commands.rs:2022-2050` inspect source-only and comparison
  request contents for selected markers.

### Concrete limitations and local mismatch

- Path announcements bypass the configured value redactor: the control socket
  and live-journal paths are printed with `escape_debug`, not literal
  redaction (`maerg/src/agent/review.rs:90-94`;
  `maerg/src/agent/live.rs:32-35`). A configured ordinary literal occurring in
  those paths can therefore still appear on stderr. The secret-content fixture
  above does not test secret-bearing paths.
- The parser's “nonblank strings” diagnostic does not match its predicate:
  whitespace-only strings pass because it checks `!value.is_empty()`, not
  trimmed emptiness (`maerg/src/config.rs:2051-2063`). This is a bounded local
  validation/message mismatch, not an intent-compliance conclusion.
- Redaction is literal matching plus one JSON-escaped representation, not
  arbitrary encoding recognition. Context accounting is estimated, not a
  tokenizer guarantee; absent configured context, the final context check is
  omitted. Processing every fragment and preserving decision flags does not
  establish that model-written summaries preserve every substantive detail.
  `log_bytes` caps the session's encoded diagnostic log, while transport
  response limits are separate; it is not an aggregate cap over every sink.
  Cancellation/timeout internals of the delegated Sav transport/server were
  outside this Maerg-only observation.

## 3. Authenticated continuous ledger initialization and resumption

### Source observations

`run` validates context and approved configuration, loads authenticated approval
and its secret, and holds a task lock. `run_locked` first checks unresolved
attempts, reads the queue, forms item groups when applicable, calculates the
definition digest, and **loads/authenticates the old ledger before deciding
whether to discard it**. Ledger parsing rejects unknown fields, wrong version,
or an incorrect HMAC-SHA256 tag; the tag covers its serialized fields with the
tag field cleared. Load/save use a 192-MiB file limit, a checked real `.kvist`
directory, and atomic replacement plus directory sync.
(`maerg/src/task_commands/continuous.rs:17-98,531-611`;
approval helper: `maerg/src/task_commands.rs:6115-6189`.)

An authenticated ledger is discarded for fresh initialization only when it is
`awaiting-approval`, has no pending commit, its definition digest differs, and
the current queue contains unfinished work. Fresh initialization captures the
current baseline, uses no HEAD in manual mode or clean-checks automatic modes,
and records the current commit policy/approval identity and already-complete
item groups. Otherwise the retained ledger must match component and commit
policy. Definition changes are accepted only in paused state with no pending
commit. Approval-identity changes are refused with a pending commit; otherwise
the identity is refreshed. Nonpaused retained runs reject changed effect
digests; paused runs skip that comparison.
(`maerg/src/task_commands/continuous.rs:86-177`.)

The definition digest includes task-definition fields and component revision/
parent references but not task status/timestamps. Effects cover `src`, `tests`,
`IMPL.md`, and continuous reviews, not the queue. Interrupt boundaries save a
paused phase. Pending commits are finished before scheduling; preparation is
saved before publication, and successful finish refreshes HEAD/baseline/effects,
clean-checks, then clears pending state. Completed queues record
`awaiting-approval`, explicitly not human acceptance. Task outcomes update the
ledger before errors propagate; noncompleted tasks stop later scheduling.
(`maerg/src/task_commands/continuous.rs:178-403,490-529,617-648`.)

### Unexecuted assertions and limits

`maerg/tests/task_commands.rs:1940-2200` asserts two item checkpoints, retained
unrelated staging, repeated terminal invocation without another commit, fresh
new-work initialization after adding tasks/changing and reapproving commit
policy, component/manual commit counts, and dirty-overlap refusal.
`maerg/tests/task_commands.rs:2203-2308,2409-2448` asserts review-decision pause,
checkpoint retry without repeating completed model work, malformed-report
blocking, and authentication rejection of an edited ledger phase.
`maerg/src/task_commands/continuous/tests.rs:3-51` asserts ledger round-trip above
the ordinary source-file limit and rejection with the wrong secret.

The terminal-new-work regression exercises its specific no-pending path, not
all nonterminal/pending combinations. Authentication binds ledger contents; the
observed digest exclusions and paused-state exceptions remain distinct
behaviors, not evidence that all external edits are detected. Crash windows
between task effects and ledger saves, and every filesystem failure ordering,
were not executed or exhaustively established here.

## 4. Explicit paths and Bark Run dispatch

`resolve_component(Some(...))` delegates to `normalize_component_argument`.
Absolute arguments must lexically strip the project prefix; relative arguments
are not resolved from the current shell directory. Empty wrapper input becomes
`.`. The helper normalizes first, strips a matching configured component-root
prefix when present, and normalizes again. Direct path normalization preserves
`.` and removes current-directory components, but rejects empty paths,
absolutes, and any remaining parent/root/prefix components. It performs no
canonicalization or filesystem/symlink containment check.
(`maerg/src/context.rs:92-99,205-255`.)

Notably, an absolute argument inside the project but outside the configured
component root falls back to its project-relative spelling and is interpreted
as component-root-relative; it is not rejected merely for missing that root
prefix (`maerg/src/context.rs:213-235`). This describes lexical mapping, not
proof that the resulting component exists.

Bark's `TaskCommand::Run` arm resolves the shell project, passes its explicit
component argument through that same resolver, then calls
`run_task_or_item_with_controls` with the resolved component and original task
selection/control flags. Run is not focus-injected. The immediate callee runs
exactly the selected existing task when present, or enters continuous execution
when omitted.
(`maerg/src/bark.rs:830,873-898,1398-1421`;
`maerg/src/task_commands.rs:5203-5225`.)

Unexecuted assertions in `maerg/src/context.rs:266-304` cover leading `./`,
hyphen-leading names, prefixed/absolute spellings, and parent-traversal
rejection. Bark's focus assertion preserves an explicitly supplied `.` at
`maerg/src/bark.rs:1858-1893`. No dispatch runtime check was performed; the
separately maintained shell-smoke regression was deliberately not inspected.
