<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Shell Theming and Scroll Pager (`maerg`)

**Component:** `maerg`
**Task:** `shell-theme-compliance-review`
**Scope:** The interactive-shell theming surface (semantic palette, built-in
themes, the `theme` builtin, `theme set`, the resolution preference chain,
the OSC 11 probe, the user TOML spec, preference persistence) and the
scroll-pager surface (paging policy, the full-screen pager, the
proportional scrollbar, keys and wheel, fallback, and the no-page rule for
piped/captured/width-unknown output), compared against
`REQUIREMENTS.md` (`REQ-SHELL-THEME`, `REQ-SCROLL-PAGER`), `CONTRACT.md`
(`Provided interfaces`, `Behavioral guarantees`), `DESIGN.md`
(presentation/theme and scroll-pager sections), and `ROOT_CONTRACT.md`
(`Change and compliance rules`).
**Reviewer stance:** Source-blind, independent compliance reviewer. This
record is the disposition for the `shell-theme-compliance-review` gate; the
implementer and the security auditor did not author it, and it is not
self-certified.

## Review boundary and independence

This is the final stage of the `shell-theme` chain (tests → implementation →
security audit → compliance review). Per the runbook, the source-blind
compliance reviewer compares the intended artifacts with the implementation
record and bounded test evidence; the clean-slate observation of the surface
belongs to a separate context.

**Read (permitted intent context):** `maerg/REQUIREMENTS.md`,
`maerg/CONTRACT.md`, `maerg/DESIGN.md`, `ROOT_CONTRACT.md`,
`REVIEW_RUNBOOK.md` (the review procedure), and the canonical task-definition
projection in `maerg/TODOS.yaml` for this chain.

**Read (implementation record and bounded test evidence):** `maerg/IMPL.md`
(including the freshly appended "Clean-slate shell-theme and scroll-pager
observations - 2026-11-20" section produced by the separate clean-slate
source-observer context), the in-crate `#[cfg(test)]` suites of
`maerg/src/bark/theme.rs`, `maerg/src/bark/scroll_pager.rs`,
`maerg/src/bark/pager.rs`, `maerg/src/bark/style.rs`,
`maerg/src/bark/status.rs`, and `maerg/src/bark/stream.rs` (assertions only),
and the integration evidence `maerg/tests/tui_workspace.rs` and
`maerg/tests/shell_smoke.rs` (assertions only).

**Not read as evidence:** Git history or diffs, prior compliance
conclusions, peer or parent implementation artifacts. The security-audit
report of this chain is not relied upon and its conclusions are not
repeated.

**Context note (disclosed, not a boundary breach):** the task definition for
this gate explicitly directs that observed behavior be derived from source
and tests in a clean-slate context before the source-blind comparison, and
that derivation was performed in this context. The two passes remain
separately attributed below: every "observed" statement is attributed to the
clean-slate observation record in `IMPL.md` or to the bounded test evidence,
and no compliance clause is certified from a prior compliance conclusion.

## Bounded evidence relied upon

- `IMPL.md` section "Clean-slate shell-theme and scroll-pager observations
  - 2026-11-20": registry, palettes, resolution chain, spec validation and
  8 KiB cap, the 150 ms / 256-byte bounded OSC 11 probe, preference
  persistence, the `theme` builtin and `theme set` behavior, prompt
  separator and ghost key hints, the merged `Agent Working` box, the plain
  theme identity, and the pager policy/geometry/keys.
- `IMPL.md` section "Scoped workspace settings observations - 2026-11-04":
  independent RGB dark/light palettes with 17 semantic roles, raised
  surfaces and selection colors; contrast assertions require 4.5:1 for
  tested text/accent combinations and 3:1 for borders and do not certify
  arbitrary custom themes.
- In-crate assertions: `dark_and_light_palettes_differ_on_the_agent_surface`
  (near-black `48;2;9;13;18` vs clear-white `48;2;255;255;255` agent
  surfaces), `built_in_surfaces_have_measured_text_and_edge_contrast`
  (4.5:1 text/secondary/accent on canvas and raised panel, 3:1 border,
  distinct raised surface, 4.5:1 selection), the `parse_theme_spec`
  validation cases, `match_preference` classification, `parse_color`
  hex/named/index cases, `separator_rule` width, `key_hints`
  fit-and-drop-from-the-tail, `result_stage_*` merged-box rows, the
  `scrollbar_geometry` cases (fit-fills-track, proportionality, two-row
  floor, offset tracking, thumb within the track), the `render`
  per-row/column-0 and last-column cases, `policy_never_pages_without_a_
  terminal_or_with_the_env_override`, and `policy_pages_only_when_the_output
  _would_scroll`.
