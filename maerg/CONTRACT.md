<!-- kvist-contract-version: 1 -->

# Kvist Maerg Contract

## Shared registry configuration (ADR 0015)

`[agent.registry] path = "..."` selects a shared Sav registry relative to the
configuration file. `[agent.roles.ROLE] profile = "NAME"` binds a shared named
profile, with role policy remaining local. No provider/model facts are copied
by setup. Selection is resolved once while loading configuration; exact registry
source bytes contribute to the configuration digest. `resolve_role_profile`
and `resolve_named_profile` expose pinned typed backend data to the execution
host. Each shared role also retains its `resolved_profile` snapshot in the
serialized approval input. External-agent refusal for protected tasks remains mandatory; parsing or
qualification never grants process authority.

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT,
RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as
described in BCP 14 (RFC 2119 and RFC 8174) when, and only when, they appear in
all capitals.

## Boundary and ownership

- **Component ID:** `kvist.maerg`
- **Contract IDs:** `kvist.cli/v1`, `kvist.artifacts/v1`
- **Owner:** Kvist root component
- **Consumers:** human CLI users, scripts, future editor/web integrations, and
  child component workflows

The component owns Kvist project and component artifacts, CLI dispatch,
validation, discovery, task policy and lifecycle, context selection, sandbox
approval, and durable evidence. It does not own model-provider wire protocols,
generic command supervision, or reusable provider profiles.

## Provided interfaces

The command and diagnostic surfaces below expose project workflow without
delegating task approval or effect authority to their clients.

### Live task diagnostics and controls

Native context configuration bypasses discovery. Without it, startup makes one
optional metadata request bounded to one second and one tenth of remaining task
time. Missing capacity, HTTP 404/405/501 and transient availability failures
continue with a stderr/journal warning. Malformed metadata, other persistent
errors, cancellation and recording failures remain errors. Discovery is not
retried and cannot consume the whole approved task budget.

When capacity is unknown, the local working budget is
`max(32768, estimated_initial_request + response_reserve + 8192)`, capped at
1,048,576 tokens. The reserve is explicitly configured or 8192. This is a
provisional compaction budget, not a claim about the server's context window;
the server may reject it explicitly. If estimated initial input, reserve and
8192-token history headroom exceed that ceiling, startup fails explicitly
instead of silently reducing headroom or truncating input.
Known capacity retains the configured
reserve or `min(8192, max(1, context/4))`. Diagnostics identify configured,
provider or provisional budget provenance, estimated initial input, reserve and
remaining wall time. Neither discovery nor fallback changes approved limits.
Explicit command-endpoint profiles retain their existing 8192-token local context
and 1024-token output reserve without a metadata probe (`source: default`).

Protected native authoring has no implicit 64-generation ceiling. Role
`timeout_seconds` is a positive whole-task budget, defaults to 300 and accepts
up to 2,592,000 seconds (30 days). Explicit `token_limit` remains cumulative.
Each sandbox effect is clipped to remaining task time and at most 86,400
seconds; this does not increase runner grants. Changing role budgets changes
approval input and requires normal policy reapproval. Guidance does not reset
these approved budgets.
Delivered guidance renews model-generation attempts. Individual model requests
retain the transport's independent 24-hour ceiling, including explicit endpoint
profiles; shared profiles may impose shorter per-attempt deadlines.

Explicit cancellation is carried into an executing sandbox effect and
terminates/reaps its owned process group; steering alone never cancels that
effect. The sandbox execution API exposes an additional explicit-token entry
point while existing callers retain signal-only behavior. A token already
cancelled refuses to spawn; cancellation never authorizes replay or implies
rollback.

`task run --allow-control` explicitly enables a private ephemeral local control
capability for each selected protected run. Without it the run has no control
socket. The command reports the diagnostic `*.events.jsonl` path and, only when
enabled, the control socket on stderr. These paths do not grant a workspace
executor. Journals use `sav::observation` version-one envelopes and contain
redacted model/broker/effect lifecycle events as they occur. A synchronized
dispatch record precedes every effect, and recording failure aborts later
effects. Existing canonical attempt evidence remains separate.

Configured literal redactions apply to diagnostic metadata and observed text,
including model names and tool identities. Journal run identities are derived
digests, not raw task text; protocol event tags and private filesystem paths
remain administrative identifiers.

`cancel` requests cooperative termination and retains observed/unknown effects.
`guide` queues bounded supplemental human guidance for the next safe boundary;
it never edits intent or changes a grant. Only an explicitly enabled run can
accept controls. Guidance is recorded before it reaches model context, all
pending assistant calls receive results, and the original wall/token/turn
accounting is retained. Completion closes the control capability. There is no
automatic replay/resume. An observer never needs control access.

The native request materializes only selected local artifacts and exact
root/parent contract mounts. `TODOS.yaml` is strictly validated in full, then
represented by its schema version and the selected task's exact `id`, `title`,
`description`, `context`, `purpose`, `expected_outcome`, `kind`, `depends_on`
and `requirements`. Other tasks, component revision metadata and lifecycle
evidence are not inline model context. Other selected artifacts remain exact.
Reads reject links, non-regular files, traversal,
invalid text and size overflow; invalid selected context fails instead of
disappearing. Profile token limits cover the cumulative run, including
conservative reservations for unknown/interrupted attempts. Known provider
usage remains distinct from those estimates. Errors and cancellation retain
known usage and observed or uncertain effects; they do not authorize replay.

Protected model-requested dependency fulfillment currently fails explicitly:
there is no exact request-to-lock binding and no mediated vendor-repack
contract. A ready unrelated registry is not fulfillment. No host package-manager
fallback is allowed. The broader automatic-acquisition target remains unmet,
not certified or silently waived; explicit human `kvist vendor` is separate.

### Command surface

The `kvist` executable provides (the authoritative design is
[`docs/command-set.md`](../docs/command-set.md)):

- _(no arguments)_: inside a project, the human-friendly project overview plus
  a tip pointing at `kvist help`; outside a project, the guided top-level help
- `shell [PROJECT_DIR]`
- `init [PROJECT_DIR]`
- `convert PROJECT_DIR`
- `import REPO_URL [--branch BRANCH] [--component PATH] [DEST_DIR]`
- `reverse-discover PATH`
- `doctor [PROJECT_DIR]`
- `repair [PROJECT_DIR] [--dry-run] [--salvage] [--reset PATH]...
  [--clean-slate] [--component-root PATH] [--backup-dir PATH] [--approve SHA256]
  [--ai PROFILE] [--proposal PATH]`
- `status [PROJECT_DIR] [--format overview|text|json] [--only-documents]
[--only-impls] [--unfinished]`; the default format is the human-friendly
  overview, and `text`/`json` are the stable report forms for scripts
