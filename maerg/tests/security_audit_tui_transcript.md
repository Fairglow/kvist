# Security Audit: TUI and Transcript Persistence

**Component:** maerg (Kvist interactive workspace)
**Scope:** Untrusted-input handling (queue- and report-derived text rendered into the transcript without escape-sequence injection or secret leakage), transcript bounds (file splitting at a fixed bound, no unbounded memory, no symlink or traversal in the state directory), and no authority change (the transcript is local uninspected state, not compliance evidence, and the TUI grants no new task or execution authority).
**Method:** Source inspection of `src/bark/tui/storage.rs`, `src/bark/tui/render.rs`, `src/bark/tui/process.rs`, `src/bark/tui/workspace.rs`, `src/bark/tui/workspace/{actions,view,menus,model_menus,bindings,preferences,menu_tests}.rs`, `src/bark/tui/editor.rs`, `src/bark/{state,status,locks,runs,stream,journal,prefill,theme}.rs`, `src/presentation.rs`, `src/agent/redaction.rs`, `src/file_io.rs`, `src/skott_embedded.rs`, and `tests/tui_workspace.rs`, together with their in-crate `#[cfg(test)]` suites.

## 1. Untrusted-Input Handling

### 1.1 Escape-Sequence Injection — Satisfied

**Observation:** Every untrusted text path is neutralized before it reaches the terminal.

- **Storage choke point.** `Workspace::append` (`workspace.rs:467`) is the single persistence entry: it applies `presentation::redact` and then `presentation::sanitize` to every delta before `Transcript::append` writes it to disk. `sanitize` (`presentation.rs:8`) retains `\n` and `\t`, drops `\r`, and rewrites every other control character as printable `\u{....}` text, so the on-disk transcript is control-free canonical Markdown.
- **Render-time neutralization.** `render::markdown` (`render.rs`) renders the stored Markdown with the `blad` renderer and feeds the resulting byte stream through a `vte::Parser` whose `Perform` implementation maps only a fixed set of SGR codes (CSI `m`) to ratatui styles and ignores every other escape sequence. Any residual control byte that reached the transcript is therefore converted to a styled span, never re-emitted. The `authored_controls_do_not_become_terminal_actions` test pins that an embedded `ESC]52;c;secretBEL` OSC sequence produces no raw escape byte in any rendered span.
- **Queue-derived text.** Task IDs, titles, blocked reasons, and component paths rendered into the transcript by the `tasks`, `last`, `locks`, `history`, and `journal` builtins (`actions.rs`) are escaped with `presentation::literal` (backslash-escaping Markdown specials and converting newlines to `<br>`) or `presentation::fence` (collision-free backtick delimiter wrapping `sanitize`), so queue text cannot break out of a table cell, heading, or fence.
- **Report-derived text.** Child stdout/stderr is decoded by `Stream::push`/`flush` (`process.rs`), which collapses carriage-return progress rewrites and routes each record or line through `presentation::report_redacted` (JSON reports are decoded and re-emitted as Markdown; non-JSON lines are `sanitize`d). The result is then appended through the `append` choke point, so report text is sanitized a second time before persistence.
- **On-screen chrome.** The status heading, stats bar, input line, completion candidates, and menu labels in `view.rs` are all passed through `sanitize` before being rendered as `Paragraph`/`List` widgets. The completion candidate values come from the clap-derived command tree and the `DynamicState` snapshot (`state.rs`), and task titles are truncated and escaped via `literal`.

**Assessment:** Strong. No path allows raw untrusted text to reach the terminal. The `sanitize` primitive is the primary defense against terminal escape injection, `literal`/`fence` are the primary defenses against Markdown structure injection, and the `vte` parser is a second line of defense at render time.

### 1.2 Secret Leakage — Satisfied

**Observation:** Redaction is applied to every text path that persists into the transcript.

