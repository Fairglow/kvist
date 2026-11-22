<!-- kvist-compliance-review-version: 1 -->
# Compliance Review: Shell Soft Wrapping (`maerg`)

**Component:** `maerg` (Kvist root component)
**Task:** `soft-wrap-compliance-review`
**Scope:** The shell soft-wrapping surfaces — the pure wrapping primitives
(`wrap_line`, `wrap_bordered_line`, `titled_box`/`titled_box_width`), the
display policy (`prepare_display`, `display_output`), the scroll-pager re-wrap,
the `Prompt`/streaming stages, the welcome banner, and the bordered
`│…│` status overview — compared against `REQ-SOFT-WRAP`, the interactive-shell
contract (the soft-wrapping and paged-output paragraph), and the design's
"Soft wrapping" section.
**Reviewer stance:** Independent compliance reviewer, in two phases per the
task prescription: a clean-slate observation of observed behavior derived from
the implementation source and its tests (record, prior reviews, and Git history
out of view), then a source-blind comparison of that derivation against
`IMPL.md`, `CONTRACT.md`, and `DESIGN.md`. The implementer and the security
auditor did not author this record.

## Review boundary and independence

Per `REVIEW_RUNBOOK.md` *Source-blind compliance pass*, the compliance reviewer
records each requirement clause, contract guarantee, and relevant design claim
as **compliant**, **mismatched**, **approved-deferred**, or **underspecified**,
with component path, stable locator, intended text, observed behavior/evidence,
severity, and owner. The nearest-ancestor component contract is `null`
(`maerg` is the root component); `ROOT_CONTRACT.md` is the only propagated
higher-level context.