- `tree [PROJECT_DIR]`
- `component new COMPONENT_DIR`
- `component validate [COMPONENT_DIR]`
- `component accept [COMPONENT_DIR] [--commit] [--message TEXT]`
- `component commit ACCEPTANCE_ID`
- `task next [COMPONENT_DIR]`
- `task transition [COMPONENT_DIR] TASK_ID STATUS [--reason TEXT]`
- `task run [COMPONENT_DIR] [TASK_ID] [--stream] [--allow-control]`, where TASK_ID is an exact
  task ID, an item prefix selecting every task of that item, or `all` (every
  uncompleted task of the component, run sequentially, stopping at the first
  failure)
- `task run-all [COMPONENT_DIR]` — run the whole component: iterate the
  component's tasks in dependency order, running each ready task, and stop when
  the component is complete, when a task becomes blocked, or when a task
  reaches an awaiting-decision point. It never auto-resolves a decision or
  unblocks a blocked task; it pauses and reports the exact state and the
  command that resolves it, leaving durable task state for explicit finalize
  or recovery
- `task log [COMPONENT_DIR] TASK_ID`
- `task replay SESSION_JSONL [--max-turns N]`
- `task approve-policy [PROJECT_DIR]`
- `task unlock [COMPONENT_DIR] [--force]`
- `task recover [COMPONENT_DIR] TASK_ID ATTEMPT_ID --disposition execution-did-not-start`
- `task finalize [COMPONENT_DIR] TASK_ID ATTEMPT_ID DISPOSITION [--commit]
[--reason TEXT]` with DISPOSITION `accept` or `block`
- `vendor [PROJECT_DIR] [--vendored-dir PATH]`
- `toolchain [PROJECT_DIR]`
- `prompt` with one explicit prompt source or redirected input, optional
  `--role`, `--model`, and `--reasoning-effort`
- `agent setup [--force]`
- `agent list`
- `agent remove [MODEL_NAME] [--all] [--global]`
- `agent role list`, `agent role set ROLE MODEL_NAME [--effort EFFORT]
[--global]`, and `agent role clear [ROLE] [--all] [--global]`
- `agent check [--global]`
- `completions SHELL`
- `help [TOPIC]` with TOPIC one of `concepts`, `lifecycle`, `task-states`
  (tab-completable); no topic renders the guided tour listing the available
  topics

**Context resolution.** Project-level commands resolve the project root by
walking upward from the current working directory to the nearest directory
containing a regular `kvist.toml` (an explicit `PROJECT_DIR` argument always
wins and is interpreted relative to the current directory). Component-level
commands take an optional `COMPONENT_DIR`: when omitted, the nearest
discovered component containing the current working directory is selected; when
given, it is interpreted relative to the project's component root, and absolute
paths outside the project are rejected. `component new` keeps its required
directory. Failing outside a project names the missing marker and suggests
`kvist init` or an explicit directory.

`task run` without TASK_ID suggests the first ready task of the component and
executes it only after an explicit interactive confirmation (a bare ENTER
accepts; a refusal changes no state). When no task is ready, or when standard
input is not an interactive terminal, it fails with an actionable diagnostic
and changes no durable state. The shell obtains this confirmation before its
working stage; streamed task commands do not run a competing terminal spinner.
Long result lines wrap without discarding error details or command arguments.

Protected inference calls use automatic transient retries:
the selected native profile's `max_attempts` (1-32, default 3), exponential
backoff capped by `retry_max_delay_secs`, and jitter between half and all of
each scheduled delay. Explicit command-endpoint profiles use three attempts
with 2-second base and 30-second cap. All waits and requests consume the same
approved role wall budget; token reservations remain cumulative. Retry notices
are redacted, emitted on stderr without requiring logging configuration, and
recorded in the live journal. Exhaustion names phase, model, endpoint, attempts,
elapsed time and stopping bound. HTTP 429/500/502/503/504/507, transient socket
errors and transport watchdogs are retryable even after provisional stream
progress: a notice and diagnostic event explicitly discard the incomplete
response, whose proposals were never executable. No retry replays effects,
verification, or an entire task.

Execution/verification failures return nonzero, retain the cause in the blocked
task and report the attempt, log and explicit retry command. A failure to
persist that disposition instead reports the unresolved attempt and retains
its fence. Separate attempts retain distinct evidence even when started within
one clock second. `task next` and automatic selection refuse unresolved attempts.
Multi-task selections stop at pending finalization or decisions, rather than
calling an unfinished task completed. Diagnostic EOF or a different next-ready
task is never evidence of completion.

`task finalize COMPONENT TASK ATTEMPT block --reason TEXT` can explicitly
abandon the latest task attempt even while valid intent is stale or another
task is blocked. It preserves evidence and filesystem effects; it does not
accept intent, certify completion/no execution, or roll back changes.
Result-digest drift and changed execution approval do not prevent rejection.
Malformed artifacts, operation locks and explicit recovery fences still
refuse it. A repeated identical block decision can finish interrupted
recording, but an older attempt cannot dispose a newer one. `--commit` is
invalid for a block disposition. Task-log inspection permits stale intent.
For stale intent plus an unresolved attempt, inspect and block the attempt
first; then review `component accept COMPONENT`, and only afterward use
`task transition COMPONENT TASK pending`. Acceptance remains fenced until
the abandonment is durable and never labels blocked task state current.
`finalize ... accept`, ordinary transitions and task execution keep their
existing current-intent requirements.

`task unlock` and authenticated `task recover ... execution-did-not-start`
also permit otherwise valid tracked stale or blocked components. Unlock
removes only a provably stale operation lock; a live owner remains protected,
and unresolved attempts still fence later writers. No-launch recovery retains
its exact authenticated policy, scope and queue checks and restores only the
recorded pre-attempt state. Neither command accepts changed intent. Review
and `component accept` remain necessary before ordinary work on stale intent.

All ten legal edges among the five task states are command-level operations;
the other fifteen edges, including self-transitions and moves out of completed,
are refused without changing queue or evidence. Entering blocked or
awaiting-decision requires a nonblank reason; other targets forbid `--reason`.
Entering in-progress requires the complete dependency chain, including when
resuming awaiting-decision. Leaving a pause clears its reason.
Both finalization dispositions require the latest exact task/attempt with
matching schema/task identity and refuse conflicting dispositions or recovery
decisions. Acceptance writes the completed queue decision before appending
the final event and refuses another task's unresolved attempt or recovery
fence, just as ordinary queue writers do. Repeating the same
`finalize ... accept` without `--commit` finishes missing recording or returns
the existing decision without another event or queue rewrite. Already accepted
work is not recommitted by repeating finalization; use the reported acceptance's
`component commit` recovery.