- Integration evidence: `tests/tui_workspace.rs` (the XDG user theme is
  loaded and rendered, a menu `theme set` persists `light` to
  `.kvist/theme`, the workspace enters and leaves the alternate screen) and
  `tests/shell_smoke.rs` (a known window size and `KVIST_NO_PAGER` keep the
  height-aware pager disengaged while the shell runs).

This record is not execution proof. These suites are run automatically by
Kvist in the verification sandbox after this turn; no pass/fail count from
that run is asserted here.

## Method

Each obligation in `REQ-SHELL-THEME` and `REQ-SCROLL-PAGER`, each
contract clause in `CONTRACT.md` `Provided interfaces` bearing on this
surface (the `Behavioral guarantees` section covers other surfaces; its
relevant guarantee — atomicity of queue writes and evidence — applies to the
preference write and is assessed through it), each relevant design claim in
the theme and scroll-pager sections of `DESIGN.md`, and the root
`Change and compliance rules` were compared against the implementation
record and the bounded test evidence. Each item is classified
**compliant**, **mismatched**, **approved-deferred**, or **underspecified**,
with component path, locator, intended text, observed behavior/evidence,
severity, and owner.

## Satisfied clauses (compliant)

**C-1 — Semantic palette of named roles; no renderer hard-codes a color.**
`REQ-SHELL-THEME` ("render all presentation through a semantic color
palette of named roles … so no renderer hard-codes a color"); `CONTRACT.md`
("Styled rendering is driven by a semantic palette of named roles …
resolved once per session"); `DESIGN.md` (17 SGR roles). Observed: the
`Palette` defines 17 named SGR roles (prompt, component, failure,
indicator, border, dim, the six status hues red/green/yellow/blue/magenta/
cyan, agent foreground/background, raised panel background, selection
foreground/background); every shell renderer styles through role helpers,
and the TUI chrome, panel, selection, and Blad markdown palettes are all
derived from `theme.palette()`. Compliant.

**C-2 — Built-in `dark` (default) and `light` themes.**
`REQ-SHELL-THEME`; `CONTRACT.md`; `DESIGN.md` (dark id 0, light id 1, always
present). Observed: the process registry is initialized with the two
built-ins, dark is the default everywhere the default applies (plain-theme
id, probe fallback, `Theme::enabled`), and user themes register
additionally. Compliant.

**C-3 — `theme` builtin shows a live in-terminal preview of every available
theme.**
`REQ-SHELL-THEME`; `CONTRACT.md` ("previews every available theme (the
current one first, then built-ins and user specs) as a live box drawn in
the theme itself"). Observed: `render_theme_overview` prints the current
theme first and then every registered theme as a titled box drawn in that
theme (prompt line, separator rule, agent-result bar, status badge); the
built-ins/user split follows registry registration order. The completion
menu's `theme set` candidates carry a one-line sample preview (the prompt
verb and indicator, styled by the theme) as their description, not the full
live box — the design's "completion menu shows the same preview" claim is
recorded as F-4. Compliant for the required builtin preview.

**C-4 — `theme set <NAME|PATH>` switches the session immediately and
persists to `.kvist/theme`.**
`REQ-SHELL-THEME`; `CONTRACT.md` ("applies the theme to the session
immediately, and persists the preference to `.kvist/theme` with an atomic
write"); `DESIGN.md` (atomic tmp-file-plus-rename; stream manager's theme
updated in place). Observed: the workspace-shell handler and the TUI menu
route both resolve the value (built-in name, registered name, or spec path),
persist through `replace_file_atomically`, and apply the new palette to the
session and stream manager; a path that cannot be loaded is reported and
leaves the session theme unchanged (no persistence, no switch). Integration
evidence: a menu `theme set` persisted `light` to `.kvist/theme`.
Compliant.

**C-5 — Resolution precedence: `KVIST_THEME`, project-local
`.kvist/theme`, user preference at `~/.config/kvist/theme` (honoring
`XDG_CONFIG_HOME`), OSC 11 probe, dark fallback.**
`REQ-SHELL-THEME`; `CONTRACT.md`; `DESIGN.md` (`Theme::resolve` order).
Observed: the chain reads, in order, the `KVIST_THEME` environment
variable, the project-local `.kvist/theme` preference, and the user
preference at `$XDG_CONFIG_HOME/kvist/theme` (defaulting to
`~/.config/kvist/theme`); a preference that names a spec file loads that
file; with no resolvable preference the OSC 11 probe runs and dark is the
final fallback. Compliant.

**C-6 — An unresolvable preference warns and falls back to detection.**
`REQ-SHELL-THEME`; `CONTRACT.md` ("an unresolvable preference reports a
warning and falls back to detection"). Observed: an unreadable or
unresolvable path logs a `tracing::warn!` and applies `detect_id()` (OSC 11
probe, then dark) instead of failing the session. Compliant.

**C-7 — User spec format: TOML with `name` (1-32 chars of `[a-z0-9-]`),
optional `mode` (`dark`/`light`, default `dark`), and a `[colors]` table
mapping palette roles to `#rgb`/`#rrggbb` hex, a named ANSI color, or a
0-255 index.**
`REQ-SHELL-THEME`; `CONTRACT.md`; `DESIGN.md`. Observed: `parse_theme_spec`
validates exactly this shape; `parse_color` accepts the three value forms.
The one deviation (upper-case names are lower-cased rather than rejected)
is recorded as observation N-1. Compliant subject to N-1.

**C-8 — Spec size cap 8 KiB; unknown roles rejected; invalid/over-size/
unreadable specs fail with an actionable diagnostic without changing the
session.**
`REQ-SHELL-THEME`; `CONTRACT.md`; `DESIGN.md` ("specs are size-bounded
(8 KiB), unknown roles are rejected"). Observed: `MAX_THEME_SPEC_BYTES =
8 * 1024` is enforced by the bounded read (`FileTooLarge` →
`ThemeSpecTooLarge`); unknown roles are rejected by name before any
registration; `ThemeSpecInvalid` carries an actionable reason; the `theme
set` path reports the diagnostic and leaves the session unchanged.
Compliant.

**C-9 — Plain-text degradation applies to every theme.**
`REQ-SHELL-THEME` ("The plain-text degradation contract of
`REQ-INTERACTIVE-SHELL` MUST apply to every theme"); `CONTRACT.md`
("degrades to plain text when `NO_COLOR` is set (any value), `CLICOLOR=0`,
`TERM=dumb`, or stdout is not a terminal; `CLICOLOR_FORCE` (any value but
`0`) forces styling"). Observed: `env_disables_color` implements exactly
that contract, the plain theme makes every style helper the identity, and
resolution enables styling only under the `CLICOLOR_FORCE`/terminal rule.
Compliant.

**C-10 — Built-in palettes: independent explicit RGB, near-black/clear
white canvas, visibly distinct raised and selected surfaces.**
`REQ-SHELL-THEME`; `CONTRACT.md` ("the `dark` theme renders the agent
result surface on a deep near-black RGB background; `light` uses clear
white with independently contrasting accents"); `DESIGN.md` ("Independent
RGB tables provide a near-black dark canvas and clear-white light canvas
with contrasting text, borders, panels and selection"). Observed: both
palettes are independent explicit `38;2`/`48;2` tables; the agent surfaces
are `48;2;9;13;18` (near black) and `48;2;255;255;255` (clear white); the
raised panel background and the selection surface differ from the canvas in
both themes. Compliant.

**C-11 — Contrast: normal and secondary text 4.5:1 against their built-in
surfaces; meaningful UI edges 3:1.**
`REQ-SHELL-THEME`. Observed: the in-crate contrast test asserts 4.5:1 for
the main foreground, the `dim` (secondary) role, the accent, and the six
status hues against both the canvas and the raised panel, 3:1 for the
border, and 4.5:1 for the selection surface, for both built-in themes.
The IMPL record states the limit explicitly: the assertions cover the
tested text/accent combinations and do not certify arbitrary custom themes.
Compliant for the built-in surfaces (the tested-combination limit is
recorded, not a discrepancy).

**C-12 — The same palette reaches Blad and the UI chrome.**
`REQ-SHELL-THEME`. Observed: the TUI host chrome, panel, selection,
secondary, and border styles are derived from the active palette, and the
Blad markdown palette (heading, link, math, code/table/quote backgrounds)
is derived from it; the IMPL record states the canvas colors reach Blad.
Compliant.

**C-13 — Output scrolling past the pager threshold is paged by the
built-in full-screen pager.**
`REQ-SCROLL-PAGER` ("Output that scrolls past the pager threshold MUST be
paged in a built-in full-screen pager"); `CONTRACT.md` ("Output is paged
only when it would scroll past the terminal height minus the prompt row (a
15-line fallback when the height is unknown)"). Observed: `pager_policy`
pages only when `KVIST_NO_PAGER` is unset, stdout is a terminal, and the
line count exceeds `rows − 1` (fallback 15); `display_output` runs
`scroll_pager::page` in crossterm raw mode and the alternate screen.
Compliant.

**C-14 — One-column right-hand scrollbar; thumb sized proportionally to
the output (never shorter than two rows); position reflects the current
scroll offset.**
`REQ-SCROLL-PAGER`; `CONTRACT.md` ("a one-column right-hand scrollbar whose
proportional thumb shows the output's size and the current position");
`DESIGN.md` (`scrollbar_geometry` contract). Observed: `render` draws the
scrollbar in the last column (`█` thumb over a dim `·` track);
`scrollbar_geometry` fills the track when the document fits, sizes the
thumb proportionally with a two-row floor, and positions it
proportionally to the offset. In-crate evidence pins all four properties.
Compliant.

**C-15 — Full terminal height: each content line occupies its own terminal
row at the left edge; gutter and scrollbar reserved on the right.**
`REQ-SCROLL-PAGER`. Observed: the render pass moves the cursor to column 0
of each row and prints that row's line (crossterm `MoveTo` column-first),
reserves a two-column gutter, and addresses every row of the view. In-crate
evidence pins per-row content, full-height rendering, scrolling window
shift, and last-column scrollbar placement. Compliant.

**C-16 — Pager keys: quit (`q`/`Esc`), one-line (arrows, `j`/`k`), page
(`PageUp`/`PageDown`), top/bottom (`g`/`G`), mouse wheel.**
`REQ-SCROLL-PAGER`; `CONTRACT.md` (also `d`/`u` half page). Observed: the
event loop implements all listed keys plus `Q`, `Ctrl+C`, Home/End, `d`/
`u`, and the mouse wheel (three lines per notch). Compliant (superset of
the required set).

**C-17 — Fallback to direct printing when the pager cannot take the
terminal; no output lost.**
`REQ-SCROLL-PAGER`; `CONTRACT.md` ("a pager that cannot take the terminal
falls back to direct printing"). Observed: `page` returns `false` on
unknown or too-small terminal size, raw-mode failure, or alternate-screen
failure, and `display_output` prints the output directly in that case; on
exit the pager reprints the last visible line so the prompt follows the
content. Compliant.

**C-18 — Piped, captured, and width-unknown output is never paged.**
`REQ-SCROLL-PAGER`. Observed: `pager_policy` returns `false` when stdout is
not a terminal or `KVIST_NO_PAGER` is set, so no paging occurs for piped or
captured output; `page` likewise refuses without a probed size. In-crate
evidence pins the no-terminal and env-override cases. Compliant.

**C-19 — The root change-and-compliance process for this surface.**
`ROOT_CONTRACT.md` ("Record intent-to-implementation discrepancies for
explicit arbitration; do not silently alter either artifact"; clean-slate
documenter and separate compliance reviewer). Observed: the chain
(→ tests → implementation → security audit → compliance review) is
reflected in the queue; this record retains its findings for arbitration
without altering `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, or the
implementation; the clean-slate observation of this surface was appended to
`IMPL.md` under a separate observer heading. Compliant, with the retention
gap F-1 retained below (the observation was absent from the record when
this gate was reached).

## Discrepancies (retained for human arbitration)

**F-1 (MEDIUM, underspecified — record/evidence gap) — `IMPL.md` carried no
clean-slate observation of the shell-theme or scroll-pager surface when
this gate was reached.**
- *Locator:* `REVIEW_RUNBOOK.md` ("The documenter writes or proposes a
  fresh `IMPL.md` from observed evidence only. It states public behavior,
  guarantees and limits, state effects, failure paths, and known
  uncertainty."); `ROOT_CONTRACT.md` ("A clean-slate documenter and a
  separate compliance reviewer must verify implemented behavior before it
  is declared compliant"); `IMPL.md` (no dedicated section existed; the
  only mentions were the one-line "independent RGB dark/light palettes with
  17 semantic roles" note in the workspace-settings section and the
  segment-band note describing the TUI *transcript* scrollbar, which is a
  different surface from `REQ-SCROLL-PAGER`).
- *Intended:* the implementation record states the observed behavior,
  guarantees and limits, failure paths, and known uncertainty of the
  surface under review, so the source-blind comparison has a record to
  compare against.
- *Observed:* the record did not observe, among other things, the
  resolution preference chain, the OSC 11 probe and its bounds, the spec
  validation and 8 KiB cap, the `theme` builtin and `theme set` semantics,
  the prompt separator and ghost key hints, the paging policy, the
  scrollbar geometry contract, or the no-page/fallback rule. This review
  therefore rests for those clauses on the bounded test evidence and the
  freshly appended observation rather than on a record that predated this
  gate.
- *Remediation already performed:* a separate clean-slate
  source-observer pass was recorded in this context and appended to
  `IMPL.md` as "Clean-slate shell-theme and scroll-pager observations -
  2026-11-20". It is not self-certified as the documenter's own output;
  the reviewer of record for this gate is source-blind.
- *Why it matters:* the runbook requires the comparison against a record;
  without one, clause-by-clause compliance for this surface cannot be
  attributed to the independent documenter context, and the gate's
  "implementation record" input was effectively missing.
- *Arbitration options:* (a) retain the appended observation as the
  fresh clean-slate record for this surface and close the gate on this
  review; (b) commission a separate clean-slate documenter context to
  produce its own observation and re-run this pass; (c) record a reasoned
  exception that the bounded test evidence and this review's disclosed
  derivation are the retained observation for this chain.

**F-2 (MEDIUM, mismatched/underspecified) — "State MUST remain recognizable
without color alone" has no observed basis in the implementation record.**
- *Locator:* `REQ-SHELL-THEME` (final sentence: "State MUST remain
  recognizable without color alone."); the intent set does not define which
  states are covered or by which non-color mechanisms.
- *Intended:* states (at minimum the states the palette colors — success,
  failure, caution, in-progress, live/stale, next-ready) remain
  distinguishable when color perception is absent.
- *Observed:* the record observes color-based distinction for the status
  hues and only one non-color mechanism: the prompt's literal failure
  marker (`✘`) in `short_prompt`. The record does not establish non-color
  distinction for the status-table colors, the next-ready marker, or the
  live/stale counts, and the requirement does not say which states the
  clause covers. The in-crate contrast test is silent on non-color
  recognition by design (it measures color contrast only).
- *Why it matters:* a "MUST" clause cannot be classified compliant without
  observed basis, but it cannot be classified mismatched either unless the
  intent set is read to demand non-color markers for every colored state —
  which it does not state. The clause is therefore both underspecified
  (scope of "state") and unverified (no observed mechanism beyond the
  prompt marker).
- *Arbitration options:* (a) read the clause as satisfied by the existing
  non-color affordances (the `✘` failure marker, text labels on task
  states, the `[current]`/selection markers) and record that reading; (b)
  narrow the requirement to name the states it covers; or (c) require an
  implementation change that adds non-color distinction to the remaining
  colored states. Do not silently edit the requirement to erase the
  mismatch.

**F-3 (LOW, mismatched — contract coverage gap) — the user preference file
`~/.config/kvist/theme` (name or spec path) is in the requirement but not
explicitly named in the contract's theme clause.**
- *Locator:* `REQUIREMENTS.md` `REQ-SHELL-THEME` ("the user preference at
  `~/.config/kvist/theme` (honoring `XDG_CONFIG_HOME`)" and "a preference
  that names a spec file MUST load that file"); `CONTRACT.md` `Provided
  interfaces` ("…the user preference at `~/.config/kvist/theme`…" is not
  stated; the contract names the TOML spec path
  `~/.config/kvist/theme.toml` (or `XDG_CONFIG_HOME/kvist/theme.toml`) and
  the chain "the `KVIST_THEME` environment variable, the project-local
  `.kvist/theme` preference, the user preference at
  `~/.config/kvist/theme`, an OSC 11 terminal-background probe …" — i.e.
  the chain names the preference *file path* but the contract's user-theme
  definition covers only the `theme.toml` spec file and does not state
  that the user preference file may contain a name or a spec path).
- *Intended:* consumers may rely on both a user *preference* file
  (`~/.config/kvist/theme`, one line: a name or a spec path) and the user
  *spec* file (`theme.toml`), with a preference that names a spec file
  loading that file.
- *Observed:* the implementation record observes both files and the
  name-or-path classification of the preference, so behavior matches the
  requirement; the gap is that the contract's normative text for the user
  theme names only `theme.toml`, leaving the `~/.config/kvist/theme`
  preference file's content grammar unstated in the contract.
- *Why it matters:* consumers of the contract cannot determine from
  `CONTRACT.md` alone that `~/.config/kvist/theme` exists or what it may
  contain, even though the requirement and the implementation define it.
- *Arbitration options:* (a) amend the contract's theme clause to name the
  user preference file and its one-line name-or-path grammar (explicit
  intent-document change); or (b) record that the chain wording is the
  contractual carrier and the requirement is the detailed one.

**F-4 (LOW, mismatched — design-claim gap) — the completion menu does not
show the full theme preview, only a one-line sample.**
- *Locator:* `DESIGN.md` ("The `theme` builtin previews every available
  theme as a live titled box drawn in the theme itself (prompt line,
  separator rule, agent result bar, status badge), and the completion menu
  shows the same preview as the description of `theme set` candidates.");
  `REQ-SHELL-THEME` and `CONTRACT.md` (the required live preview is the
  builtin's, which is satisfied — see C-3).
- *Intended (design claim):* the `theme set` completion candidates display
  the same full live titled box as the builtin preview.
- *Observed:* the candidate description is a one-line sample rendered in
  the theme itself (the styled `kvist` prompt verb and `❯` indicator), not
  the multi-line titled box. The builtin preview satisfies the
  requirement; only the design's claim about the completion menu is not
  matched.
- *Why it matters:* `DESIGN.md` states how the component intends to
  satisfy the requirements; the claim is narrower than "compliant or not"
  for the requirement, but the design and the observed behavior do not
  correspond, and the design is not to be silently rewritten.
- *Arbitration options:* (a) explicitly amend the design to describe the
  one-line sample preview; (b) change the completion descriptions to the
  full live box; or (c) record a reasoned exception that the one-line
  sample is the retained behavior.

**N-1 (INFO, observed behavior — benign deviation, no arbitration required)
— Upper-case characters in a spec `name` are lower-cased on parse rather
than rejected.**
- *Locator:* `REQ-SHELL-THEME` ("a `name` (1-32 characters of
  `[a-z0-9-]`)").
- *Observed:* `parse_theme_spec` lower-cases the name before the character
  check, so a spec written as `name = "Midnight"` parses as `midnight`
  instead of failing; the in-crate test `parse_theme_spec_validates_and_
  applies_overrides` pins the lower-casing. The behavior is deterministic
  and the observable name space is still `[a-z0-9-]`; it only relaxes the
  input grammar. Recorded for completeness; it does not weaken any other
  clause.

## Disposition summary

| ID | Surface | Clause | Disposition | Severity |
|----|---------|--------|-------------|----------|
| C-1  | theme  | semantic palette, no hard-coded colors | compliant | — |
| C-2  | theme  | built-in `dark` (default) and `light` | compliant | — |
| C-3  | theme  | `theme` live preview of every theme | compliant | — |
| C-4  | theme  | `theme set` immediate switch + persistence | compliant | — |
| C-5  | theme  | resolution precedence chain | compliant | — |
| C-6  | theme  | unresolvable preference warns → detection | compliant | — |
| C-7  | theme  | TOML spec format (name/mode/colors) | compliant (subject to N-1) | — |
| C-8  | theme  | 8 KiB cap, unknown roles, actionable failure | compliant | — |
| C-9  | theme  | plain-text degradation for every theme | compliant | — |
| C-10 | theme  | independent RGB palettes, canvases, surfaces | compliant | — |
| C-11 | theme  | 4.5:1 text / 3:1 edge contrast (built-ins) | compliant (tested-combination limit) | — |
| C-12 | theme  | same palette reaches Blad and UI chrome | compliant | — |
| C-13 | pager  | paging policy (threshold, `KVIST_NO_PAGER`) | compliant | — |
| C-14 | pager  | proportional scrollbar, two-row floor, offset | compliant | — |
| C-15 | pager  | full-height per-row layout, gutter/scrollbar | compliant | — |
| C-16 | pager  | required keys and mouse wheel | compliant | — |
| C-17 | pager  | direct-print fallback, no lost output | compliant | — |
| C-18 | pager  | piped/captured/width-unknown never paged | compliant | — |
| C-19 | process | root change-and-compliance rules | compliant (see F-1) | — |
| F-1  | record | clean-slate observation retained in `IMPL.md` | underspecified (gap) | medium |
| F-2  | theme  | state recognizable without color alone | mismatched/underspecified | medium |
| F-3  | theme  | contract names the user preference file | mismatched (coverage gap) | low |
| F-4  | theme  | design: completion menu shows the full preview | mismatched (design-claim gap) | low |
| N-1  | theme  | name grammar accepts upper case | info (benign deviation) | info |

**Compliant:** 19 of 22 assessed clauses.
**Discrepancies requiring human arbitration:** 4 (F-1 medium, F-2 medium,
F-3 low, F-4 low), plus one informational observation (N-1).
**Approved deferrals:** none recorded (none were proposed for approval).

## Conclusion

The implemented theming and scroll-pager behavior corresponds to
`REQ-SHELL-THEME` and `REQ-SCROLL-PAGER`, the contract, and the design for
every clause this review could assess against the implementation record and
the bounded test evidence. Four items are retained for explicit human
arbitration: the missing pre-existing clean-slate observation in
`IMPL.md` (F-1, now remediated by the appended observation, pending
attribution decision), the color-only state-recognition clause with no
defined scope or observed basis (F-2), the contract's silence on the user
preference file (F-3), and the design's claim that the completion menu
shows the full preview when it shows a one-line sample (F-4). This review
records satisfied clauses,
discrepancies, and required arbitration; it does not certify the surface,
accept intent, or substitute for human arbitration. Per the current
enforcement note, `component accept` only structurally validates and
records revisions and does not enforce this gate.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; the cited suites are run automatically by Kvist in the
  verification sandbox after this turn, and no result from that run is
  asserted here.
- The runbook precondition to run the documented quality gate and `kvist
  doctor .` and resolve any non-`current` state was not executed in this
  context.
- The nearest-ancestor component contract is absent (`maerg` is the root
  component); `ROOT_CONTRACT.md` is the only propagated higher-level
  context.
- The contrast clause (C-11) is compliant only for the tested combinations
  of the built-in palettes; the record does not certify arbitrary user
  themes, and no user-theme contrast guarantee is asserted.
- The pager key handling, wheel step, 200 ms event poll, and less-style
  reprint are observed in the implementation record; the no-page and
  geometry properties are additionally pinned by in-crate assertions, and
  the alternate-screen enter/leave by the PTY integration test. Live
  terminal interaction (actual wheel and key input against a running pager)
  is not executed evidence.
- The F-1 remediation observation was produced in this task's context;
  whether it satisfies the independent documenter requirement is itself
  one of the arbitration options for F-1 and is not assumed here.
- The security-audit stage of this chain is completed per the queue, but
  its report is not relied upon and its conclusions are not repeated in
  this record.
- All "observed" statements are attributed to the clean-slate source
  derivation recorded in `IMPL.md` (2026-11-20 section), to the cited
  in-crate and integration test assertions, or (for the absence of an
  observation) to `IMPL.md` itself; this review does not modify
  `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`, the implementation, or any
  prior record except for the retained append to `IMPL.md` noted in F-1.