**Read (permitted intent evidence):**
- `maerg/REQUIREMENTS.md`
  `sha256:c786ec71ab7aa367e6305ae1a715adb6a16c7126410fe72dd760c83d9f3e712e`
  (matches the prior review chain's `requirements_revision`),
- `maerg/CONTRACT.md`
  `sha256:957ce69d562103d49f26cac15691f96f1fe3ab9ab0e43962af6cb89776d9d53a`,
- `maerg/DESIGN.md`
  `sha256:bc195225b837d3416c7d9fe9bb154759606bdbd4246dc768687e0349f0acef3f`,
- `maerg/IMPL.md`
  `sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`
  (checked for a soft-wrapping observation; see F-2),
- `ROOT_CONTRACT.md`
  `sha256:b4dd022afb6237cd20c255a0b9476348e9e76e43bfae4878d4626136be535252`,
- `REVIEW_RUNBOOK.md` `sha256:385bccc91eeaf47ac98c3e8d2990619d91c31655d071f98a8275fb99168690b2`
  (procedure).

**Clean-slate observation carrier (phase 1):** this task prescribes two phases.
Phase 1 derived observed behavior by reading the implementation source and its
in-crate `#[cfg(test)]` suites — `maerg/src/bark/style.rs`
(`sha256:4714266d33c8462362cb186dfc0304b59c97217cbd4d3b6e6714933b08c2c456`),
`maerg/src/bark/pager.rs`
(`sha256:c300a97421e4ffff9d4d0e9d5b6c26ed0a52260ca4fee1fe479047375a91926c`),
`maerg/src/bark/scroll_pager.rs`
(`sha256:0bac90d59467a99d0bb8260c2197e0e59d5f043d21ab0996db61f095bc43bc41`),
`maerg/src/bark/stream.rs`
(`sha256:9b5b836cfb0d2f972d070a9b546a10b62e7e3d072bf9a1830c70b9e6f83b4344`),
`maerg/src/bark/status.rs`
(`sha256:d3137bfaecf48432bf16d5562249f83284e35fddc1c977b0d7b9cee67bb60a4e`),
`maerg/src/bark.rs`
(`sha256:e110cd3a316ac9fc9f0524aff110694f3dc622c845ffa3e87c3b4131870509b8`),
and `maerg/src/status.rs` (the bordered `│…│` overview builder) — treating the
implementation and its tests as the evidence base, with the implementation
record (`IMPL.md`), prior reviews, Git history, and peer artifacts out of view.
Phase 2 (source-blind comparison) compared the implementation record
(`IMPL.md`), `CONTRACT.md`, and `DESIGN.md` against that derivation and the
bounded test evidence.

**Observation limitation (stated for the arbitrator):** unlike a few prior
chains, this component has **no `tests/` integration suite** for the
soft-wrapping surfaces. A term sweep of `maerg/tests/` for `wrap`,
`titled_box`, `bordered`, `prepare_display`, and `scrollbar` found no
integration test; every executable statement of the wrap behavior is an
in-crate `#[cfg(test)]` suite inside the `bark` module (source-side evidence,
not the runbook's bounded `tests/` carrier). The byte-identity clause was
therefore checked against the `terminal_size` dependency's documented and
vendor-sourced behavior rather than against a `tests/` case. This does not
weaken the retained findings, because F-1 and F-2 are each independently
supportable from the vendor `terminal_size` source, the dependency manifest,
`IMPL.md`, `CONTRACT.md`, `DESIGN.md`, and the in-crate assertions alone.

**Not read as evidence:** peer component implementations, `sav/`, `galla/`,
`skott/`, `blad/` source, Git history or diffs, prior compliance conclusions,
and the security-audit stage's report (completed per the queue, but not in this
reviewer's permitted read set). No tests, builds, linters, or `cargo`
invocations were executed in this review; the `bark` and related suites are
run automatically by Kvist in the verification sandbox after this turn, and no
pass/fail count from that run is asserted here.

*Context-isolation note (stated for the arbitrator):* both phases were
performed in a single session, so this is the analytical separation the task
prescribes (derive observed behavior from source and tests, then judge the
implementation record against intent) rather than the runbook's fully isolated
separate-agent source-blind context. The intent texts (`REQUIREMENTS.md`,
`CONTRACT.md`, `DESIGN.md`) and `IMPL.md` were part of the same session as the
phase-1 source read. That limitation does not weaken the retained findings,
because F-1 is independently supportable from the `terminal_size` vendor
source, the dependency manifest, `CONTRACT.md`, `DESIGN.md`, and the
in-crate assertions alone (none of which depend on the phase-1 read), and F-2
is established by a term sweep of `IMPL.md` that a fully isolated
source-blind reviewer would perform identically.

## Bounded evidence relied upon

The executable statements of observed behavior are the in-crate `#[cfg(test)]`
suites co-located with the implementation:

- `style.rs` tests: `wrap_line_breaks_at_word_boundaries`,
  `wrap_line_keeps_the_line_indentation_on_continuations`,
  `wrap_line_aligns_key_value_continuations_under_the_value`,
  `wrap_line_key_value_never_overflows_the_width`,
  `wrap_line_without_a_key_keeps_the_plain_indent`,
  `wrap_line_hard_splits_unbreakable_words`,
  `wrap_line_hard_splits_overlong_multibyte_words`,
  `wrap_line_re_emits_styling_on_every_physical_line`,
  `wrap_line_handles_mixed_styled_and_plain_spans`,
  `wrap_line_is_the_identity_when_the_line_fits`, `wrap_line_tolerates_degenerate_input`,
  `wrap_bordered_line_keeps_the_frame_on_every_physical_line`,
  `wrap_bordered_line_is_the_identity_when_the_line_fits`,
  `wrap_bordered_line_falls_back_to_plain_wrap_for_unframed_lines`,
  `titled_box_lines_share_one_visible_width`,
  `titled_box_fits_narrow_terminals_and_wraps_wide_content`,
  `titled_box_never_goes_below_40_columns`,
  `titled_box_wraps_wide_content_instead_of_extending_past_the_terminal`.
- `pager.rs` tests: `prepare_display_wraps_bordered_and_plain_lines_for_a_known_width`,
  `prepare_display_is_the_identity_without_a_width`, and the `pager_policy`
  threshold cases.
- `scroll_pager.rs` tests: the `scrollbar_geometry` proportionality, two-row
  floor, and fit-fills-the-track cases, and the `render` full-height /
  last-column scrollbar cases.
- `stream.rs` tests: `result_stage_retains_long_outcomes_and_multiline_errors`
  and `result_stage_rows_truncates_multibyte_reasoning_on_a_char_boundary`
  (long result rows retained, not discarded; char-boundary truncation).
- `status.rs` tests: `overview_change_details_are_bordered_and_aligned`
  (every overview line is `│`/`╭`/`╰` framed) and the `welcome_banner`
  suites (every banner line shares one visible width; long branch names fit
  the box).

These are run automatically by Kvist after this turn; no result from that run
is asserted here.

## Method

Every obligation was drawn from `REQUIREMENTS.md` (`REQ-SOFT-WRAP`),
`CONTRACT.md` (the interactive-shell paragraph that states the soft-wrapping
and paged-output guarantees, `Protected resilience`; `Provided interfaces` for
the "Long result lines wrap without discarding error details" line), and
`DESIGN.md` ("Soft wrapping"). Each was compared against the observed behavior
in `IMPL.md` and the bounded test evidence. Because `IMPL.md` is silent on
these surfaces (F-2) and no `tests/` integration suite exists, the phase-1
clean-slate source derivation and the in-crate assertions are the primary
observation carriers; each clause below cites its intent text, the
clean-slate observation, and the matching in-crate assertion where present.

## Satisfied clauses (compliant)

**C-1 — Long lines are soft-wrapped at word boundaries before rendering, so no
line overflows the terminal width.**
`REQ-SOFT-WRAP` ("long lines MUST be soft wrapped at word boundaries before
rendering"). `CONTRACT.md` ("the shell soft-wraps output at word boundaries so
no line overflows"). Clean-slate observation: `wrap_line` tokenizes on
whitespace and `place_token` starts a new physical line when the next token
would not fit; `wrap_bordered_line` and the scroll-pager `wrap` route every
line through the same primitive. In-crate evidence:
`wrap_line_breaks_at_word_boundaries` (`"one two three four"` at 8 →
`["one two","three","four"]`) and `wrap_line_key_value_never_overflows_the_width`
(asserts `visible_len(line) <= 16` for every physical line). Compliant.

**C-2 — Boxed surfaces keep their borders intact on every physical line.**
`REQ-SOFT-WRAP` ("boxed surfaces … keep their borders intact on every physical
line"). `CONTRACT.md` ("titled boxes (welcome banner, streaming stages, the
`Prompt` stage) wrap their rows to the box's inner width … and the border is
drawn on every physical line; bordered status-report lines (`│…│`) wrap their
inner content with the border and padding re-drawn per line"). Clean-slate
observation: `titled_box` wraps each row to the box's inner width and re-frames
it as `│  <row padded to inner> │`; `wrap_bordered_line` strips the frame,
wraps the trimmed inner content, and re-draws `│…│` on each physical line.
In-crate evidence: `titled_box_wraps_wide_content_instead_of_extending_past_the_terminal`
(every top/row/bottom line shares one visible width),
`wrap_bordered_line_keeps_the_frame_on_every_physical_line` (each physical line
starts and ends with `│` and is exactly the frame width), and
`status.rs::overview_change_details_are_bordered_and_aligned` (every overview
line is `│`/`╭`/`╰` framed). Compliant.

**C-3 — The box fits the terminal cap and does not extend past the terminal;
the Prompt stage keeps the full command line.**
`CONTRACT.md` ("at least 40 columns, never wider than the terminal width minus
a margin or 100 columns, whichever is smaller"). `DESIGN.md` ("the `Prompt`
stage passes the full command line to the box and no longer truncates it").
Clean-slate observation: `titled_box_width` computes
`content_width.max(title_width)+4` clamped to `[40, min(term−2, 100)]` (80 cap
when unknown), and `print_prompt_stage` passes the full `prompt_line` as the
single row (no truncation) before `titled_box` wraps it to the inner width.
In-crate evidence: `titled_box_never_goes_below_40_columns` (a 10-column
terminal still renders a 40-column box) and
`titled_box_fits_narrow_terminals_and_wraps_wide_content` (a 120-column row on
a 60-column terminal renders a 58-column box with `rows.len() >= 2`). Compliant.

**C-4 — Text blocks keep their indentation, with continuation lines prefixed by
the line's leading whitespace.**
`REQ-SOFT-WRAP` ("text blocks keep their indentation, with continuation lines
prefixed by the line's leading whitespace"). `CONTRACT.md` ("plain lines wrap
with each continuation line prefixed by the line's leading whitespace so text
blocks keep their indentation"). Clean-slate observation: `wrap_line` splits
the leading whitespace off as `indent` and re-issues it as the head and every
continuation tail. In-crate evidence:
`wrap_line_keeps_the_line_indentation_on_continuations`
(`"  hello world foo bar"` at 12 → `["  hello","  world foo","  bar"]`).
Compliant.

**C-5 — A `key: value` line wraps with its continuation lines aligned under the
start of the value, not the key.**
`REQ-SOFT-WRAP` ("A `key: value` line (a word, a colon, and at least one space
after the leading whitespace) MUST wrap with its continuation lines aligned
under the start of the value"). `CONTRACT.md` ("a `key: value` line wraps with
its continuation lines aligned under the start of the value"). Clean-slate
observation: `key_prefix` recognizes a word (`[A-Za-z]` then
`[A-Za-z0-9_-]`), a colon, and at least one following space; the head is
`indent + key + " "` and the continuation tail is a spaces-only prefix of equal
width, so continuations land in the value column; a colon without a following
space (the `sha256:` of a revision) is not treated as a key. In-crate evidence:
`wrap_line_aligns_key_value_continuations_under_the_value` (every continuation
indented to the value column) and `wrap_line_without_a_key_keeps_the_plain_indent`
(`sha256:` is not a key; continuations keep the plain indent). Compliant.

**C-6 — Wrapping is a pure function of the text and the probed terminal width.**
`REQ-SOFT-WRAP` ("Wrapping MUST be a pure function of the text and the probed
terminal width"). Clean-slate observation: `wrap_line`, `wrap_bordered_line`,
`prepare_display`, and `titled_box` take only their text and a width and
perform no I/O, no time, and no randomness; the width is obtained once per
call from `terminal_size()`. In-crate evidence: the deterministic
`prepare_display_wraps_bordered_and_plain_lines_for_a_known_width` and the
fixed-width `titled_box` assertions. Compliant for the pure-function property.
(The width *probe* that feeds the width is not itself gated on stdout being a
terminal — that is F-1, not a purity defect.)

**C-7 — ANSI styling survives wrapping: a styled span re-emits on every physical
line it touches.**
`REQ-SOFT-WRAP` ("ANSI styling MUST survive wrapping (a styled span re-emits on
every physical line it touches)"). Clean-slate observation: `parse_spans`
tracks the open SGR code to its `ESC[0m` reset, `Token` carries its span's code,
and `render_tokens` re-emits the SGR open plus reset on each physical line,
grouping consecutive same-style tokens. In-crate evidence:
`wrap_line_re_emits_styling_on_every_physical_line` (every physical line begins
with the open SGR and ends with the reset, and the visible content is
preserved in order) and `wrap_line_handles_mixed_styled_and_plain_spans`.
Compliant.

**C-8 — Output that fits is unchanged (identity); unbreakable words longer than
the available width are hard-split at the width rather than dropped.**
`REQ-SOFT-WRAP` ("Unbreakable words longer than the available width MUST be
hard-split at the width rather than dropped") plus the identity implicit in the
byte-identity clause. `DESIGN.md` ("Each primitive is the identity for a line
that already fits, so output is byte-identical until a line actually
overflows"). Clean-slate observation: `wrap_line` returns `vec![line]` when the
line fits (or width is 0); `place_token` hard-splits an over-long token on char
boundaries at `avail`, never dropping characters. In-crate evidence:
`wrap_line_is_the_identity_when_the_line_fits`,
`wrap_line_hard_splits_unbreakable_words` (`"abcdefghij"` at 4 →
`["abcd","efgh","ij"]`), and `wrap_line_hard_splits_overlong_multibyte_words`
(a 30-`é` word at 10 → 3 lines, `concat()` equals the input, all lines ≤ 10).
Compliant.

**C-9 — Paged output re-wraps at the pager's content width; the pager never
pages piped or width-unknown output and falls back to direct printing.**
`REQ-SOFT-WRAP` / `REQ-SCROLL-PAGER` (paged surfaces also fit the width);
`CONTRACT.md` ("Output is paged only when it would scroll past the terminal
height …; `KVIST_NO_PAGER` disables paging; … a pager that cannot take the
terminal falls back to direct printing"). Clean-slate observation: the
scroll-pager `page` re-wraps the *raw* text at `width − GUTTER` (so its
full-screen layout is not double-wrapped), and `pager_policy` returns `false`
unless stdout is a terminal, `KVIST_NO_PAGER` is unset, and the line count
exceeds the threshold; a pager that cannot start the terminal returns `false`
and the caller prints directly. In-crate evidence: the `scrollbar_geometry`
cases (proportionality, two-row floor, fit-fills-track), the `render`
full-height/last-column cases, and `policy_never_pages_without_a_terminal_or_with_the_env_override`.
Compliant for the paged-output wrapping and the no-page/fallback policy.

**C-10 — Long result lines wrap without discarding error details or command
arguments.**
`CONTRACT.md` `Provided interfaces` ("Long result lines wrap without
discarding error details or command arguments."). Clean-slate observation: the
streaming result stage passes full command lines and multi-line error reasons
as rows to `titled_box` (which wraps rather than truncates them), and only the
agent-reasoning *snippet* is intentionally truncated to a bounded,
char-boundary-safe length with an ellipsis marker. In-crate evidence:
`result_stage_retains_long_outcomes_and_multiline_errors` (a 144-char command
and a two-line `TaskRunFailed` reason are retained intact; no row contains
`\n`) and `result_stage_rows_truncates_multibyte_reasoning_on_a_char_boundary`
(truncation stays on a char boundary and is marked `...`). Compliant.

## Discrepancies (retained for human arbitration)

**F-1 (MEDIUM, mismatched) — The byte-identity guarantee for piped, captured,
or width-unknown output is not fully upheld: the width is probed from
stderr/stdin as well as stdout, and the `titled_box` surfaces carry no
`is_terminal()` gate, so piped stdout is still soft-wrapped when stderr or
stdin is a TTY.**
- *Locator:* `REQ-SOFT-WRAP` ("Output that is piped, captured, or rendered
  without a known width MUST be byte-identical to the unwrapped form");
  `CONTRACT.md` ("Wrapping is a presentation-only transformation: piped,
  captured, or width-unknown output is byte-identical to the unwrapped form");
  `DESIGN.md` ("`display_output` applies the per-line wrapping only when stdout
  is a terminal with a probed width"; "`prepare_display` … is the identity when
  the width is zero or unknown"); `maerg/src/bark/pager.rs::display_output`
  and the `titled_box`-based surfaces (`bark/status.rs`, `bark/stream.rs`);
  the `terminal_size 0.4.x` dependency (vendor source:
  "This function checks the stdout, stderr, and stdin streams (in that
  order). The size of the first stream that is a TTY will be returned.").
- *Intended (requirement/contract/design):* when output is piped or captured,
  or rendered without a known width, the emitted bytes MUST be identical to
  the unwrapped form so scripts and stable report consumers are unaffected;
  the design frames this as "wrapping … only when stdout is a terminal with a
  probed width."
- *Observed (clean-slate source + dependency source):* `display_output`
  obtains its width from `terminal_size::terminal_size()`, which returns the
  size of the **first TTY among stdout, then stderr, then stdin** — not
  stdout alone. `display_output` then wraps whenever that probed width is
  `Some`, with no `stdout().is_terminal()` gate. The `titled_box`-rendered
  surfaces (the welcome banner, the `Prompt` stage, and the `Agent Working`
  streaming stage) probe the same width and print **directly** (not through
  `display_output`), so they are also ungated on stdout. Consequently, in the
  common case `kvist … > file` where stdout is redirected to a file but
  stderr (or stdin) is still the interactive terminal, the probed width is
  `Some(stderr's width)` and the output is soft-wrapped — **not** byte-identical
  to the unwrapped form. Byte-identity holds only when *no* stream is a TTY
  (fully captured) or when stdout itself is the probed TTY; the "piped stdout"
  case is not covered by the guarantee. The `titled_box` surfaces are pinned
  only for their wrapping (not for piped-identity) and no `tests/` case
  exercises the piped-stdout-with-TTY-stderr path.
- *Why it matters:* this is a genuine contract/requirement mismatch, not a
  documentation nicety. The requirement's stated purpose — "so scripts and
  stable report consumers are unaffected" — is precisely the redirect-to-file
  case, which the observed behavior does transform. It is also a contract
  statement (a consumer may rely on byte-identical piped output) that the
  implementation does not fully honor. Severity is medium rather than high
  because fully-captured output (no TTY at all) is byte-identical, and the
  effect is presentation-only (no data loss, no escape-sequence corruption);
  but it defeats the guarantee in the most common scripted invocation
  (`> file` from an interactive shell).
- *Arbitration options:* (a) gate the wrapping width on `stdout().is_terminal()`
  in `display_output` and on the `titled_box` surfaces (so a known width from
  stderr/stdin does not wrap piped stdout), matching the requirement and
  design intent; (b) if wrapping on a probed TTY width is a deliberate product
  choice (e.g. "fit the controlling terminal even when stdout is redirected"),
  explicitly amend `REQ-SOFT-WRAP`, `CONTRACT.md`, and `DESIGN.md` to state
  that the width may be probed from stderr/stdin and that piped stdout is not
  guaranteed byte-identical — an intent change that must be approved, not
  inferred; (c) record a reasoned exception with an owner. This review does
  not modify `REQ-SOFT-WRAP`, `CONTRACT.md`, `DESIGN.md`, or the implementation.

**F-2 (LOW, mismatched) — `IMPL.md` carries no scoped observation of the
soft-wrapping surfaces, so the runbook's primary observation carrier is absent
for this change.**
- *Locator:* `maerg/IMPL.md`
  (`sha256:e76dcd015b65151d7b8c137cc0869c4a14021497e885a93d049de80929552a84`);
  `ROOT_CONTRACT.md` `Change and compliance rules` ("`IMPL.md` describes
  observed implementation behavior and is not copied from intended
  requirements, contracts, or designs"); `REVIEW_RUNBOOK.md` (the compliance
  reviewer reads "the newly produced `IMPL.md`").
- *Intended (declared):* `IMPL.md` states public behavior, guarantees, limits,
  state effects, failure paths, and known uncertainty derived independently
  from source; the source-blind pass compares intent against that fresh
  observation.
- *Observed:* `IMPL.md` contains no heading, addendum, or scoped observation of
  the soft-wrapping primitives, `display_output`/`prepare_display`,
  `wrap_bordered_line`, `titled_box` sizing and the 40-column floor / 100-column
  cap, the `Prompt`-stage full-command-line behavior, the scroll-pager re-wrap
  and scrollbar geometry, or the `key: value` value-column continuation. A term
  sweep of `IMPL.md` for `wrap_line`, `display_output`, `prepare_display`,
  `titled_box`, `wrap_bordered_line`, `styled piece`, and `soft wrap` found no
  soft-wrapping observation. The only "wrap"-adjacent lines are two incidental
  mentions: one describes long diagnostics reaching "the existing wrapping
  renderer" (error-message wrapping in a task-execution repair observation,
  explicitly "not re-observed" — not the terminal soft-wrapping surface), and
  the other is "wrapped as evidence failure" in a resilience observation.
  Neither is a scoped observation of the soft-wrapping change.
- *Why it matters:* with the observed-behavior carrier absent and no `tests/`
  integration suite present, the source-blind comparison for C-1…C-10 and the
  F-1 byte-identity check degrade to the phase-1 clean-slate source derivation
  plus the in-crate `#[cfg(test)]` assertions and the dependency's documented
  behavior, and the runbook's expectation that the compliance pass compares
  intent against a freshly derived `IMPL.md` is not met for this change.
- *Arbitration options:* (a) commission a clean-slate documenter pass over the
  soft-wrapping change so `IMPL.md` carries the observed behavior (the
  `wrap_line`/`wrap_bordered_line`/`titled_box` primitives, the `key: value`
  and continuation rules, the 40/100 box sizing, the `Prompt`-stage
  full-command-line retention, the `display_output` width probe and its
  stderr/stdin fallback, the scroll-pager re-wrap and scrollbar geometry, and
  the byte-identity property and its limits), then re-run this compliance pass
  against it; (b) record a reasoned exception that the in-crate `bark`
  `#[cfg(test)]` suites are the retained observation for this chain; (c)
  approved deferral with an owner. This review does not modify `IMPL.md`.

## Discrepancy summary

| ID | Finding | Status | Severity |
|----|---------|--------|----------|
| F-1 | Piped/captured byte-identity not fully upheld: width probed from stderr/stdin and the `titled_box` surfaces are ungated on stdout, so piped stdout is wrapped when stderr/stdin is a TTY | mismatched | medium |
| F-2 | `IMPL.md` has no scoped observation of the soft-wrapping surfaces (observation carrier absent) | mismatched | low |

**Total discrepancies requiring arbitration:** 2
**Satisfied (compliant) clauses:** C-1 … C-10 (10)

## Required arbitration

The project architect owns arbitration and, per `REVIEW_RUNBOOK.md`, may (1)
request redesign/implementation changes, (2) explicitly approve an
intent-document change, or (3) record a reasoned exception or approved
deferral. For this review:

1. **F-1** is the substantive item. The word-boundary wrap, indentation,
   `key: value` alignment, ANSI re-emission, hard-split, bordered-frame
   re-draw, box-sizing/cap, Prompt-stage full-command-line retention, and
   paged-output wrapping obligations are met and the in-crate tests pin them;
   the defect is that the byte-identity guarantee for piped/captured output is
   not fully honored because the width is probed from stderr/stdin and the
   `titled_box` surfaces are not gated on `stdout().is_terminal()`.
   Recommended: either gate the wrapping width on `stdout().is_terminal()`
   (implementation change) or explicitly approve an intent-document change to
   `REQ-SOFT-WRAP`/`CONTRACT.md`/`DESIGN.md` documenting the stderr/stdin width
   fallback. Do not silently edit the intent documents to erase the mismatch.
2. **F-2** is a low-severity evidence gap: commission a clean-slate `IMPL.md`
   observation pass over the soft-wrapping change (then re-run this pass), or
   record a reasoned exception that the in-crate `bark` suites are the
   retained observation for this chain.
3. No compliance pass may be self-certified by the implementer or the security
   auditor; this record is the independent source-blind disposition for the
   `soft-wrap-compliance-review` gate. Per the current enforcement note,
   `component accept` only structurally validates and records revisions and
   does not enforce this gate.

## Evidence limits (explicit)

- No tests, builds, linters, or `cargo` invocations were executed by this
  reviewer; the `bark` and related suites are run automatically by Kvist in
  the verification sandbox after this turn, and no result from that run is
  asserted here.
- The runbook precondition to run the documented quality gate and `kvist
  doctor .` and resolve any non-`current` state was not executed in this
  context.
- The nearest-ancestor component contract is `null` (`maerg` is the root
  component); `ROOT_CONTRACT.md` is the only propagated higher-level context.
- There is no `tests/` integration suite for the soft-wrapping surfaces; the
  executable statements are in-crate `#[cfg(test)]` suites (source-side
  evidence), so the bounded `tests/` carrier the runbook prefers is absent for
  this change (see the observation limitation above and F-2).
- The F-1 byte-identity finding rests on the `terminal_size 0.4.x`
  dependency's documented and vendor-sourced stdout→stderr→stdin probe order
  and on the absence of an `is_terminal()` gate in `display_output` and the
  `titled_box` surfaces, as derived in the clean-slate source read; a fully
  isolated source-blind reviewer would rely on `IMPL.md` plus the bounded
  tests, and F-1 and F-2 are each independently supportable from those
  carriers, the dependency manifest/vendor source, and the intent artifacts.
- The security-audit stage of this chain is completed per the queue, but its
  report is not in this reviewer's permitted read set; this review neither
  relies on nor repeats its conclusions.
- All "observed" statements are attributed to the clean-slate source
  derivation, the in-crate `#[cfg(test)]` assertions, the `terminal_size`
  vendor source, or (for the absence of an observation) `IMPL.md`; this
  review does not modify `REQUIREMENTS.md`, `CONTRACT.md`, `DESIGN.md`,
  `IMPL.md`, or the implementation.
