# Blad: Text-Only Terminal Rendering Plan

**Date:** 2026-10-21

**Status:** Accepted by the human on 2026-10-21 for milestone-by-milestone
implementation with separate commits. Local requirements/contract/design and
the queue record the executable breakdown. Acceptance of this plan does not
claim completed features, a selected dependency, review receipts or compliance.

## 1. Outcome and scope

Make Blad a richer text-only terminal Markdown viewer through coherent themes,
Unicode subscript/superscript, limited mathematical notation, and Mermaid
diagrams drawn with text boxes, connectors, arrows, and labels.

```text
+----------+     +----------+     +----------+
| Markdown |---->| Renderer |---->| Terminal |
+----------+     +----------+     +----------+
```

Both Unicode box drawing and an ASCII structural-glyph alternative are in scope.
ANSI styling is optional; colors must never be necessary to understand content.

**Excluded:** PNG/SVG generation or display, including intermediate graphical
rendering; image protocols; browser/WebView previews; Chromium/Node/Graphviz
runtime requirements; full TeX typesetting; a complete Mermaid implementation
written in Blad; unrelated viewer, session-storage, folding, or streaming work.

This is a focused extension of the earlier local `.work-docs/blad/PLAN.md`,
not a restart of its broader backlog. For this work, it replaces earlier
suggestions of optional image rendering or document-triggered external Mermaid
executables. The current component artifacts remain authoritative until
deliberately updated.

## 2. Feasibility and implementation choices

| Capability | Proposed result | Approach |
| --- | --- | --- |
| Themes and color | Coherent dark, light, terminal-native, and monochrome presentation across content and viewer chrome | Extend the existing renderer with semantic palette roles and explicit output capabilities |
| Subscript/superscript | Common digits, operators, and a curated letter subset; explicit notation for unsupported spans | Share small Unicode mappings with the math renderer |
| Limited math | Greek letters, common operators, scripts, grouping, fractions, roots, and plain text runs | Enable existing Markdown math events; implement a bounded text projection, evaluating a parser dependency rather than growing a general TeX parser |
| Mermaid | Readable spatial text diagrams for a declared syntax/family subset | Embed an existing native Rust text renderer behind a small private adapter |
| Unsupported syntax | Original source with an informative explanation, not invented geometry or a success placeholder | Explicit supported/unsupported/invalid/resource-failure handling |

### Existing behavior to change deliberately

- `blad/src/markdown.rs` enables sub/superscript extensions but not
  `ENABLE_MATH`; current math is literal passthrough.
- Recognized sub/superscript spans only receive dim styling. Current adjacent
  forms such as `H~2~O` remain literal, so a glyph map alone is insufficient.
- The Mermaid adapter invokes `mmdc`, discards successful graphical output,
  and returns a placeholder. Remove that path rather than extend it.
- The library already has background and syntax-theme setters, but colors
  are scattered and syntax-theme defaults are dark-oriented.
- Standalone dump currently writes renderer ANSI rather than consistently
  satisfying its plain-text promise.
- The viewport already has display-column-aware clipping and horizontal
  overflow. Reuse those capabilities rather than wrapping diagram rows.

The supplied local `.work-docs/terminal_capabilities_findings.md` is useful
background, with these corrections: Unicode script letters exist outside the
listed blocks; dim is not smaller text; measured widths are not universal
font/terminal guarantees; and native Rust Mermaid-to-text implementations exist.
The claim that a converter covers "90%" of math is not an acceptance criterion.

## 3. Proposed consumer semantics

These options and defaults are proposals, not existing CLI interfaces.

| Option | Proposed meaning |
| --- | --- |
| `--theme auto\|dark\|light\|terminal` | Select appearance; default `auto`; `terminal` preserves terminal-native base colors and uses sparse accents |
| `--color auto\|always\|never` | Select whether color is emitted; default `auto`; independent of light/dark appearance |
| `--math unicode\|source` | Render the supported math subset or retain its TeX source; default `unicode` |
| `--mermaid text\|source` | Render supported diagrams as text or retain the source; default `text` |
| `--diagram-charset unicode\|ascii` | Choose renderer-owned diagram structure; default `unicode`; authored Unicode labels are never transliterated |

