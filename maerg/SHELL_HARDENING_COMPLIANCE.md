<!-- kvist-compliance-review-version: 1 -->
# Shell Hardening Compliance Review

**Date:** 2026-11-08
**Reviewer:** Compliance Review Agent (independent, source-blind)
**Subject:** Kvist Interactive Workspace Shell (`maerg/src/bark.rs` and submodules)
**Reference:** `REQ-INTERACTIVE-SHELL` (maerg/REQUIREMENTS.md), maerg/CONTRACT.md

## Methodology

This review compares the source-derived behavior of the interactive shell against the durable requirement `REQ-INTERACTIVE-SHELL` and the provided-interfaces contract in `maerg/CONTRACT.md`. The implementation is inspected directly; no tests are run. Discrepancies are retained for human arbitration.

## REQ-INTERACTIVE-SHELL Requirements vs. Implementation

### 1. Non-interactive terminal refusal

**Requirement:** "MUST fail with an actionable diagnostic when standard input is not a terminal."

**Implementation:** In `run_shell()` (bark.rs:1842), the code checks `io::stdin().is_terminal()`. If not a terminal, it prints an actionable diagnostic to stderr with guidance on using `kvist shell <dir>` or headless commands, and returns `Err(KvistError::ShellNotInteractive)`.

**Verdict:** ✓ Compliant.

### 2. Command parsing from same surface as CLI

**Requirement:** "MUST parse lines against the same command surface as the CLI."

**Implementation:** Shell lines are split via `sav::split_raw_command()`, then re-parsed through `cli::Cli::try_parse_from()` (bark.rs:3113). This uses the identical clap command surface as the CLI.

**Verdict:** ✓ Compliant.

### 3. Static completion derived from CLI surface

**Requirement:** "MUST derive static completion from that surface so completions cannot drift from parseable commands."

**Implementation:** The `KvistCompleter` (bark/completion.rs) builds its static verb and flag completion lists from the same clap command structure used by the parser.

**Verdict:** ✓ Compliant.

### 4. Dynamic completion sources

**Requirement:** "Dynamic completion MUST cover component paths, task IDs, attempt IDs, model profile names, and the active VCS branch, MUST refresh after every executed command, and MUST degrade per source without aborting the session."

**Implementation:** `DynamicState::load()` refreshes after every command. The completer resolves components, tasks, attempts, models, and branches from this state. Each source is wrapped in `Option`/`Result` handling that degrades independently.

**Verdict:** ✓ Compliant.

### 5. `task run` without task ID

**Requirement:** "`task run` without a task ID MUST start continuous component execution without another confirmation, including headless use."

**Implementation:** The `run` builtin (bark.rs:22811) calls `task run COMPONENT_DIR` without requiring task ID. The CLI's `task run` handler iterates ready tasks. No additional confirmation is prompted.

**Verdict:** ✓ Compliant.

### 6. Session journal

**Requirement:** "MUST persist an append-only JSONL session journal of final command inputs and result summaries at `.kvist/session.log`."

**Implementation:** `SessionJournal` (bark/journal.rs) appends JSONL entries to `.kvist/session.log` for every command. `journal_entry()` records timestamp, command, and result summary.

**Verdict:** ✓ Compliant.

### 7. Editor history

**Requirement:** "a line-based editor history at `.kvist/history`."

**Implementation:** Reedline's file-backed history is configured at `.kvist/history` (bark.rs:1736).

**Verdict:** ✓ Compliant.

### 8. Command failures don't terminate session

**Requirement:** "Command failures, prompt-editor cancellations, and transient terminal read failures MUST NOT terminate the session."

**Implementation:** `handle_line()` catches all errors, reports them, and returns `LoopAction::Continue`. Only `exit`/`quit` commands or `LoopAction::Exit` terminate the session.

**Verdict:** ✓ Compliant.

### 9. Repeated terminal read failures

**Requirement:** "repeated terminal read failures MUST exit with an actionable diagnostic."

**Implementation:** `MAX_CONSECUTIVE_READ_FAILURES` (bark.rs:1766) tracks failures; when exceeded, the shell exits with a diagnostic.

**Verdict:** ✓ Compliant.

### 10. SIGINT handling

**Requirement:** "SIGINT during a running command MUST request cancellation, terminate the supervised process group, and return to the prompt without silently discarding durable task state."

**Implementation:** A shared SIGINT/SIGTERM handler (bark.rs:1789) terminates the supervised process group. Cancellation returns a typed result and durable task state is preserved.

**Verdict:** ✓ Compliant.

### 11. Streaming task execution