- **Redaction values.** `Workspace::refresh` (`workspace.rs`) collects the configured role redaction values (`config.agent.roles.*.redaction_values`, `developer`, `architect`, `security_reviewer`) and the sandbox `allowed_environment` values, sorts and de-duplicates them, and stores them in `self.redactions`.
- **Persistence path.** `Workspace::append` applies `presentation::redact` (a `StreamRedactor` longest-match pass) before `sanitize`, so any configured secret echoed in a report, queue text, or error is replaced with `[REDACTED]` before it is written to the transcript. The child-output `Stream` (`process.rs`) is constructed with the same redaction values and applies `StreamRedactor::absorb` to every decoded record and line before it reaches `append`.
- **Command history.** `submit` (`actions.rs`) redacts the submitted line with `presentation::redact` before pushing it to `self.history` and persisting it via `Transcript::save_history`, so recalled commands do not retain secrets. The history is bounded to 500 entries and a 1 MiB serialized cap, and `history()` on load rejects any entry longer than 8192 bytes.
- **Interactive answers.** While a command is running, the input line is the answer area. The `mask_answers` setting (Appearance menu) displays the answer as asterisks in the footer; the answer is not retained in the transcript (only a fixed "answer not retained" note is appended). The editor's undo history is cleared on submission, so a recalled answer cannot be resurrected through Ctrl+Z (`editor.rs` test `clearing_a_submission_cannot_recall_an_answer_through_undo`).
- **Queue-derived text.** Task IDs, titles, and blocked reasons are not redacted, consistent with the threat model: the queue is local state readable by the same user who owns the transcript, and the status Markdown output (audited separately) makes the same choice.

**Assessment:** Satisfied for the intended threat model. Every text path that persists into the transcript applies the configured redaction values before `sanitize`. The `mask_answers` setting and the cleared undo history prevent interactive answers from leaking into the on-screen chrome or the editor buffer.

## 2. Transcript Bounds

### 2.1 File Splitting at a Fixed Bound — Satisfied

**Observation:** `Transcript::append` (`storage.rs`) enforces a 1 MiB segment bound and a 16 MiB document bound.

- When `self.bytes + markdown.len() <= SEGMENT_BYTES` (1 MiB), the delta is appended directly. Otherwise the current segment is read (bounded to 16 MiB), the delta is merged, and `block_boundary` rotates at top-level Markdown block boundaries (a completed blank line after a depth-0 block end), writing the new pages first and then atomically replacing the old page. An interrupted rotation can duplicate diagnostics but never silently loses previously saved text.
- A single indivisible block exceeding 16 MiB produces an explicit `FileTooLarge` error and cancellation rather than truncation or broken Markdown. The `oversized_indivisible_blocks_fail_without_changing_persisted_history` test pins that the persisted history is unchanged after the failure.
- The `rotation_never_bisects_fences_tables_or_lists_even_across_appends` test pins that rotation never splits a fence, table, or list, and that the concatenated segments reconstruct the source exactly.

**Assessment:** Strong. The segment bound is fixed at 1 MiB, the document bound is fixed at 16 MiB, and rotation is block-aligned. Oversized blocks fail explicitly rather than truncating.

### 2.2 No Unbounded Memory — Satisfied

**Observation:** Every in-memory buffer is bounded independently of total history size.

- The child-output `Stream` buffer (`process.rs`) caps at 16 MiB per command and errors when a single record exceeds the bound.
- The current segment (`Transcript.bytes`) is always under 16 MiB; `append` reads at most 16 MiB when rotating.
- The `SkottEventLog` (`skott_embedded.rs`) retains at most 2000 lines, dropping older lines.
- The editor input (`editor.rs`) is capped at 8192 bytes.
- The viewport (`view.rs`) loads one bounded segment at a time; older segments are reachable by navigation without loading the entire history. The `journal` builtin reports segment count and current bytes without loading all segments.

**Assessment:** Strong. No buffer grows with total history size. The worst-case transient memory during a rotation is the current segment (≤16 MiB) plus the merged delta (≤16 MiB), which is bounded and acceptable.

### 2.3 No Symlink or Traversal in the State Directory — Satisfied

**Observation:** The transcript state directory is protected against symlink and traversal attacks.

