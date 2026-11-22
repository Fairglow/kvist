# Security Audit: Shell Theming and Scroll Pager

**Component:** maerg (Kvist shell)
**Scope:** Theme resolution, TOML spec parsing, preference files, OSC 11 probe, scroll pager
**Method:** Source inspection of `src/bark/theme.rs`, `src/bark/style.rs`, `src/bark/scroll_pager.rs`, `src/bark/pager.rs`, `src/bark.rs`, `src/bark/tui/workspace/actions.rs`, `src/file_io.rs`

## 1. Untrusted-Input Handling

### 1.1 TOML Theme Spec

**Observation:** `load_theme_file` canonicalizes the path, then reads via `read_text_bounded` with `O_NOFOLLOW | O_NONBLOCK`, an `is_file()` check, and a `take(limit + 1)` byte bound. The parsed spec is validated by `parse_theme_spec`: the name must be 1–32 characters of `[a-z0-9-]`, the mode must be `dark` or `light`, unknown color roles are rejected, and each color value is parsed by `parse_color` (hex `#rgb`/`#rrggbb`, a named ANSI color from a fixed lookup table, or a 0–255 index). The resulting palette is built by `palette_for_spec`, which maps only known roles to SGR codes derived from the validated color values.

**Assessment:** Strong. A user spec cannot inject arbitrary SGR codes or escape sequences into the palette. All color values are validated before they reach the palette. The spec is read-only and bounded.

### 1.2 Preference Files

**Observation:** `read_preference_file` reads via `read_text_bounded` with an 8192-byte cap, `O_NOFOLLOW | O_NONBLOCK`, and an `is_file()` check. It reads only the first line, trims it, and rejects empty content. `match_preference` classifies the value: a path if it contains `/` or ends in `.toml`, `dark`/`light` (case-insensitive), a registered theme name (case-insensitive), or a path (fallback). `save_preference` writes only to `project_dir/.kvist/theme` via `replace_file_atomically`.

**Assessment:** Strong. Preference reads are bounded and non-following. Preference writes stay inside `.kvist`. No path escape.

### 1.3 OSC 11 Response Bytes