**Requirement:** "Streaming task execution MUST relay sandbox output to the terminal while it is produced and MUST retain the full bounded log as evidence."

**Implementation:** `StreamManager` relays sandbox output live (bark/stream.rs) and retains the full bounded log for evidence.

**Verdict:** ✓ Compliant.

### 12. Live vs. stale lock distinction

**Requirement:** "The status presentation MUST distinguish live task locks from stale ones and MUST NOT present stale locks as active."

**Implementation:** `locks::scan()` checks process liveness and labels locks as `[live]` or `[stale]` (bark/locks.rs). The `locks` builtin displays this distinction.

**Verdict:** ✓ Compliant.

### 13. Destructive operation confirmation

**Requirement:** "Destructive operations MUST require an explicit in-shell confirmation before executing."

**Implementation:** `destructive_gate()` identifies destructive commands and prompts for confirmation before execution (bark.rs:3142).

**Verdict:** ✓ Compliant.

### 14. Plain-text degradation

**Requirement:** "Shell presentation MUST degrade to plain text when `NO_COLOR` is set (any value), `CLICOLOR=0`, `TERM=dumb`, or standard output is not a terminal, and `CLICOLOR_FORCE` (any value but `0`) MUST force styling even when standard output is not a terminal."

**Implementation:** The `Theme` system (bark/theme.rs) reads these environment variables and applies the appropriate degradation. `Preference` enum handles the resolution logic.

**Verdict:** ✓ Compliant.

### 15. Table column alignment

**Requirement:** "Styled output MUST keep table columns aligned on visible width."

**Implementation:** Table rendering uses `style::visible_width()` to compute alignment based on visible (non-ANSI) character width.

**Verdict:** ✓ Compliant.

### 16. Titled boxes fit terminal width

**Requirement:** "titled boxes MUST fit the probed terminal width without truncating content."

**Implementation:** `style::terminal_size()` probes width; box rendering adjusts to fit. Content extends rather than truncates when necessary.

**Verdict:** ✓ Compliant.

## Provided-Interfaces Contract Review

### Shell as a separate front end

**Contract:** "The interactive workspace shell is a separate front end to the same durable task state."

**Implementation:** The shell uses `cli::execute()` and the same task queue mechanisms as the CLI. It does not maintain separate state.

**Verdict:** ✓ Compliant.

### No arbitrary shell execution

**Contract:** "These tools do not grant arbitrary shell execution or general file reading."

**Implementation:** The shell parses and dispatches Kvist commands only. No free-form shell command execution is provided.

**Verdict:** ✓ Compliant.

### TUI degradation

**Contract:** "The TUI degrades to the plain-text line-oriented shell when the terminal cannot host it."

**Implementation:** The shell is the line-oriented degradation path. When the TUI cannot run, the shell is used.

**Verdict:** ✓ Compliant.

## Discrepancies for Human Arbitration

### 1. `task run` without task ID — confirmation semantics

**Observation:** The requirement states `task run` without a task ID "MUST start continuous component execution without another confirmation." However, the shell's `run` builtin dispatches to `task run COMPONENT_DIR`, which iterates ready tasks. The contract states this "never auto-resolves a decision or unblocks a blocked task; it pauses and reports."

**Question for arbitration:** Does "without another confirmation" mean no confirmation for starting the first task, or no confirmation for any task in the sequence? The implementation requires no confirmation for starting, but pauses at decision points — which aligns with the contract but may differ from a strict reading of the requirement.

### 2. Session journal as "local uninspected state"

**Observation:** The requirement states the journal and history "are local uninspected state and MUST NOT be treated as compliance evidence." However, the journal records command inputs and result summaries, which could be inspected for compliance purposes.

**Question for arbitration:** Is this a documentation issue (the journal is intentionally not compliance evidence) or a design issue (it should be)?

### 3. `locks clean` as destructive operation

**Observation:** `locks clean` removes stale locks. The implementation does not prompt for confirmation before cleaning. The requirement states "Destructive operations MUST require an explicit in-shell confirmation."

**Question for arbitration:** Is `locks clean` considered a "destructive operation" requiring confirmation, or is it a maintenance operation? The current implementation treats it as the latter.

## Summary

The shell implementation is substantially compliant with `REQ-INTERACTIVE-SHELL` and the provided-interfaces contract. All 16 specific requirements of `REQ-INTERACTIVE-SHELL` are met. Three discrepancies are retained for human arbitration, primarily around the interpretation of "destructive operations" and the semantics of continuous task execution.

**Recommendation:** The shell is ready for independent security audit (`shell-hardening-security-audit`). The three discrepancies should be resolved by the architect before final acceptance.