The initial configuration surface is CLI options plus typed library settings.
Do not introduce persistent preferences or an arbitrary theme-file schema.
Embedding applications can supply their own palette through the library.

### Output and configuration rules

- Resolve standalone CLI/environment/capability policy once and pass explicit
  settings through all existing file, session, and dump rendering routes.
  Library rendering must not query a terminal or launch a subprocess.
- Explicit `--color always` overrides `NO_COLOR`; `auto` honors a nonempty
  `NO_COLOR`, output TTY state, `TERM=dumb`, and supported color depth.
  `never` disables added foreground/background colors, not terminal navigation.
- `--dump` always produces escape-free rendered text, even with a color/theme
  override. Unicode text is still allowed. Markdown export remains source
  Markdown, not a rendering.
- An embedding host supplies resolved appearance/capabilities; Blad must not
  override its palette by guessing from the process environment.
- Use RGB, ANSI256, ANSI16, or monochrome as supported. Do not infer background
  brightness from truecolor support.
- Automatic background detection is best-effort. Any terminal query must have
  a real I/O deadline, decode hexadecimal responses correctly, preserve
  unrelated input, and never consume piped input. Ambiguity has a documented
  default, with explicit theme selection always available.
- Invalid option values and unknown explicitly requested syntax themes are
  actionable errors, not silently selected alternatives.

### Scripts and math

- Preserve already-authored Unicode. Convert only recognized Markdown/math
  constructs, never ordinary prose or fenced/inline code.
- Support Markdown `^...^` and `~...~`, including adjacent examples such as
  `E=mc^2^` and `H~2~O`. Investigate the parser's delimiter rules first; choose
  a narrow parser-aware extension if needed, not global text replacement.
- Treat these as documented dialect extensions, not universal GFM behavior.
  Arbitrary HTML `<sub>`/`<sup>` interpretation is not part of this increment.
- Map a complete script span only when its characters have approved glyphs.
  Otherwise preserve its distinction with `^(...)` or `_(...)`; dim styling
  may supplement, but never replace, that notation.
- Math input uses the Markdown parser's `$...$` and `$$...$$` events. Define
  escaped-dollar and currency behavior before enabling the option.
- First math grammar: literals, nested braces, Greek/common operator commands,
  `_`/`^`, `\frac`, `\sqrt`, `\text`, and a declared subset of `\left`/`\right`
  delimiters. List exact supported commands in user documentation.
- Preserve grouping: `\frac{a+b}{c+d}` becomes `(a+b)/(c+d)`, not `a+b/c+d`;
  `\sqrt{x+1}` becomes `sqrt(x+1)` or its unambiguous Unicode equivalent.
- Unknown commands, unsupported layouts, or malformed expressions retain the
  whole original expression and delimiters. Do not partially convert a
  formula whose meaning cannot be preserved.
- Display math occupies a separate text block. No stacked fractions, matrices,
  aligned systems, custom macros, file inclusion, or executable TeX.

### Mermaid

- Initial target families: flowchart, sequence, state, class, and ER.
  Admission of each family depends on the selected engine and evidence, not
  merely its parser accepting a diagram header.
- Publish a support matrix distinguishing spatial diagrams, structured-text
  projections, approximated presentation, and unsupported constructs.
  A summary must never be presented as a fully drawn diagram.
- Preserve node/participant labels, edge endpoints, direction, and edge labels
  for the supported subset. Unsupported semantic constructs fall back to
  source rather than silently disappearing.
- Use the authored direction. Prefer horizontal scrolling when a diagram
  cannot fit; do not rotate the graph, truncate labels, or wrap completed rows
  automatically. Compact layout may reduce spacing without losing semantics.
- On resize, use one width profile consistently for layout and display.
  Define and exercise behavior for wide graphemes and ambiguous-width glyphs.
- Retain searchable text and source access. Never invent a second persistent
  diagram format or change the stored Markdown/transcript.
- Input, semantic complexity, layout work, grid extent, output bytes, and
  nesting require explicit bounds. Resource failures retain their diagnostic;
  they are not reported as successful rendering.

## 4. Reuse decisions and evaluation gate

### Mermaid shortlist

1. **Preferred evaluation candidate: Merman 0.9.1**, using the `merman`
   facade with defaults disabled, `ascii`, and only the admitted `diagram-*`
   features. Its documented terminal resource policy, cancellation, palette,
   width profiles, and capability reporting are a strong fit.
