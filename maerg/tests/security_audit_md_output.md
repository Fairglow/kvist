# Security Audit: Markdown Output Layer

**Component:** maerg (Kvist shell)
**Scope:** Markdown presentation layer for command results, status, and error output
**Method:** Source inspection of `src/presentation.rs`, `src/status_markdown.rs`, `src/status.rs`, `src/cli.rs`, `src/error.rs`, `src/file_io.rs`, and `src/task_queue.rs`

## 1. Untrusted-Input Handling

### 1.1 Escape-Sequence Injection

**Observation:** The Markdown output layer provides three composable escaping primitives:

- `sanitize()` (`presentation.rs:6`) retains `\n` and `\t`, drops `\r`, and represents all other Rust control characters as printable `\u{....}` text. This prevents ANSI escape sequence injection and other terminal control injection.
- `literal()` (`presentation.rs:23`) escapes all Markdown-special characters (`\`, `` ` ``, `*`, `_`, `[`, `]`, `<`, `>`, `|`, `#`, `!`) with a backslash and converts newlines to `<br>`. This prevents heading injection, table cell breakage, link injection, emphasis injection, and code fence breakage.
- `fence()` (`presentation.rs:40`) chooses a backtick delimiter longer than any backtick run in the body, preventing code fence escape.

Every untrusted text source in the Markdown output is passed through one or more of these primitives:

| Source | Primitive(s) | Location |
|--------|-------------|----------|
| Project directory path | `literal()` | `status_markdown.rs:38` |
| Artifact paths and statuses | `literal()` | `status_markdown.rs:53,147` |
| Diagnostics (root, discovery, VCS, toolchain) | `literal()` | `status_markdown.rs:63,79,103` |
| VCS summary | `literal()` | `status_markdown.rs:77` |
| VCS artifact tracking | `description()` — fixed `&'static str` from `VcsArtifactState`, no escaping needed | `status_markdown.rs:88` |
| Component paths | `literal()` | `status_markdown.rs:130,324` |
| Task IDs | `literal()` | `status_markdown.rs:235` |
| Blocked reasons | `fence(reason, "text")` → `sanitize()` | `status_markdown.rs:236` |
| Next command text | `fence(&next, "sh")` → `sanitize()` | `status_markdown.rs:240` |
| Stale document paths and revisions | `literal()` | `status_markdown.rs:155,156,157` |
| Git diff commands | `fence()` → `sanitize()` | `status_markdown.rs:181` |
| Task progress unavailable error | `literal()` | `status_markdown.rs:249` |
| Project guidance | `sanitize()` — host-authored `&'static str` from `project_state::guidance()`, no untrusted input | `status_markdown.rs:74` |
| Legacy report titles | `literal()` | `presentation.rs:229` |
| Legacy report bullets | `literal()` | `presentation.rs:268` |
| Legacy report section headings | `literal()` | `presentation.rs:272` |
| Legacy report key labels | `literal()` | `presentation.rs:283` |
| Legacy report values | `sanitize()` | `presentation.rs:284` |
| Legacy report fence contents | `sanitize()` | `presentation.rs:248,260` |
| Legacy report prose lines | `sanitize()` | `presentation.rs:287` |
| Document titles | `literal()` | `presentation.rs:215` |
| Report JSON scalar values | `literal()` | `presentation.rs:109,111` |
| Report JSON object keys | `literal()` via `label()` | `presentation.rs:140,148,152` |
| Report JSON nested report strings | `report()` recursively | `presentation.rs:141` |
| Error messages | `fence()` → `sanitize()` | `error.rs:763` |
| Error document body | `literal()` via `document()` | `error.rs:780` |

**Assessment:** Strong. Every untrusted text path is properly escaped. There is no path where raw untrusted text reaches the Markdown output without escaping. The `literal()` function is the primary defense against Markdown injection, `sanitize()` is the primary defense against terminal escape injection, and `fence()` prevents code fence escape. The three primitives compose correctly: `fence()` calls `sanitize()` internally, and `document()` calls `literal()` on the title and `report()` on the body.

### 1.2 Secret Leakage

**Observation:** The Markdown output layer applies redaction to report JSON via `report_redacted()` (`presentation.rs:66`), which calls `redact()` on every string value in the JSON before rendering. The redaction uses `StreamRedactor` (`agent/redaction.rs`) which does longest-prefix matching at UTF-8 boundaries. The redaction values come from the configured role redaction values.

However, the status Markdown output (`status_markdown.rs`) does not apply redaction to queue-derived text. Blocked reasons, task IDs, and component paths are rendered directly via `fence()` or `literal()` without redaction. This is consistent with the design: the status Markdown is a local, read-only projection of the queue state, and the queue is a local file that the user has access to. The attention brief (`task_commands/attention.rs`) does apply redaction to queue-derived text before persisting it.

**Assessment:** Satisfied for the intended threat model. The status Markdown is a local presentation of local state; the user who can read the queue can read the secrets in it. The attention brief, which is persisted to disk and potentially shared, does apply redaction. The report JSON, which may contain secrets echoed back from model calls, is redacted. There is no path where a secret from a model call leaks into the status Markdown without redaction, because the status Markdown does not render model call output.

### 1.3 Queue Reading Bounds

**Observation:** `read_queue()` (`status_markdown.rs:338`) reads `TODOS.yaml` via `read_text_bounded()` (`file_io.rs:13`) with a 1 MiB inclusive bound. The read uses `O_NOFOLLOW | O_NONBLOCK` and checks `is_file()` before reading. The byte limit is enforced via `take(limit + 1)` and a length check. Invalid UTF-8 is rejected.