`component commit` retries or performs the
isolated index commit for an already accepted set (the acceptance id is
reported when `component accept --commit` records the acceptance but cannot
create the Git commit).

**Guidance.** Project-state domain failures end with a `hint:` line naming the
concrete unblocking command. A non-current project state is never shown bare:
`status` (all three formats) lists every non-valid root artifact with its
exact reason, plus the project-root diagnostic and discovery error when
present, and a concrete action per state — `kvist init` for uninitialized,
`repair --reset PATH` for partial artifacts, ordinary `repair` for set lists,
and proposed `repair --salvage`, reset or clean slate for other invalidity.
Unsupported versions offer a supporting binary or approved reset, not migration.
`status` (overview and text)
renders, per component, state, document validity, task progress, the next
ready task, an Action line for every non-current state, and — for stale
components — each changed document with its expected and observed revision
plus a `git diff HEAD -- <path>` review hint; those per-document lines carry
the same left border as the rest of the report, aligned under the changed
document's name. For blocked components, the status output lists
each blocked task with its `blocked_reason` first line in full (soft-wrapped
to the display width, never truncated) and the exact
next command (`task transition <COMPONENT_DIR> <TASK_ID> pending`, after
resolving the recorded blocker, with an incomplete-dependency count when
applicable); this reopens the task but does not execute it. Awaiting-decision tasks are listed with
a pointer to the task-states help topic. The shell's `tasks` table carries the
same per-task reason and next-command details. Every Action line names a concrete
next command or the `kvist help <TOPIC>` that explains a required human
decision. Invalid `task transition` failures report the legal transitions from
the current state plus a pointer to `kvist help task-states`.
Gating success messages chain to the next lifecycle stage (`component accept`
points at `task next`; `task run` points at `task log`; `task approve-policy`
points at `task run`). Agent configuration commands operate on the project
configuration inside a project and on the global user configuration outside one
(as if `--global` were passed).

`repair` leaves valid queues exact and automatically normalizes invalid set
lists and dependency declaration order when strict validation of the candidate
succeeds. Comprehensive salvage planning is enabled by default; `--salvage`
explicitly requests that same default. Salvage proposes reconciliation and removal of
unrecoverable tasks and affected dependants. It preserves valid task identities,
statuses and dependency edges rather than inventing successful verification.
Missing timestamp objects/fields are repaired automatically with a backup and
without approval. Task status, authored fields, blockers, dependency edges,
finalization references and component revision/revalidation metadata are
preserved. Existing dates remain exact. Missing creation uses the earlier of
the epoch and available date bounds; missing update uses creation as a synthetic
lower bound. Missing completion stays null, even for an already-completed task.
`timestamps.recovered: true` identifies this unavailable history. Native
validation allows completed tasks with null completion only with that flag;
supplied dates retain ordinary calendar and ordering validation. This optional
boolean defaults to false and is omitted when false. True permanently
identifies unavailable original timestamp history; synthetic values are not
observed dates. Malformed supplied history still proposes an approved blocked
recovery record rather than silently replacing data. Future explicit
task transitions record their real update/completion times without clearing this
provenance. Original claimed status and the unknown-history explanation are shown
in any destructive proposal and blocker; originals remain in the backup.
`--reset PATH` (repeatable, project-relative) proposes replacement of a named
root/component artifact with its native draft template; `--clean-slate` proposes
all root/component templates plus archiving project-owned `.kvist` and
`.kvist-attempts` workflow directories. Source and VCS files are never reset.
Unknown versions require explicit reset, not migration. Reset queue revisions
are deliberately unaccepted. Existing execution effects remain unknown.

For invalid Markdown, default salvage proposes native markers/headings and
canonical required sections while preserving prose and unknown sections.
Missing/empty sections and reset drafts receive visible recovery-draft prose and
`<!-- kvist-recovery-blocker: REASON -->` on its own line outside code fences.
These documents pass structural validation but report blocked readiness.
Acceptance and task execution refuse unresolved local intent/record blockers,
immediate-parent contract blockers and global project-intent blockers. Removing
all active markers after resolving their content removes this specific gate;
ordinary revision acceptance is still required. Marker examples inside fenced
code are inert. Subordinate headings remain inside their required section, and
fenced example headings cannot satisfy required sections. Known unsupported
versions are not relabeled. Fenced text is retained during structural salvage.

Salvage/reset/clean-slate are proposals until `--approve SHA256` matches the
displayed plan token. The token binds the canonical project location, exact
original object digests, proposed replacements, reasons and archive scope.
No destructive proposed operation is applied without approval; `--dry-run` never writes,
even with approval. Rerun the same options with the token to apply. Input changes
invalidate it. Resetting a queue with retained execution evidence requires
clean slate, not an ordinary reset that would orphan that evidence.

Every application creates a private `.kvist-recovery/` backup bundle containing
original bytes/objects and a JSON manifest with before/after digests and progress.
The backup path is reported, including on partial failure. Original permissions
are retained in the backup; newly written artifacts are private. Backups are
excluded from component discovery. Recovery does not require configuration or
queue validity; traversal and resource failures are explicit. Live task locks
block writes, and ordinary repair does not bypass unresolved attempts.
The command exits nonzero while any structural artifact problem, discovery
failure or unapplied proposal remains. JSON retains `state_before`, `state_after`,
`queues`, `remaining` and adds `plan`, `approval`, and `backup`. Structural
validity does not assert readiness, acceptance, environmental health or compliance.
`blockers` reports structurally valid recovery drafts separately from `remaining`;
structural recovery can succeed while documents/tasks remain blocked.
Component-validation JSON keeps structural `valid` separate from readiness
`blocked` and `blockers`; status components also expose `recovery_blockers` paths.

`repair --ai PROFILE --proposal PATH` optionally stages a new, private JSON
proposal using a native profile from the shared project/user registry, independent
of `kvist.toml`. PATH must be outside managed artifacts and workflow/VCS storage,
with real existing directory parents; an existing file is never overwritten.
Only deterministic failures or data-discarding salvage qualify. Each inference
contains one target and its local intent bundle, nearest parent contract and root
contract, never peer internals, and has no tools or workspace executor. There are
at most eight targets, 64 KiB per target, 256 KiB per context, 256 KiB per response,
8192 output tokens and a shared 120-second inference deadline. Configured context
capacity is honored; absent capacity uses an explicit provisional 32768-token
bound, not a discovery prerequisite. Invalid/unsupported or oversized inputs and
invalid responses fail without managed writes. AI does not edit execution evidence
or change queue schema.