2. **Alternative: mmdflux 2.6.1**, as a library with CLI defaults disabled.
   It has a smaller declared runtime dependency surface and spatial text
   output for flowchart, sequence, state, and class. ER is not documented in
   that release; choosing it requires an explicit first-release scope change.

These are candidate baselines, not approved dependencies. Evaluate pinned
releases rather than assuming default-branch documentation matches a release.
Do not combine two engines to paper over coverage gaps in the first increment.
Do not build a Mermaid-to-DOT translator.

The comparison must record:

- exact corpus results and unsupported semantics for every proposed family;
- label, endpoint, and direction preservation, plus readable route geometry;
- 40/80/120-column behavior, tiny/empty viewports, Unicode and CJK labels;
- malformed input, controls in labels, resource exhaustion, and cancellation;
- selected feature graph, added dependencies, MSRV, license/notice obligations,
  offline vendoring cost, and absence of graphical/runtime requirements;
- measured cold/repeated render cost and memory on small and larger examples.

Before production integration, record numeric input/work/grid/output limits
and performance budgets in the design, calibrated against that corpus. Reject
an engine that cannot meet the chosen correctness and boundedness requirements.
No published speedup or broad compatibility claim substitutes for this check.

### Math and themes

Reuse pulldown-cmark, syntect, and the existing Unicode/terminal helpers.
Evaluate MIT-licensed `pulldown-latex` for math parsing if it reduces the
complexity of the bounded grammar; Blad still owns the text projection.
Only use a local parser if its explicitly small grammar is clearer and safer.
`unicodeit` is a symbol-conversion reference, not a full math renderer, and its
MIT/LPPL combination needs a separate licensing decision before reuse.

Dependencies must be acquired through the approved host vendoring phase, with
the lock file and vendor state updated together. Normal builds and tests stay
offline and locked. Do not install optional executables during verification.

## 5. Implementation structure

Keep parsing/rendering separate from terminal ownership:

```text
CLI or embedding host
  -> explicit render settings and terminal capabilities
  -> Markdown parser
  -> prose / code / math / diagram text rendering
  -> display-column layout and existing viewport clipping
  -> styled terminal output OR escape-free dump
```

Extract only focused helpers as needed: palette/output policy, shared script
maps and math projection, and a private Mermaid adapter. Keep business logic
out of `main.rs`; do not introduce a general plugin framework or require a
whole-viewer rewrite.

Reuse expensive immutable syntax/theme assets for the renderer lifetime.
Measure repeated diagram rendering before adding a bounded in-memory cache.
If needed, its key includes source, relevant width, charset, palette, and
engine/options identity; resize/theme changes must not return stale geometry
or styling. No on-disk cache is required.

Renderer errors must distinguish unsupported syntax, invalid input, resource
limits, cancellation, and writer failure. Unsupported-content notices can
accompany source display; cancellation and output failures must propagate.
Only renderer-owned terminal controls may reach output.

## 6. Delivery plan and dependencies

### P0: Define and approve the feature boundary

Update Blad's local requirements, contract, design, and task definitions
before implementation. Give outcomes, public semantics, and private algorithms
their respective authoritative homes; allocate fresh requirement IDs without
copying conflicting proposed IDs from the historical plan.

Include the engine comparison and its pass/fail criteria in that intent.
Record the advisory-review opportunity or a deliberate exception using the
project's actual available mechanism. Current `component accept` only performs
structural/revision recording; do not claim it enforces advisory review.
Do not mark proposed scope or dependencies accepted on the strength of this
planning document.

### Milestone packets

For each packet, create atomic, requirement-linked queue tasks in this order:
`write_tests`, `implement_code`, `security_audit`, `compliance_review`.
Tests must fail against the behavior being intentionally changed before
production changes. Do not weaken existing passthrough/style assertions merely
to make the new implementation pass; replace them from approved semantics.