**Assessment:** Strong. The queue read is bounded in size, non-following, and non-blocking. A symlink or FIFO cannot cause a block or a read beyond the limit.

## 2. Determinism

**Observation:** All Markdown rendering functions are pure functions of their input:

- `sanitize()`, `literal()`, `fence()`, `report()`, `document()`, `legacy_report()` are pure functions of their string input.
- `render_context()`, `render_details()`, `render_summary()` are pure functions of the `ProjectInspection` and boolean flags.
- `read_queue()` reads from the filesystem, which is a side effect, but it is deterministic for a given filesystem state. The read is bounded and non-following.

There are no timestamps, random values, or other nondeterministic elements in the Markdown rendering. The `ProjectInspection` is a snapshot of the project state, and the rendering is a pure function of that snapshot.

**Assessment:** Strong. Stable Markdown for a given input. Two invocations of `kvist status` against the same project state will produce byte-identical Markdown output.

## 3. No Behavior Change Beyond Presentation

### 3.1 Stable Text and JSON Report Forms

**Observation:** The native text and JSON status formats are unchanged by the Markdown output layer:

- `status --format text` routes to `render_text()` (`status.rs:42`), which uses `escape_text()` for all untrusted values. This function was not modified by the Markdown commits (`b3177b3`, `3382e6f`).
- `status --format json` routes to `render_json()` (`status.rs:370`), which uses `json_string()` for all untrusted values. This function was not modified by the Markdown commits.
- The `--json` global flag produces a native JSON envelope via `execute_raw()` (`cli.rs:900`), which is bypassed by the Markdown wrapper in `execute()` (`cli.rs:870`).
- `completions` produces a native shell script, bypassed by the Markdown wrapper.
- `AuthoringApply` produces native output, bypassed by the Markdown wrapper.

The `execute()` function (`cli.rs:865`) wraps command output in `presentation::document()` only when the output is not native, not empty, and does not already start with `# `. The native bypass is controlled by the `native` flag, which is true for JSON, text status, completions, and authoring apply.

**Assessment:** Strong. The stable text and JSON report forms are byte-identical to before the Markdown changes. The Markdown output is a new presentation layer that does not modify the underlying data or behavior.

### 3.2 No New I/O

**Observation:** The `read_queue()` function in `status_markdown.rs` reads `TODOS.yaml` files via `read_text_bounded()`. This is not new I/O: the existing `render_text()` and `render_overview()` functions also read `TODOS.yaml` files via `guidance_entries_for()` (`status.rs:250`), which calls `fs::read_to_string()`. The Markdown rendering adds another reader of the same file, but the I/O itself is not new.

The `render_context()`, `render_details()`, and `render_summary()` functions do not perform any filesystem I/O beyond `read_queue()`. They operate on the `ProjectInspection` snapshot, which is produced by `project_state::inspect()` before the rendering begins.

**Assessment:** Satisfied. No new I/O paths are introduced. The queue reading is the same I/O that the existing text and overview formats perform.

### 3.3 No New Authority

**Observation:** The Markdown output layer does not grant any new authority. It is a pure presentation layer that reads the same data as the existing formats and renders it differently. It does not modify files, invoke commands, or change task state. The `execute()` function wraps command output in a Markdown document, but the underlying command logic is unchanged. The Markdown wrapper is applied after the command has completed, and it does not alter the command's effects.

**Assessment:** Satisfied. No new authority is granted.

## 4. Findings

### 4.1 No Escape-Sequence Injection (Satisfied)

All untrusted text paths are properly escaped via `literal()`, `sanitize()`, and `fence()`. No path allows raw untrusted text to reach the Markdown output. The three primitives compose correctly and cover all untrusted text sources.

### 4.2 No Terminal Escape Injection (Satisfied)

`sanitize()` strips all terminal control characters, representing them as `\u{....}` escape text. The Markdown output is safe to render in a terminal. No ANSI escape sequence can be injected through the Markdown output layer.

### 4.3 No Code Fence Escape (Satisfied)

`fence()` chooses a backtick delimiter longer than any backtick run in the body. A blocked reason or legacy report containing backtick runs cannot escape the fence.

### 4.4 Redaction Applied to Report JSON (Satisfied)

`report_redacted()` applies `redact()` to every string value in the report JSON before rendering. Secrets echoed back from model calls are redacted before they reach the Markdown output.

### 4.5 Status Markdown Does Not Redact Queue Text (Noted)

The status Markdown output does not apply redaction to queue-derived text (blocked reasons, task IDs, component paths). This is consistent with the design: the status Markdown is a local, read-only projection of local state. The attention brief, which is persisted to disk, does apply redaction. This is not a defect; it is a design choice that aligns with the threat model.

### 4.6 Determinism (Satisfied)

The Markdown output is deterministic for a given input. No timestamps, random values, or other nondeterministic elements. Two invocations against the same project state produce byte-identical output.

### 4.7 Stable Text and JSON Forms (Satisfied)

The native text and JSON status formats are byte-identical to before the Markdown changes. The Markdown output is a new presentation layer that does not modify the underlying data or behavior.

### 4.8 No New I/O or Authority (Satisfied)

No new I/O paths are introduced. The queue reading is the same I/O that the existing text and overview formats perform. No new authority is granted.

## 5. Required Fixes

None. All controls are satisfied. The one noted observation (4.5) is a design choice, not a defect.

## 6. Blockers

None. The Markdown output layer introduces no disclosure, corruption, or nondeterminism.