- `private_directory` (`storage.rs`) walks the path components, rejecting `..` (parent traversal) and symlinked components, and creates missing directories at mode 0o700.
- `open_private` opens with `O_NOFOLLOW | O_NONBLOCK` and verifies the descriptor is a regular file with one hard link and mode 0o600. A symlinked segment is refused, and the target is untouched (pinned by `transcript_rejects_links_and_competing_writers`).
- Segment names must be exactly 20 ASCII digits plus `.md`; all reads are bounded to 16 MiB and validated as UTF-8.
- The `.writer` lock file uses `try_lock` so a second workspace is refused, and `append` re-checks the file length to detect out-of-band changes. The `dropping_the_writer_releases_a_briefly_inherited_descriptor_lock` test pins that a forked descriptor does not wedge the lock.

**Assessment:** Strong. The state directory is private, non-following, and non-blocking. A symlinked segment or a competing writer is refused without touching the target.

## 3. No Authority Change

### 3.1 Transcript Is Local Uninspected State — Satisfied

**Observation:** The transcript is documented as local diagnostic state, not compliance evidence.

- `REQ-TRANSCRIPT-PERSISTENCE` states "The transcript is local uninspected state and MUST NOT be treated as compliance evidence."
- `CONTRACT.md` states "History is local diagnostic state, not acceptance or compliance evidence."
- The `journal` builtin's text says "Source Markdown is local diagnostic history, not workflow evidence."
- The `bindings::help()` text says "Transcript history is local diagnostic state, not canonical evidence."

**Assessment:** Satisfied. The transcript is consistently presented as local uninspected state across the requirements, contract, and in-app help.

### 3.2 TUI Grants No New Task or Execution Authority — Satisfied

**Observation:** The TUI dispatches commands through the same CLI surface and the same destructive-action gates as the standalone CLI.

- `Workspace::dispatch` (`actions.rs`) parses the submitted line with `cli::Cli::try_parse_from` and `cli::normalize_task_positionals`, the same parser the standalone CLI uses. The same `bark::destructive_gate` confirmation overlay applies to destructive commands.
- `prompt --allow-host-execution` is explicitly refused in the embedded transcript: "host execution requires the standalone interactive `kvist prompt --allow-host-execution` acknowledgement; the embedded transcript is sandbox-only."
- Child commands use argument-vector dispatch with isolated stdout/stderr and the existing cancellation/authority boundaries (`process.rs`). The embedded Skott session uses the existing `SandboxExecutor` and `ToolPolicy` (`skott_embedded.rs`).
- The TUI writes only to the private transcript directory and the theme preference file, both validated to stay inside `.kvist`. The `Preferences` struct (`preferences.rs`) rejects component shortcuts that contain parent traversal or control characters.

**Assessment:** Satisfied. The TUI grants no new task or execution authority beyond what the CLI already provides. The transcript is a local diagnostic record, not a compliance artifact.

## 4. Findings

### 4.1 No Escape-Sequence Injection (Satisfied)

All untrusted text paths are neutralized before they reach the terminal. The `sanitize` primitive is the primary defense against terminal escape injection, `literal`/`fence` are the primary defenses against Markdown structure injection, and the `vte` parser is a second line of defense at render time. No path allows raw untrusted text to reach the terminal.

### 4.2 No Secret Leakage (Satisfied)

Every text path that persists into the transcript applies the configured redaction values before `sanitize`. The `mask_answers` setting and the cleared undo history prevent interactive answers from leaking into the on-screen chrome or the editor buffer. Queue-derived text is not redacted, consistent with the threat model.

### 4.3 Transcript Bounds (Satisfied)

The segment bound is fixed at 1 MiB, the document bound is fixed at 16 MiB, and rotation is block-aligned. Oversized blocks fail explicitly rather than truncating. Every in-memory buffer is bounded independently of total history size. The state directory is private, non-following, and non-blocking.

### 4.4 No Authority Change (Satisfied)

The transcript is consistently presented as local uninspected state across the requirements, contract, and in-app help. The TUI dispatches commands through the same CLI surface and the same destructive-action gates as the standalone CLI. `prompt --allow-host-execution` is explicitly refused in the embedded transcript. The TUI grants no new task or execution authority.

## 5. Required Fixes

None. All controls are satisfied.

## 6. Blockers

None. The TUI and transcript introduce no disclosure, corruption, or authority change. The independent gate is the compliance-review task.