`repair --proposal PATH` validates and previews that frozen proposal;
`repair --proposal PATH --approve SHA256` applies it without inference, using
ordinary exact-input approval, evidence guards and verified backups. Source,
context or proposal changes invalidate approval. AI Markdown remains blocked;
AI queue tasks are blocked with unknown-history provenance and stale revisions,
never fabricated completion. `--ai` cannot accompany approval, dry-run, reset or
clean slate; `--proposal` cannot accompany reset or clean slate. Staging is the
only explicitly requested unapproved write and never overwrites an existing file.
On failure, deterministic reset/clean slate remain the backed-up fallback.
Inventory respects project `.gitignore` directory exclusions so ignored scratch
worktrees are not implicitly repaired. The ignore policy also binds approval.
Explicitly configured/selected component roots remain inspectable even beneath
an ignored namespace.
`--backup-dir PATH` selects alternative private storage when the default is
unusable; it is included in the approval binding. Within the project, custom
storage must be root-level `.kvist-recovery` or `.kvist-recovery-NAME` so backups
cannot masquerade as components. Intact object quarantine requires same-filesystem
storage; regular-file backups may use other filesystems.
When resetting `kvist.toml` (explicitly or by clean slate), `--component-root PATH`
chooses the replacement component tree without editing TOML. It is a bounded,
non-traversing project-relative path, defaults to `.`, and cannot select VCS,
build, attempt or recovery storage.
External backup storage must be outside every object being quarantined.

`vendor` is a host-authorized provisioning step (ADR-0011, ADR-0013): it
detects the project's language strategy and performs that language's
host-authorized provisioning pass from the exact locked versions, then enforces
readiness, failing closed when the lockfile drifted or locked material is
missing. For a Rust project it provisions the exact `cargo vendor` registry and
writes the versioned vendoring manifest under `.kvist/`. For the other
supported languages it provisions their vendored material under `.kvist/`
(Go's committed `vendor/` directory via `go mod vendor`; the JavaScript package
cache via `npm ci`/`yarn install --frozen-lockfile`; the Python locked wheels
and provisioned virtualenv; the C/C++ project-local Conan home and generated
build files via `conan install --build=missing`).

`toolchain ensure` is a host-authorized provisioning step (ADR-0012): it
resolves the project's pinned Rust toolchain (`rust-toolchain.toml` or
`rust-toolchain` at the project root, else the rustup default), installs the
pinned channel via `rustup` when absent (the supported upgrade/downgrade
path), validates the toolchain layout, and records the versioned toolchain
manifest under `.kvist/`. It is the only supported toolchain change path and
never runs inside a sandbox. Builds and verification never install, upgrade,
or modify a toolchain; they consume the provisioned one and fail closed when
it is absent or has drifted from the recorded manifest.

Commands are non-interactive unless their contract explicitly obtains terminal
input; `prompt` (with a terminal), `shell`, the agent setup/configuration flows,
and `task run` without TASK_ID (which confirms the suggested task) are the
interactive exceptions. `prompt` obtains a terminal to open the standalone
`skott` shell; with no interactive terminal it runs the same shell
headless (`skott --headless --json`), which is sandbox-only and rejects
`--allow-host-execution`.
Success is written to standard output. Domain failures are actionable, written
to standard error, and return a nonzero status. Parser help returns success and
parser input errors use the parser's nonzero status.

`prompt` runs the standalone `skott` shell in every case: on an
interactive terminal the shell is interactive (sandboxed and multi-turn by
default, with an interactive `--allow-host-execution` opt-out); without a
terminal it runs headless, inheriting the child's standard output. Global
`--json` selects the headless NDJSON event stream, which ends with a
run-summary disposition; plain mode inherits the child's answer text.
`--allow-host-execution` is rejected for headless execution. Model selection
is limited to the configured models for the selected role. Reasoning effort
is a typed per-invocation value and is passed through to the shell.

`agent setup` always runs the generated provider command with the fixed prompt
`Reply with exactly: OK` before role configuration is persisted. The explicit
setup invocation first presents the runtime's bounded provider model catalog
when available and places manual entry behind a final custom choice. It
acknowledges current host authority for those discovery commands and the
qualification command only; it does not authorize a later `prompt`.
Kvist does not parse or reinterpret provider catalog descriptors; it receives
the runtime's validated rendered profile and stores that exact selected command
for the requested roles.
Qualification failure returns without persisting the new model. `--force`
still runs qualification but permits a non-cancellation failure to be persisted
after a visible warning; setup never offers an interactive save-after-failure
bypass.
With global `--json`, setup writes its interactive transcript and status to
standard error, suppresses qualification-command output, and writes exactly one
result object to standard output.
A setup invocation that binds an already saved reusable runtime profile does
not generate or execute a new qualification command.

`agent check` live-verifies the model profiles configured in the selected
scope (the project-local `kvist.toml` by default, the user configuration with
`--global`). It lists the profiles and obtains an explicit interactive
acknowledgement before executing any profile command; that acknowledgement is
also a cancellation point. A refusal, cancellation input, exhausted standard
input, or interruption ends the check without executing a test command and
without changing configuration. Each profile is tested on the host with the
runtime's bounded verification and the fixed prompt `Reply with exactly: OK`.
For each failing profile, the check offers removal (the standard profile
removal semantics, including role-binding clearance) or ignore for now
(configuration unchanged), and it remains cancellable at every prompt. With
global `--json`, the transcript goes to standard error and standard output
carries exactly one result object, as in the other interactive agent flows.
`agent role list` presents only the predefined roles (developer, architect,
security-reviewer) and their assigned model profiles, and never presents model
profile names as roles.

`init` writes the complete root artifact set only for an uninitialized project,
is a no-op for a current project, converts an existing Rust package into draft
`.kvist/` metadata, and refuses every other state.

`component new` creates all three intent templates after checking every
destination and never overwrites. `component validate` validates all three
documents. `component accept` validates local intent and the immediate parent
contract, records their exact revisions in the queue, and clears attributable
stale evidence without changing task definitions or status.

The pre-spawn dogfooding recovery surface provides explicit attempt recovery
only when authenticated evidence proves the runner descriptor was not launched
and no write scope was exposed. `task finalize` records an explicit human
disposition for a completed attempt and optionally creates the acceptance
commit. Acceptance operations support an explicit `--commit` option, and
`vcs commit-accepted ACCEPTANCE_ID` retries a commit for an already accepted
set.