| Packet | Work | Prerequisites | Exit condition |
| --- | --- | --- | --- |
| P1: Themes/output | Typed settings, semantic palette, syntax-theme pairing, color capability policy, CLI plumbing, plain dumps, and existing host integration | P0 | All existing rendering entry points use one coherent policy; monochrome/dump retain content |
| P2: Script spans | Shared glyph maps, parser-aware adjacent-span support, whole-span fallbacks, nesting/style preservation | P0 and P1 | Documented Markdown examples work without changing ordinary prose/code |
| P3: Limited math | Parser decision, bounded projection, enabled math events, block layout, exact source fallbacks | P1 and P2 | Accepted grammar renders unambiguously; unsupported formulas remain intact |
| P4: Text Mermaid | Corpus-based engine selection, approved vendoring, adapter, removal of `mmdc`, block/viewport integration | P0 and P1; approved dependency acquisition before building candidate code | Admitted families draw real text diagrams without runtime tools or graphical output |
| P5: Release integration | Cross-feature/CLI/PTY coverage, consumer verification, user documentation and final evidence comparison | P2, P3, and P4 | Exit checklist below is satisfied or discrepancies are explicitly retained for human arbitration |

P4's library evaluation need not wait for P2/P3, and Mermaid does not depend on
the math parser. Keep these dependency edges explicit instead of imposing
dependencies solely because packets appear in a numbered order.

For implementation records, use a clean-slate documenter with source/test
evidence only, excluding intent, queue, prior record, reviews, chat, and Git
history. A separate source-blind reviewer compares that observation and test
evidence against the local requirements/contract/design. Neither author-side
checks nor advisory feedback certify compliance or human acceptance.

## 7. Acceptance and verification checklist

- Dark/light/terminal palettes cover headings, links, alerts, footnotes, math,
  code, tables, search cues, and header/footer chrome without altering text
  geometry. No meaning depends solely on color.
- Normalized visible text is identical across color modes at the same layout
  settings; dump contains no ESC/control protocols or added styling.
- Check TTY/non-TTY behavior, `NO_COLOR`, explicit overrides, ANSI16/256/RGB,
  bounded background-query timeout/malformed response, and absent stdin TTY.
- Script examples include `H~2~O`, `E=mc^2^`, unsupported alphabets, mixed
  supported/unsupported spans, nested emphasis, code, and table cells.
- Math examples include Greek/operators, grouped fractions/roots, scripts,
  nested braces, text runs, display blocks, currency, escaped dollars,
  unknown commands, unmatched delimiters, and nesting/input limits.
- Mermaid corpus covers every admitted family, branches/cycles, directions,
  nested groups, repeated renders, long/multiline labels, edge labels,
  malformed/unsupported constructs, controls, Unicode, and explicit limits.
  Assert labels/endpoints/directions and spatial invariants, not only a
  "Mermaid diagram" substring or a golden snapshot.
- Use PTY coverage for tiny screens, resize, line-number gutter, scrolling,
  and text-diagram clipping without terminal autowrap or split graphemes.
- Verify file/session rendering, existing reasoning/replay consumers, and
  source Markdown export. Do not add unrelated session lifecycle changes.
- Exercise writer errors, cancellation, and resource-limit diagnostics.
  Results must not depend on installed `mmdc`, `dot`, or browser tools.
- Run targeted existing tests first; escalate to Blad's complete suite and
  consumer compilation/integration for the affected public API. Use existing
  formatting and strict lint commands with offline, locked Cargo resolution.
- Update `blad/README.md`, `blad/docs/USAGE.md`, rustdoc, and the local intent
  and task state together with deliberate behavior changes. Document actual
  supported profiles, fallback behavior, defaults, and measured limitations.

## 8. Research references

Source/documentation research, not evidence of an integrated Blad backend:

- [Merman 0.9.1 terminal backend](https://github.com/Latias94/merman/tree/v0.9.1/crates/merman-ascii)
- [Merman 0.9.1 terminal support matrix](https://github.com/Latias94/merman/blob/v0.9.1/docs/rendering/ASCII_SUPPORT_MATRIX.md)
- [mmdflux 2.6.1](https://github.com/kevinswiber/mmdflux/tree/mmdflux-v2.6.1)
- [pulldown-latex parser](https://docs.rs/pulldown-latex/latest/pulldown_latex/)
- [pulldown-cmark 0.13.4 options](https://docs.rs/pulldown-cmark/0.13.4/pulldown_cmark/struct.Options.html)
- [NO_COLOR convention](https://no-color.org/)