**Observation:** `probe_terminal_background` issues the query only when both stdin and stdout are terminals. It enters raw mode via `/dev/tty` (a `RawModeGuard` restores the previous termios on drop), writes `ESC]11;?ESC\`, then reads byte-by-byte with a 150 ms deadline and a 256-byte cap. `parse_osc11` requires valid UTF-8, the `ESC]11;rgb:` marker, a BEL or `ESC\` terminator, and three 1–4-digit hex channels. Non-terminating or malformed input yields `None`, and the dark default applies. The response bytes are parsed, never echoed to the terminal.

**Assessment:** Strong. The probe is bounded in time (150 ms) and size (256 bytes). Malformed input degrades to the dark default. No escape-sequence injection: the response is parsed, not re-emitted.

### 1.4 Terminal Input (Pager)

**Observation:** The scroll pager's event loop matches on `Event::Key` and `Event::Mouse`. Key codes are matched against a fixed set (`q`, `Esc`, arrows, `j`/`k`, `PageUp`/`PageDown`, `g`/`G`, `Home`/`End`, `d`/`u`, `Ctrl+C`). Mouse events are matched against `ScrollUp`/`ScrollDown`. All other events are ignored (`_ => {}`). No terminal input is interpreted as a command, path, or escape sequence.

**Assessment:** Strong. Terminal input is treated as inert bounded data. No escape-sequence injection.

## 2. Determinism

### 2.1 Theme Resolution Order

**Observation:** `Theme::resolve` applies a fixed preference chain: (1) `NO_COLOR`/`CLICOLOR=0`/`TERM=dumb`/non-tty → plain; (2) `KVIST_THEME` env var; (3) project-local `.kvist/theme`; (4) user preference at `~/.config/kvist/theme`; (5) OSC 11 probe; (6) dark default. The chain is deterministic given the same environment and preference files. The OSC 11 probe is the only terminal-dependent element, but it is bounded and degrades to dark.

**Assessment:** Strong. Stable resolution order.

### 2.2 Preview and Hint Rendering

**Observation:** `render_theme_preview` and `render_theme_overview` are pure functions of `(theme, width)`. `registered()` returns themes in registration order (stable within a process). `scrollbar_geometry` is a pure function of `(total, visible, offset)`. `render` is a pure function of `(writer, width, height, lines, offset)`.

**Assessment:** Strong. Stable preview and hint rendering for a given width.

## 3. Boundedness

### 3.1 Spec Size Cap

**Observation:** `MAX_THEME_SPEC_BYTES` was `8 * 192` = 1536 bytes (1.5 KiB), contradicting the documented 8 KiB cap in CONTRACT, REQUIREMENTS, and DESIGN. **Fixed** to `8 * 1024` (8 KiB). A regression test `theme_spec_cap_is_the_documented_eight_kibibytes` now pins the exact value.

**Assessment:** Fixed. The cap now matches the documented 8 KiB.

### 3.2 Probe Timeout and Response Bound

**Observation:** The OSC 11 probe has a 150 ms deadline and a 256-byte response cap. The original implementation checked the deadline only *before* each `read`, but a raw-mode terminal read blocks until a byte arrives — so a non-responding terminal would block the probe indefinitely and the 150 ms deadline was never enforced. **Fixed** by polling the stdin descriptor with `nix::poll` (the same pattern used in `task_commands/checkpoint/process.rs`) with a timeout computed from the remaining deadline, so the probe now actually stops at 150 ms. The 256-byte response cap is enforced in the read loop.

**Assessment:** Fixed. The probe is now bounded in both time (150 ms, enforced by `poll`) and size (256 bytes).

### 3.3 Pager Event Loop

**Observation:** The pager's `read_event` uses `poll(Duration::from_millis(200))` + `read`, bounding each event wait to 200 ms. `max_offset` and `scrollbar_geometry` use saturating arithmetic. Resize events below 10×3 are ignored.

**Assessment:** Strong. Bounded event loop.

## 4. No New Authority

### 4.1 Preference Writes

**Observation:** `save_preference` writes only to `project_dir/.kvist/theme`. The TUI `set_theme` calls `private_directory` (which rejects parent traversal and symlinked directories) before calling `save_preference`. No writes outside `.kvist`.

**Assessment:** Strong. Preference writes stay inside `.kvist`.

### 4.2 Network and Daemon

**Observation:** Neither the theme path nor the pager path makes network calls or spawns daemons. The theme path reads local files and probes the terminal. The pager takes raw-mode and alternate-screen control of the terminal (documented behavior of a full-screen pager) but does not write to files, make network calls, or spawn processes.

**Assessment:** Strong. No new authority.

## 5. Findings

| ID | Severity | Description | Status |
|----|----------|-------------|--------|
| F1 | Medium | `MAX_THEME_SPEC_BYTES` was `8 * 192` (1.5 KiB), contradicting the documented 8 KiB cap. Fixed to `8 * 1024`. Regression test added. | **Fixed** |
| F2 | Medium | The OSC 11 probe's 150 ms deadline was checked only before each blocking `read`, so a non-responding terminal blocked the probe indefinitely. Fixed by polling the stdin descriptor with `nix::poll` (the pattern used in `task_commands/checkpoint/process.rs`) with a timeout computed from the remaining deadline. | **Fixed** |
| F3 | Info | `parse_osc11` uses `rfind('\x1b')` to locate the `ESC\` terminator. Correct in practice because the body is hex digits and slashes (no ESC or backslash). | Observation |
| F4 | Info | The pager's `read_event` blocks up to 200 ms per event. Bounded and standard for a pager. | Observation |
| F5 | Info | The pager reprints the last visible line on exit (like `less`). Documented and intentional. | Observation |

## 6. Conclusion

Both the theme and pager paths are well-constructed. Two concrete defects were found and fixed:

- **F1** — the spec-size cap was `8 * 192` (1.5 KiB), contradicting the documented 8 KiB cap. Fixed to `8 * 1024` and pinned by a regression test.
- **F2** — the OSC 11 probe's 150 ms deadline was not enforced because the blocking `read` was never interrupted. Fixed by polling the stdin descriptor with `nix::poll` so the probe actually stops at 150 ms.

The remaining observations (F3–F5) are documented, bounded, and intentional. No disclosure, injection, or nondeterminism is introduced by the new presentation paths. The controls for untrusted-input handling, determinism, boundedness, and no-new-authority are satisfied.