`shell [PROJECT_DIR]` starts the interactive workspace shell and requires an
interactive standard input; it fails with an actionable diagnostic otherwise
and is not supported under global `--json`. Lines are parsed against the same
command surface as the CLI. Static completion is derived from that surface (including the help topics as
closed value sets for `help [TOPIC]`) and dynamic completion covers component
paths, task IDs, attempt IDs, model profile names, and the active VCS branch; dynamic sets are refreshed after
every executed command and each source degrades independently without aborting
the session. The shell builtins are `cd [COMPONENT_DIR]`,
`tasks [COMPONENT_DIR] [--status STATUS]`, `run [COMPONENT_DIR] [TASK_ID]`,
`help [TOPIC]`, `last [COUNT]`, `history [COUNT]`, `journal`, `locks
[clean]`, `theme [set <NAME|PATH>]`, and `exit`/`quit`. `help` with no topic lists the builtins and the
available help topics; with a topic it renders that topic, exactly as
`kvist help [TOPIC]` does. The `tasks` table lists each blocked and
awaiting-decision task with its truncated reason first line and the exact next
command (the same run/transition logic as the status report), so no non-current
task is shown without the step that resolves it. `cd` remembers a default component for the builtins and for
completion ordering; when `tasks` or `run` names a component different from the
current focus, the shell prints a hint to switch and leaves the focus
unchanged. `run` without a task ID suggests the first ready task of the
selected component and runs it only after an explicit confirmation (a bare
ENTER accepts; a refusal changes no state). `prompt TASK_ID` opens the external
editor seeded with
the task context and submits only after explicit confirmation. `theme`
previews every available theme (the current one first, then built-ins and
user specs) as a live box drawn in the theme itself, and `theme set
<NAME|PATH>` resolves a built-in name (`dark`, `light`), a registered
user-theme name, or a spec file path, applies the theme to the session
immediately, and persists the preference to `.kvist/theme` with an atomic
write; a preference that cannot be resolved is reported and leaves the
session theme unchanged.

Shell presentation degrades to plain text when `NO_COLOR` is set (any value),
`CLICOLOR=0`, `TERM=dumb`, or stdout is not a terminal; `CLICOLOR_FORCE` (any
value but `0`) forces styling even when stdout is not a terminal. Styled
rendering is driven by a semantic palette of named roles (prompt, component,
failure, indicator, border, dim, the six status hues, and the agent result
surface) resolved once per session: the `dark` theme (default) and the
`light` theme are built in, and a user theme is a TOML spec at
`~/.config/kvist/theme.toml` (or `XDG_CONFIG_HOME/kvist/theme.toml`) with a
`name`, an optional `mode` (`dark` or `light`), and a `[colors]` table
overriding any palette role with a `#rgb`/`#rrggbb` hex, a named ANSI color,
or a 0-255 index. Resolution prefers, in order, the `KVIST_THEME`
environment variable, the project-local `.kvist/theme` preference, the user
preference at `~/.config/kvist/theme`, an OSC 11 terminal-background probe
(light when the probed surface is light), and the `dark` default; an
unresolvable preference reports a warning and falls back to detection. The
`dark` theme renders the agent result surface on a true black background.
Styled rendering keeps table columns aligned on visible width. When stdout is a
terminal with a known width, the shell soft-wraps output at word boundaries so
no line overflows: titled boxes (welcome banner, streaming stages, the
`Prompt` stage) wrap their rows to the box's inner width — at least 40
columns, never wider than the terminal width minus a margin or 100 columns,
whichever is smaller — and the border is drawn on every physical line; bordered
status-report lines (`│…│`) wrap their inner content with the border and
padding re-drawn per line; plain lines wrap with each continuation line
prefixed by the line's leading whitespace so text blocks keep their
indentation, and a `key: value` line wraps with its continuation lines
aligned under the start of the value. Wrapping is a presentation-only
transformation: piped, captured, or width-unknown output is byte-identical to
the unwrapped form. Output is paged
only when it would scroll past the terminal height
minus the prompt row (a 15-line fallback when the height is unknown), and
`KVIST_NO_PAGER` disables paging; paging runs the built-in full-screen pager
with a one-column right-hand scrollbar whose proportional thumb shows the
output's size and the current position (keys: `q`/`Esc` quit, arrows/
`j`/`k` one line, `PageUp`/`PageDown` a page, `g`/`G` the ends, `d`/`u` a
half page, mouse wheel), and a pager that cannot take the terminal falls
back to direct printing. The prompt draws a single full-width separator rule
above the status line, with the most important key hints as dim ghost text
on the input line (whole hint groups dropped from the tail so they never
truncate, and sized to end before the status badge so the badge on the input
line's right edge is never hidden by them). The prompt
reports the last command's exit
state with a failure marker until a command succeeds; Ctrl+L clears the screen
and redraws the prompt. Completion describes dynamic values (task status and
title, a next-ready marker in run contexts, the current component), and a
complete `--` token ends flag parsing and switches to positional completion.

The shell persists an append-only JSONL session journal at
`.kvist/session.log` and a line-based editor history at `.kvist/history`; both
are local uninspected state and MUST NOT be treated as compliance evidence.
Command failures, prompt-editor cancellations, and transient terminal read
failures do not terminate the session; repeated terminal read failures exit
with an actionable diagnostic. SIGINT during a running command requests
cancellation, terminates the supervised process group, and returns to the
prompt with durable task state left for explicit finalize or recovery.
Streaming task execution relays sandbox output to the terminal while it is
produced and retains the full bounded log as evidence. The prompt and status
line report the active VCS branch, the default component, the configured
sandbox backend, the default model, and task-lock counts that distinguish
live locks from stale ones. Destructive operations (`task unlock --force`,
`agent remove --all`, `component accept --commit`, `vcs commit-accepted`)
require an explicit in-shell confirmation.

### Markdown-first output and the interactive TUI

Every command reports its result as a well-formed Markdown document that is the
canonical description of that result. The interactive shell renders that
document with the `blad` renderer inside a full-screen terminal UI in the shape
of the `skott` layout:

- **Status heading.** Shows the active component together with its state and
  task progress, plus the VCS branch, sandbox backend, default model, and
  live/stale lock counts. When no component is selected the heading shows the
  whole project and the shell operates on the project; when a component is
  selected it is auto-selected as the default for every command that expects a
  component, so the operator is never asked for it and it is pre-filled.
- **Main transcript area.** A bounded on-screen window with a scrollbar over a
  durable, per-component, disk-persisted record of every interaction (commands
  and their rendered Markdown output). The record is unlimited in size and
  splits into successive files when one grows too large. The operator can
  scroll back through the session. Switching component clears the transcript
  area and restores that component's former transcript when the operator
  returns to it. The transcript is local uninspected state, not compliance
  evidence.
- **Command footer.** The same command surface as the CLI with full
  tab-completion (static from the clap surface, dynamic from the project
  state) and keyboard-shortcut help rendered the same way `skott` does it.

The TUI degrades to the plain-text line-oriented shell when the terminal cannot
host it, and the plain-text degradation contract of `REQ-INTERACTIVE-SHELL`
still applies. The stable `status --format text` and `--format json` report
forms are unchanged for non-interactive consumers.

`task run-all [COMPONENT_DIR]` runs the whole selected component: it iterates
the component's tasks in dependency order, running each ready task, and stops
when the component is complete, when a task becomes blocked, or when a task
reaches an awaiting-decision point. It never auto-resolves a decision or
unblocks a blocked task; it pauses and reports the exact state and the command
that resolves it, leaving durable task state for explicit finalize or recovery.
It respects the same locking, policy-approval, and evidence guarantees as
`task run`.

## Required interfaces

ADR 0015 retains maerg as the authority host for shared Sav mechanisms.
Provider configuration is resolved through the shared registry; role bindings,
task grants, dependency decisions and canonical evidence remain maerg-owned.
Live diagnostics may be observed independently without starting Skott workspace
tools. Control capability does not authorize changes to intent or grants.
Unsupported external-agent/task combinations fail before execution.

Kvist requires:

- `sav.library/v1` for prompt acquisition, command rendering,
  profiles, setup, process supervision, and model transports;
- `blad.library/v1` for Markdown rendering in the interactive TUI, with the
  renderer's theme, width, and panel background supplied by the host so the
  rendered output matches the TUI's theme;
- a selected Git or Jujutsu repository for durable-artifact tracking checks;
- an independently installed runner implementing
  `kvist-sandbox-probe-v1` and `kvist-sandbox-request-v1` for task execution,
  together with an approval-bound enforcement backend executable configured as
  an absolute `[sandbox] backend` path;
- canonical supported package-source services for explicitly approved
  dependency acquisition;
- operating-system filesystem, process, terminal, and user-state services.

Core inspection does not require any external agent or network interface.

## Data and schemas

All current formats are version 1 and independently versioned:

- `kvist.toml`: strict TOML configuration;
- `VISION.md`: `kvist-vision-version` marker;
- `ARCHITECTURE.md`: `kvist-architecture-version` marker;
- `ROOT_CONTRACT.md`: `kvist-root-contract-version` marker;
- component intent documents: version markers and exact required Markdown
  headings;
- `TODOS.yaml`: strict YAML schema implemented by typed Rust parsing with
  unknown fields rejected;
- `IMPL.md`: version marker and required heading;
- status JSON: `format_version: 1`, with `root_artifacts` (path + status
  pairs in stable artifact order), `root_diagnostic`, and `guidance`
  alongside `project_state`; the stable text report carries the same data as
  `root-artifact:` and `root-diagnostic:` lines plus a `Next Step:` line for
  every non-current project state;
- attempt logs: bounded JSON Lines evidence;
- sandbox request: `protocol_version: 1`;
- vendoring manifest (`.kvist/vendoring-v1.json`): `schema_version: 1` with
  the lock-file digest, vendored directory, and sandbox mount destinations;
- toolchain manifest (`.kvist/rust-toolchain.json`): `schema_version: 1` with
  the pinned channel, toolchain root, cargo path, cargo content digest, and
  provisioning timestamp.

No external schema file is currently normative. JSON Schema export for
machine-consumed formats is permitted later; the contract must then state the
exact dialect and whether the schema or Rust parser is authoritative.

The queue `component` mapping contains:

- `requirements_revision`
- `contract_revision`
- `design_revision`
- `parent_contract`, null at the root or `{ path: "../CONTRACT.md", revision }`
- `revalidation` with state, timestamps, and attributable causes

Every revision is `sha256:` followed by 64 lowercase hexadecimal digits.

## Behavioral guarantees

Discovery is lexical, bounded, and never follows link-like paths. A directory
is a child component when at least one required adjacent artifact exists; all
intermediate component hierarchy rules apply. Artifact reporting order is
`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`.

Status precedence is unsupported version, invalid, missing, stale, blocked,
then current. It computes exact UTF-8 byte digests without persisting derived
staleness. Local mismatch causes are:

- `component-requirements-revision-changed`
- `component-contract-revision-changed`
- `component-design-revision-changed`
- `parent-contract-revision-changed`

Canonical queue serialization preserves task declaration order, stable field
order, two-space indentation, LF endings, sorted set-like lists, and normalized
timestamps.

A task is ready only when pending, current, and all explicit and transitive
predecessors are completed. Completed task IDs are terminal. Queue writes,
policy approvals, attempt records, and test verification follow the atomicity,
locking, and evidence guarantees stated in the requirements.
Any fenced task or unresolved authenticated attempt fences every queue writer
in that component until its exact recovery finishes; read-only inspection
remains available. A completed authenticated recovery chain remains terminal
historical evidence after the recovered queue digest has been verified, so
later legal queue updates do not re-fence it.

`prompt` host execution requires `--allow-host-execution`. It is never
represented as sandboxed. `task run` now emits the redefined version-one
sandbox request: a closed, typed value with an explicit phase, argument vector,
working directory, environment, network capability, resource limits, and
approval-bound grants. Before serialization the maerg resolves `argv[0]` to a
canonical, non-symlink executable (a bare name only against an explicitly
present request `PATH`, with no ambient host fallback) and replaces `argv[0]`
with that canonical path; every grant source is canonical; the request policy
identity is the authenticated execution-approval digest; the toolchain identity
is the content digest of the exact resolved `argv[0]` executable, exposed as a
narrow read-only toolchain grant for that executable (a full immutable
toolchain-set approval is later work); and resource limits use bounded defaults
and options that never exceed fixed safe maxima, failing closed on overflow
rather than saturating. An authoring request grants read-write access to the whole current component
directory minus the excluded paths — the five Kvist intent and record documents
(`REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, `TODOS.yaml`, `IMPL.md`), the
component's `.kvist` state and evidence, its `.git`, and any sub-component
directory — and mounts each excluded document as read-only context at a disjoint
destination. No writable ancestor may expose an excluded document, the
component's `.kvist` or `.git`, or a sub-component directory. The agent may read
the whole current project and approved dependency source within bounded per-file,
per-turn, and directory-depth limits. When an exclusion cannot be correctly
identified, or when no writable path remains, `task run` fails closed rather than
granting the component root. It emits none of the
retired `program`, `arguments`, `mounts`, or `context_files` fields. The
independently installed runner strictly parses and validates this request and
rejects the retired shape, but Bubblewrap enforcement is not yet integrated, so
a valid request fails closed and `task run` still cannot execute Kvist's own
root workspace safely. There is no fallback to the legacy request or host
execution. The maerg builds fallible typed mediated-acquisition and offline-verification
plans with distinct canonical UTF-8 host sources and fixed sandbox
destinations. Acquisition is exactly `cargo fetch` using a real writable
attempt-local `CARGO_HOME`, an isolated writable lockfile workspace, and
separate scratch; it records lockfile before/after content identities for an
immutable-generation promotion result. Verification is exactly
`cargo test --locked` with denied network, `CARGO_NET_OFFLINE=true`, an
approved generation mounted read-only at `CARGO_HOME`, and separate target
scratch. Strict `[sandbox.acquisition]` configuration is approval-bound. The
runner validates these values but live `task run` acquisition wiring, OS mount,
process, DNS/address-pinning, redirect, and network enforcement, and final
project-cache generation selection remain later work.

Language support is declared per language with executable evidence. Rust is
first-class for verification: `kvist task verify` routes Rust projects
(`Cargo.lock` present) to the closed offline Cargo topology and runs exactly
`cargo test --locked` network-denied against the vendored registry. Every
other **vendored** project is routed to the shared offline language topology
(ADR-0013): vendoring is enforced, the language's vendored material is mounted
read-only with the lock-file digest identity, one disjoint writable scratch is
granted at the fixed Cargo scratch destination, the network is denied, and the
language's canonical offline test command runs against the host system
toolchain — Go: `go test -mod=vendor ./...`; JavaScript: `node --test`;
Python: `python3 -m unittest -v` against the mounted provisioned venv;
C/C++: the approved `[test_policy]` command (the build system is
project-defined), with the Conan home mounted read-only at its canonical host
path. Go, JavaScript, and Python are vendored-supported with end-to-end
evidence (`language_offline_e2e`); C/C++ is vendored-supported when its
end-to-end evidence is tracked and passing. Non-vendored projects
use the generic approved-test-command path: the per-component `[test_policy]`
command runs in the network-denied sandbox against host system toolchains
(the runner exposes filtered `/usr`, `/lib`, `/lib64` and `/bin` read-only),
with no vendored mounts and no `$HOME`. JVM and Ruby support zero-dependency
projects only. Container-based builds, toolchains without an offline/locked
mode, toolchains requiring ambient home/global mutable state, and GPU/
accelerator toolchains are explicitly unsupported. Each supported language is
backed by an end-to-end integration test that provisions a small real project
on the host and runs its real build/test offline inside the sandbox.

The Rust toolchain used by offline verification is pinned by the project's
`rust-toolchain.toml` or `rust-toolchain` when present (else the rustup
default), provisioned by `kvist toolchain` on the host, and recorded in
the `.kvist/` toolchain manifest. Verification resolves the toolchain
channel-explicitly, independent of the process working directory, and fails
closed with an actionable message when the pinned toolchain is absent or the
manifest no longer matches the on-disk toolchain. Rust authoring uses the
System/Toolchain grant variant described below, not the closed Cargo cache
or dependency-acquisition topology.

The target supervised tier requires one explicit task and produces a pending
human disposition after agent and verification results are recorded. A
separate finalization action binds acceptance or blocking to the exact attempt
and scoped changes. Automatic selection, retry, and completion are not part of
that tier.

An acceptance with `--commit` creates one local commit from a canonical set of
accepted paths and maerg-written state. It leaves unrelated staged, unstaged,
and untracked paths unchanged and refuses any overlap or concurrent head
change. Commit automation does not push or amend. A commit failure does not
reverse acceptance; it returns the acceptance ID and leaves a retryable
versioned journal for `vcs commit-accepted`.

Dependency acquisition permits Cargo network access only in its distinct phase
and only to exact configured supported sources. It uses a real attempt-local
writable `CARGO_HOME`, an isolated writable lockfile workspace, and separate
target scratch, and never mounts the user's Cargo home. `cargo fetch` may
update that workspace's lockfile. Authoring and verification remain
network-denied; verification uses a selected approved immutable Cargo-home
generation read-only with `cargo test --locked` and offline true.

`agent setup` is the explicit acknowledgement for its single generated
qualification command. Kvist adds no filesystem, credential, executable, or
network restriction to that command and does not represent it as sandboxed;
the selected runtime and provider determine which available host authority
they exercise. The acknowledgement does not extend to subsequent provider
runs.

This section describes the target multi-turn agent tier. The current tier
performs a single, write-only authoring turn; the target tier runs a multi-turn
loop that persists the agent's intermediate reasoning in a per-run trajectory
and persists only the final brokered effect, preserving Kvist's durable,
inspectable state rather than in-chat context. When `task run` executes an
external agent, the maerg performs each model turn on the host, outside the
effect sandbox. The selected model command must target a numeric loopback model
gateway; any other command is refused before any transport work with
`AgentCommandNotModelGateway`, so no agent command ever runs on the host outside
the effect sandbox. The maerg liveness-probes the gateway with a bounded TCP
connect to the resolved endpoint before issuing the model turn. The probe issues
no HTTP request, so it never loads, selects, or shifts a model slot; a gateway
that does not accept a connection fails fast with `LocalModelGatewayUnreachable`
and an actionable "ensure the model server is running and listening on
`{endpoint}" hint.

The loop advertises the closed authoring tool set (`read_file`, `write_file`,
`edit_file`, `request_dependency`, and `propose_decision`). The broker reduces
each turn's untrusted tool intents to capability-bound effects under a
deny-by-default policy; a dropped intent fails that turn without ending the run.
`read_file` is honored within the read scope and logged to the trajectory but
produces no persisted effect. `write_file` and `edit_file` are reduced to the
writable component scope and applied by the maerg itself inside the effect
sandbox against a read-only staged-intent mount; the host never writes component
state for an effect, and only the final brokered effect of a run is persisted.
`request_dependency` is evaluated by the broker's dependency-origin policy rather
than executed inline: a request whose origin is an exact, pinned registry
revision or a public VCS origin with an exact pinned revision (never a private,
link-local, loopback, or unverified production address, and never a wildcard or
unpinned range) is recorded and accepted so the agent continues without
interruption and acquires the revision through the build/verification step; a
request outside that policy is surfaced as a decision. `propose_decision`
records an impactful, uncovered decision for the user as a redacted proposal under
the component state directory and ends the run by placing the component in an
awaiting-decision state; the run harness transitions the task to that state and
never runs verification or jumps to completed. A decision never writes a protected
intent document, and it never blocks on a trivial matter. A run succeeds only when
the agent reports completion, no decision worthy of intervention remains surfaced,
and every authorized effect applied. Once such a decision is accepted, `TODOS.yaml` MUST gain the tasks needed
to implement it and `IMPL.md` MUST become stale; the component then remains in the
awaiting-decision state until an updated advisory review is performed and accepted,
after which `IMPL.md` is rederived from the code.

The model phase runs under one shared wall-clock budget equal to the configured
profile timeout, covering the liveness probe, every model and brokered turn, every read and effect, and every
retry backoff; each per-attempt transport deadline is clipped to the remaining
budget and independent transport/profile ceiling. When
the gateway accepts but a turn still hits a transient availability failure — a
socket connection refused, timed out, interrupted, or reset error, a
slot-allocation timeout, or an overall transport timeout — the maerg retries it
up to three attempts with a short fixed backoff, then surfaces the error if it
never succeeds. Response-level failures (a non-success HTTP status, a malformed
or oversized response, or cancellation) are never retried, because they
indicate a real answer rather than an unavailable gateway. When streaming output
is requested, text deltas are relayed to standard output with the run's
redaction values applied; a streamed attempt that has already emitted text is
never retried. None of this retries, weakens, or changes sandbox authorization,
effect grants, policy approval, or output redaction.

## Errors and failure semantics

Rust authoring uses a bin-only PATH including `/usr/local/bin`.
Standard administrative directories are excluded by Galla in every phase;
language verification does not select tools through sbin aliases.

For components with a local `Cargo.toml`, authoring prepares every validated
standard-layout host Rust installation read-only at `/rust/toolchains/N`.
Generated registrations/settings are mounted at `/rust/rustup-home`; shared
`sav::offline_rust` scripts initialize `RUSTUP_HOME=/tmp/rustup-home` in
invocation scratch. The project pin takes precedence; otherwise the host default
is preserved and `rustup default <installed-name>` changes only that invocation.
Cargo/compiler/doc and companions share selection; PATH Cargo is offline/locked
against the read-only project vendor registry (empty if absent). Local Cargo
metadata is mounted read-only, not exposed as a writable component ancestor.
No user Cargo config/credentials or host settings are mounted. Invalid/unsafe
or over-bound installations fail explicitly. Non-Rust generic authoring retains
the explicitly approved environment and requires no Rust. Verification retains
its separate pinned, vendoring-enforced Cargo topology.

Rust authoring also materializes read-only build context at `/rust/project`,
preserving project-relative workspace paths, root/local Cargo metadata and
declared local dependency source. Workspace members and local path dependencies
must be regular, non-link Cargo roots inside the project. Preparation is bounded
to 128 crates, 100,000 source entries, 32 MiB path state, depth 128 and 30 seconds.
Members may use literal paths or a whole-directory `*` segment; other member
glob syntax fails explicitly. Hidden/operational roots, generated caches and
provider component intent/state are excluded recursively. Providers nested in
writable component implementation/test roots fail preparation rather than
retaining writable host aliases. This is explicitly approved
build-resource context (ADR 0014), not additional prompt context or peer writes.
PATH Cargo selects the authored component's build-view directory. Compiler/
formatter wrappers map its source arguments back to `/workspace/component`,
retaining normal source writes there. Existing writable source/test directories
remain live in the build view so newly authored targets are discovered. Synthetic
ancestor directories are private sandbox backing, never writable host ancestors.
Explicit concrete Cargo paths are not
rewritten. External dependencies still require pre-provisioned vendoring.

Invalid, oversized, unsupported, non-UTF-8, missing, non-regular, or link-like
artifacts fail or are reported according to the read-only command contract.
Kvist does not silently migrate, truncate, repair, overwrite, stage, commit, or
approve user state.

Initialization and multi-artifact creation check conflicts first but are not
multi-file filesystem transactions. A failed operation reports the durable
state left on disk for explicit recovery.

Task execution distinguishes spawn, timeout, output-limit, policy, runner,
agent, verification, gateway-unreachable, and lifecycle failures. A selected
model command that does not target a numeric loopback gateway fails the turn
with `AgentCommandNotModelGateway` before any transport work. An external model
turn to a loopback gateway is liveness-probed first and runs under one shared
wall-clock budget; only its transient availability failures are retried a
bounded number of times within that budget before the gateway-unreachable
failure is surfaced, and response-level failures are never retried. A turn whose
intents were dropped, or whose authorized effects did not all apply, fails
closed with the bounded redacted reason recorded in the log. Failed
verification blocks the task with bounded redacted evidence. Retained locks or
a trailing prepared
record fence further writes until explicit recovery. Target recovery can
reconcile only digest-proven state; uncertain source effects remain fenced for
human disposition.
`task unlock --force` bypasses only the confirmation prompt: it refuses a
demonstrably live, changed, replaced, malformed, or fenced lock state.

Commit recovery operates only on an already accepted canonical set. A changed
accepted path, expected head, signing policy, backend identity, or repository
selection prevents commit creation without altering the accepted files or
acceptance record.

## Security and authority

Repository files, schemas, prompts, configuration, paths, environment values,
external commands, and subprocess output are untrusted. Commands are split and
spawned directly by Kvist without shell expansion. A selected provider remains
an external trust boundary and may implement its own subprocess behavior. Host
prompt execution inherits the user's full authority only after explicit
acknowledgment.

Sandbox approval is bound to canonical project/worktree identity, the exact
bounded `ROOT_CONTRACT.md` digest, runner identity, and the canonical path and
content digest of the approval-bound Bubblewrap enforcement backend, exact
policy bytes, typed grants, supported package sources, command and toolchain
identity, and a user-owned authentication secret outside the repository. The
availability probe must report the approved runner digest and the exact backend
kind, path, and digest, and the backend bytes are rehashed and revalidated
immediately before execution. Runners and enforcement backends inside the
selected worktree or project root are rejected. Every execution phase receives
only its
approved paths, network capability, environment, and bounded resources.

Model transport and credentials are not dependency-acquisition capabilities.
Initial task agents are local. A future remote agent uses host-owned model
transport and credential references plus typed tool requests; mounting ambient
provider state into the effect sandbox is outside this contract.

Git commit creation uses an isolated index and verifies the resulting tree
before an atomic expected-head branch update. Repository-controlled hooks are
not invoked by default. Signing is explicit and a required signature cannot
degrade to unsigned output. Unsupported write-capable VCS backends, including
initial Jujutsu support, fail before changing repository state.

Secrets must not be persisted in project configuration, task queues, prompts,
logs, schemas, or evidence. Configured literal redactions and output bounds
apply before durable attempt evidence is written.

## Compatibility and verification

This pre-release contract intentionally has no compatibility or migration
commitment. Each artifact keeps an independent version marker so compatibility
can be declared precisely before launch.

Contract behavior is verified by CLI, filesystem, schema, VCS, task lifecycle,
process, and Linux sandbox integration tests. `IMPL.md` must be derived
independently from observed code. Compliance requires a separate source-blind
comparison with `REQUIREMENTS.md`, this contract, and `DESIGN.md`.

Workflow/recovery integration fixtures use bounded native artifact templates,
independent of repository-document growth. They retain the same context
limits, execution approval and lifecycle checks; production behavior and
real repository-layout/runner checks are unchanged.
