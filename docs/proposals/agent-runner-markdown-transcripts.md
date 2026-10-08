# First-class Markdown transcripts for agent-runner

**Status:** proposal for human approval, not an accepted architecture decision.
**Investigated:** 2026-10-06.
**Scope:** workspace-agent conversation display, private history and explicit
continuation; not Kvist intent, approval or canonical evidence.

## Recommendation

Use **Markdown for message content, small versioned YAML for session structure,
and one shared semantic transcript viewer for live and historical output**.
Do not persist terminal colours or reconstruct trusted message roles from
agent-authored headings, quotes or fences.

Retain Ratatui, `pulldown-cmark` and `syntect` initially. They already exist here.
Evaluate `tui-markdown` against a fixed fixture suite before replacing the
current rendering adapter. Neither a parser nor a Markdown-to-Text crate owns
session identity, folding, cursor navigation, streaming reconciliation or
safe continuation. Those remain application responsibilities.

Make the viewer a child component, proposed at
`agent_runner/transcript_view/`, when its intent is approved. Do not build a new
Markdown grammar, shell out to a pager for the embedded view, or promise browser
features that have no useful terminal representation.

Implement in stages: navigation repair; structured storage and Markdown
history; source-derived re-theming; selectable folds and long-history indexing;
then explicit conversation continuation. Keep folding and continuation out of
the navigation bug fix.

## What was verified in this repository

| Surface | Current behavior | Consequence |
|---------|------------------|-------------|
| History | `src/history.rs` lists `.log` files and loads diagnostic text; `tui/render.rs::render_replay` displays plain lines | History does not use the live Markdown renderer |
| Navigation | Replay handled arrows, j/k, pages and mouse scrolling, but not Ctrl+Home/End; live output handled Ctrl+End, not Ctrl+Home | Boundary navigation needed explicit wiring |
| Scrollbar | Live transcript had a themed Ratatui scrollbar; replay did not | Reuse its renderer and reserve the replay scrollbar column |
| Markdown | `src/markdown.rs` already uses `pulldown-cmark` with tables, task lists and strikethrough, plus `syntect` code highlighting | This is an existing capability, not a greenfield parser project |
| Source retention | Live Markdown blocks retain source for resize, but rows are largely a flat styled display model | Useful foundation, not stable semantic block identity |
| Theme changes | `App::cycle_theme` restyles rows, retaining Markdown/code foregrounds; `default_theme` selects one dark syntax palette | Full re-theming needs rendering again from source and selecting a syntax palette |
| Streaming | `next_block_end` waits for blank-line paragraphs or closed fences, then renders a completed block | This is block buffering, not arbitrary incremental document parsing |
| Reasoning | Live reasoning has a distinct row kind and global collapse/reveal | History has no typed reasoning blocks or per-block focus |
| Persistence | `.jsonl` journal holds operational metadata/hashes; `.log` holds bounded labelled text | Neither is a full structured conversation checkpoint |
| Bounds | History rejects files above 5 MiB; listing is capped at 4096 directory entries; individual transcript text items are capped at 64 KiB | Very large sessions can disappear from the list or lose content; removing limits alone is not a solution |
| Authority | Records are private diagnostics, potentially sensitive and never canonical evidence | Viewing or importing them must not grant execution authority |

Paths in this table are relative to `agent_runner/`. Inspection used source,
tests, manifest and local intent, not private session contents.

The scoped repair accompanying this proposal adds boundary keys, reuses the
themed scrollbar, aligns wrapped replay extents, clamps offsets on resize,
documents controls and permits Markdown in both execution-scope prompts.
It does **not** add structured storage, Markdown history rendering, per-region
folding, universal Markdown support or session continuation.

Verification of that repair: 215 agent-runner library tests passed, including
boundary keys, preserved arrow/page/mouse navigation, both themes, empty/short
views and long-history tail reachability after resize. Strict all-target
Clippy, changed-file rustfmt checks and patch whitespace checks passed.
A clean-slate observer produced a scoped implementation-record supplement;
a separate source-blind comparison found no concrete discrepancy and requested
additional preservation/resize evidence, now covered by explicit regressions.
Independent security audit and final compliance comparison remain queued;
none of this constitutes component acceptance.

## Markdown versus YAML versus JSON

Markdown is the right presentation and message-body format. It is readable in a
normal editor, widely understood, portable and independent of a colour palette.
Headings, paragraphs, lists, tables, links, reference links, quotes and fenced
code already convey useful semantics.

It is not a sufficient trusted session envelope. An assistant can emit
`## User`, `---`, a `thinking` fence or a blockquote as ordinary answer text.
Those constructs cannot reliably establish the speaker, distinguish a final
answer from an intermediate tool-request turn, or bind a tool result to a call.

| Option | Pros | Cons | Recommendation |
|--------|------|------|----------------|
| One unconstrained `.md` file | Best standalone reading and external viewer support | Ambiguous message boundaries and reasoning/tool identity; poor crash recovery and indexing | Export only, not executable session state |
| Markdown plus embedded special markers | One file, visible prose | Requires escaping, a versioned envelope parser and careful treatment of fake markers; unrestricted bodies complicate reliable delimiting | Possible later interchange format, unnecessary initial complexity |
| YAML with `body: \|` scalars | Typed roles with readable multiline Markdown in one file | Indentation and YAML escaping rules; whole-document rewrite costs and partial-write recovery; less convenient in Markdown viewers | Viable alternative for small finalized exports |
| YAML manifest plus `.md` bodies | Explicit roles, original Markdown, independent parsing and theme selection; lazy loading | More files and commit-order/recovery rules | Preferred canonical conversation record |
| JSON/NDJSON plus text export | Excellent appendable event framing and typed validation | Escaped multiline bodies are unpleasant in an editor | Keep for the existing operational journal, not the human-facing transcript |

JSON is not technically incapable of multiline text: strings encode newlines.
The objection is readability, not correctness. There is no reason to replace
the existing hash-oriented operational journal merely to improve Markdown
display. Keep journal and conversation records purpose-specific and avoid
duplicating authoritative body text.

### Proposed storage layout

This is an example of the **proposed format**, not a supported current interface:

```text
session-2026-10-06T16-00-00Z-example/
  session.yaml
  messages/
    000001.md
    000002.md
  attachments/
    000003.txt
```

```yaml
schema_version: 1
kind: agent-runner-conversation
session_id: session-2026-10-06T16-00-00Z-example
canonical_evidence: false
status: completed
markdown_profile: commonmark-gfm-subset-v1
parent_session: null
entries:
  - id: message-000001
    sequence: 1
    prompt: 1
    kind: user
    media_type: text/markdown
    path: messages/000001.md
    bytes: 31
    complete: true
    truncated: false
  - id: message-000002
    sequence: 2
    prompt: 1
    kind: assistant
    disposition: final
    media_type: text/markdown
    path: messages/000002.md
    complete: true
    truncated: false
```

The example omits fields whose exact names and semantics need contract design:
body digests, provider/model provenance, timestamps, tool-call linkage, outcome
status and validated optional source-session references. Define required fields,
enum values, unknown-field handling and a native schema before implementation.
Use a safe YAML subset without custom tags, aliases or arbitrary object loading.
Digests detect inconsistency; they do not authenticate an agent-writable record.

User/assistant prose remains Markdown. Provider-emitted reasoning gets its own
typed entry, only if actually supplied and recording is enabled. Do not ask the
model to fabricate or disclose internal reasoning. Tool results are literal
text or explicit binary summaries, not automatically interpreted as Markdown;
trusted host-generated tool summaries can be separate Markdown entries.

Write bodies first to private same-directory temporary files; sync and publish
them before atomically replacing the manifest that references them. A crash
must not make a partial assistant answer appear complete. Treat an unpublished
body as an orphan, not an accepted turn; interrupted streaming staging files
must be visibly partial. Preserve originals when diagnosing inconsistencies.
Session status and completion need their own durable ordering.

Do not reopen and rewrite the entire manifest for every token. Persist at
bounded checkpoints/message completion; keep staging and the operational
journal separate. A future paged manifest/index is warranted only if measured
large-session costs require it.

### Human-readable Markdown export

Generate `session.md` on explicit export, without making it a second authority.
Use a session heading and `## Prompt N` / `### User` / `### Assistant` headings.
Horizontal rules are useful visual separation, not framing. Normal agent
headings remain untouched within each message.

Use `<details><summary>Provider reasoning</summary>...</details>` for optional
GitHub-friendly export folding, and language-tagged fences for code. A blockquote
is also a readable presentation choice for reasoning, but is not its identity.
An artificial `thinking` fence treats prose as code and should not be the
canonical representation. Plain Markdown/CommonMark has no standard interactive
folding semantics; HTML details is a viewer-dependent export affordance.

Escaping and rendering must keep exported agent content from terminating
host-generated containers. Choose fence lengths greater than any matching run
in literal content. An external Markdown viewer sees presentation, not trusted
speaker identity. Reference-link definitions and footnotes are message-scoped;
export must namespace them or explicitly delimit their scope to prevent
cross-message collisions.

## Rendering profile and crate choices

Define a tested **terminal Markdown profile**, not "all Markdown". GFM is a
named dialect; GitHub website conveniences such as issue autolinks, alerts and
footnotes must be evaluated separately from the formal GFM specification.
`pulldown-cmark` extension flags do not themselves prove complete GFM support.

| Capability | Proposed treatment |
|------------|--------------------|
| CommonMark prose, headings, emphasis, lists, quotes, rules, links and reference links | Baseline; test nesting, escaping, hard/soft breaks and reference scope |
| Fenced/indented code | Language-aware highlighting, literal copy, stable folding identity and source-preserving wrap |
| GFM tables, task lists and strikethrough | Baseline extensions; width-aware wrapping without losing cells or styling |
| GFM extended autolinks and tag filtering | Explicit conformance fixtures; do not claim these from current flags |
| Footnotes and GitHub alerts | Next explicit extensions; keyboard navigation to references and safe fallback |
| Images | Alt text and destination, no automatic download or terminal graphics |
| HTML | Literal safe fallback; no scripts, embedded browser or arbitrary HTML execution |
| Math, Mermaid and other diagrams | Preserve source with labelled fallback initially, not an external executable renderer |
| Unknown extensions/languages | Readable source fallback, no silent content omission |

Existing current support is narrower than this target. Link interaction and
footnote navigation are not implied by styled text. A full terminal viewer can
be first-class without pretending to be a web browser.

### Existing projects

Verified upstream documentation is linked below; upstream `main` is not proof
that every documented feature exists in a published compatible release.

| Project | Verified strengths | Fit and limitations |
|---------|--------------------|---------------------|
| [pulldown-cmark][pulldown] | Rust CommonMark pull parser, extension flags and source-offset event iterator | Already installed; parsing only. Pull iteration over a supplied string is not appendable token-stream parsing |
| [syntect][syntect] | Syntax definitions and highlighting themes | Already installed; choose dark/light syntax palettes and rebuild highlights from source |
| [tui-markdown][tui-markdown] | Ratatui Text conversion; upstream documents style sheets, code themes, tables, footnotes, alerts and literal HTML/image fallbacks | Strongest renderer candidate; README calls it an experimental proof of concept. Validate published API, Ratatui type compatibility, narrow layout, source mapping and feature fixtures |
| [Termimad][termimad] | Rust/crossterm skins, wrapping, balanced tables and scrolling | README explicitly disclaims generic/complete Markdown; documented feature table lacks syntax colouring, ordered lists and links. Poor fit for the requested profile and existing Ratatui architecture |
| [Glow][glow] / [Glamour][glamour] | Attractive terminal Markdown viewer and stylesheet-based renderer, dark/light and custom styling | Go ecosystem; useful optional viewer for exported `.md`, not an embedded Rust session viewer or checkpoint format |
| [Comrak][comrak] | Rust parser with explicit CommonMark/GFM compatibility claims and many extensions | Useful parser alternative if strict GFM fixtures justify it; not a TUI viewer, and an AST still needs layout, themes and interaction |

**Decision recommendation:** keep the current dependency set for the first
semantic-viewer iteration. Run a bounded `tui-markdown` spike, then either adopt
it as the rendering adapter or retain our adapter with a documented reason.
Prefer reuse where it meets the contract; neither choice warrants writing a
custom CommonMark parser. Check exact release, license, dependency graph,
maintenance and offline-vendoring impact before adding anything.

The reviewed documentation does not establish an end-to-end API that combines
appendable Markdown input, stable region IDs, per-region folds, theme
invalidation and virtualized historical paging. That gap does **not** mandate a
new parser. It requires a small application-owned document/view-state layer.

## Streaming and re-theming

Markdown cannot always be finalized when the next newline arrives. Later input
can change a table header, a setext heading, an unclosed fence, a list or a link
whose reference definition appears much later. Parsing each network fragment
independently gives incorrect results.

Keep raw message source, stable message IDs and source-range-derived block
identity. Coalesce deltas on the existing UI cadence. Parse/render a bounded
active message or invalidated range, and show incomplete content provisionally
with literal-safe fallback. On completion, parse the complete message and
reconcile its blocks. Do not freeze an apparently closed paragraph if later
reference definitions can change it. Test every chunk boundary of fixtures,
including UTF-8 handling by the transport adapter.

Start with bounded active-message reparsing rather than claiming a perfect
incremental parser. Measure CPU, allocation and first-visible-content latency.
Avoid whole-session reparsing and highlighting on each token. If large active
messages exceed limits, report it visibly and apply a documented paging/staging
policy rather than silently dropping the tail.

Theme selection is view state. Cache rendered output by content revision,
Markdown profile, width, theme/syntax palette and fold state. Changing any of
these invalidates the appropriate cache. Re-render Markdown semantic styles
and syntax highlighting from original source; do not recolour already-coloured
terminal escape sequences. Raw persisted bodies contain no application-added
ANSI styles. Escape untrusted terminal controls before display while keeping
source provenance and literal-copy behavior explicit.

## Cursor, folding and very long histories

Use a logical focus cursor rather than the prompt editor's text cursor.
Keep focus identity and viewport scroll distinct. Focus targets are messages,
foldable code/reasoning/tool regions, links and references. Width changes and
theme changes must not lose focus or expansion state.

Proposed history controls: Up/Down or j/k scroll; Tab/Shift+Tab move between
interactive regions; Enter/Space toggle the focused fold; Left collapses and
Right expands; Ctrl+Home/End jump to the first/last viewport; Esc returns.
Reserve link activation for a separate explicit action so a fold key never
opens a URL. Live viewing needs an explicit focus mode before intercepting keys
that currently belong to the prompt editor. Approve and document that mode
before implementing it.

Default **history** to provider reasoning and verbose tool output collapsed;
show code in a short preview with a collapsed remainder. Keep prompts, final
answers, failures and cancellation/truncation warnings visible. Provide an
"expand all/collapse details" action and remember local view state without
rewriting the transcript. Whether *all* code starts fully collapsed is a
human UX decision: it can hide the main answer when the answer is a patch.
Recommend a bounded preview rather than hiding every code block.

Search must find folded content and reveal the containing region. Copy/export
use original selected content, not placeholders. Fold summaries report type,
language where applicable, line count and partial/truncated status. Do not
count hidden rows in the visible scrollbar extent.

Long-history requirements need lazy body loading, cached wrapped-row extents,
stable source anchors and viewport rendering. Metadata listing must not load
all transcript bodies. Per-session limits should produce a visible unavailable
or partial entry, not silently remove the session. Account for Unicode display
cells, narrow terminals, resize, giant lines, tables, many tiny messages,
malformed records and concurrently growing sessions.

Proposed benchmark gates, to approve before work: on a declared release-build
Linux reference machine, list 1,000 indexed sessions within 250 ms; navigate a
100,000-line fixture within a 50 ms p95 input-to-frame budget after indexing;
add no more than 128 MiB peak RSS for a 20 MiB fixture; preserve every retained
tail character. Separate cold indexing from warm navigation measurements.
These are proposed targets, not current measurements or guarantees.

## Continuation: context import, not action replay

Offer **Continue as a new session**, not "resume execution". Load selected
completed user prompts and validated final assistant answers as conversation
messages under a fresh current system prompt, current tool registry and current
authority. Record the source session and imported message IDs as provenance.
Do not replay tools, restore credentials, apply old policy or assume the
workspace still matches an old result.

Prompt plus final results is a sensible default. Exclude provider reasoning
and raw tool traffic. Let the person choose scope, show context size and
truncation, and obtain explicit confirmation before any model request. Keep the
original immutable. Content can be imported as historical user/assistant data,
never as system instructions or acceptance evidence.

This is intentionally lossy: omitted tool evidence may contain important file
paths or intermediate conclusions, and past answers may be wrong. Long sessions
need explicit selection or a labelled summary, not silent compaction at import.
Ask the new agent to revalidate referenced current files before acting.
An interrupted tool dispatch has unknown effects; omitting it does not undo it.
Display that warning before continuation. Do not import failed, provisional or
truncated answers as completed results.

Current `.log` labels do not reliably distinguish validated final answers from
all assistant turns, and `.jsonl` hashes do not recover missing text. Therefore
existing diagnostic history must not be silently converted into a trustworthy
continuation record. Explicit plain-text context attachment is a different,
untrusted and labelled operation. The pre-release no-migration policy means a
new record format need not recognize retired formats as resumable state.

## Impact, tradeoffs and SWOT

The largest impact is not adding a Markdown parser: it is replacing flat
display rows with stable typed messages/blocks, separating storage from view
state, and sharing the viewer between live and history. Recording needs durable
completion semantics; theming needs source-based invalidation; continuation
needs a separately approved context-import boundary.

| Dimension | Assessment |
|-----------|------------|
| Strengths | Editor-readable Markdown; palette-independent history; one live/history UX; reuse of installed parsers/highlighter; explicit speaker/provenance; searchable folds |
| Weaknesses | Manifest/body consistency and extra files; more state for focus/layout; Markdown reparsing can be costly; terminal tables/HTML have unavoidable limits |
| Opportunities | Readable export into Glow/GitHub/editor viewers; reusable child viewer; reference navigation; reproducible theme/width fixtures; explicit safe conversation continuation |
| Threats | Fake role markers and prompt injection; terminal-control injection and dangerous links; giant documents causing CPU/memory pressure; unsupported extensions losing content; misleading "resume" claims; orphaned or partial writes |

Pros over the current design are richer history, true re-theming and stable
interaction. Costs are a storage schema and a semantic view model. Against a
single `.md` file, the mixed format adds files but avoids relying on Markdown
syntax for trustworthy framing. Against an external pager, embedding costs
more work but preserves the live UI, keyboard ownership and confinement model.

Mitigate threats with typed host-owned framing, schema and byte limits,
non-link descriptor-relative reads/writes, bounded parsing, explicit
completeness, terminal-control escaping and no automatic link/image execution.
Private records remain potentially sensitive; keep no-log behavior and avoid
turning operational conversations into version-controlled canonical evidence.

## Actionable implementation plan

Every approved implementation bundle must first define requirements, consumer
contract, private design and an atomic task queue. Its queue orders
`write_tests`, `implement_code`, `security_audit`, `compliance_review`; the last
two are independent work, not implementer checkboxes. Request the planned
advisory intent-review opportunity when available, or record an explicit
exception; never invent a receipt.

| Step | Work and boundary | Acceptance evidence |
|------|-------------------|---------------------|
| 0: navigation repair | Current parent component; keys, shared scrollbar, matching wrapped extents, resize clamp and Markdown prompt guidance | Boundary, narrow/empty/short, >65,535-row and theme fixtures; no storage or execution change |
| 1: approve format and profile | Human decision on YAML manifest/Markdown bodies, supported dialect and folding defaults; define exact schema and ADR if accepted | Delimiter/spoofing, completeness, unknown-field and reference-scope test vectors; no acceptance claim from this proposal |
| 2: bounded dependency spike | Compare current adapter with a published `tui-markdown` release using the same fixtures; assess Comrak only if GFM gaps warrant it | Feature matrix, release/API/license and vendoring check, layout snapshots and performance results; explicit adopt/retain decision |
| 3: define child viewer | `agent_runner/transcript_view/` owns semantic parsing, source anchors, layout, theme invalidation and interaction; parent owns storage/execution | Child five-artifact lifecycle, parent consumer contract, pure typed inputs and display actions; no dependency on parent internals or provider process APIs |
| 4: structured private records | Parent recorder/history write YAML manifest plus Markdown/literal bodies; deterministic export | Crash-order tests, privacy/non-link checks, incomplete/truncated diagnostics, lazy metadata listing and no-log tests |
| 5: unify live/history rendering | Feed both paths through child viewer; choose syntax theme from active UI theme | Same-source/same-theme equivalence, light/dark syntax contrast, resize/copy preservation and unsupported-extension fallback |
| 6: streaming and folds | Provisional active message, completion reconciliation, logical focus, preview defaults, reveal-on-search | Chunk-boundary equivalence to complete-message rendering; no disappearing content; focus and expansion survive resize/theme changes |
| 7: scale history | Indexed extents, lazy/paged bodies, cached layout and viewport rendering | Approved cold/warm latency/RSS gates on large and adversarial fixtures, including 100,000 lines and 20 MiB sessions |
| 8: optional continuation | New-session context import with explicit selection/confirmation and source provenance | No effect replay, no old authority/system prompt, unknown-effect warnings, invalid/incomplete import rejection and visible context loss |

Do not roll all steps into one feature change. Steps 1-3 are design decisions;
4-7 deliver first-class history. Step 8 is separately optional and should follow
the record/viewer foundation.

## Sources

Repository evidence: `agent_runner/src/history.rs`,
`agent_runner/src/session_log.rs`, `agent_runner/src/markdown.rs`,
`agent_runner/src/tui/app.rs`, `agent_runner/src/tui/render.rs`,
`agent_runner/src/tui/theme.rs`, `agent_runner/src/run.rs` and
`agent_runner/Cargo.toml`. Product constraints: `VISION.md`,
`ARCHITECTURE.md`, `ROOT_CONTRACT.md` and the component's local intent.

External primary sources, consulted 2026-10-06:

- [GitHub Flavored Markdown specification][gfm]: dialect and website
  post-processing distinction.
- [pulldown-cmark README][pulldown]: pull-parser model, extensions and
  source-offset iteration.
- [tui-markdown README][tui-markdown] and
  [manifest][tui-manifest]: experimental status, style sheets, code themes,
  feature claims and dependencies.
- [Termimad README][termimad]: skins, scrolling and explicit completeness limits.
- [Glow README][glow] and [Glamour README][glamour]: terminal viewer and styles.
- [Comrak README][comrak]: parser conformance claims and extensions.
- [syntect documentation][syntect]: syntax/theme rendering API.

[gfm]: https://github.github.com/gfm/
[pulldown]: https://github.com/pulldown-cmark/pulldown-cmark
[tui-markdown]: https://github.com/joshka/tui-markdown/blob/main/tui-markdown/README.md
[tui-manifest]: https://github.com/joshka/tui-markdown/blob/main/tui-markdown/Cargo.toml
[termimad]: https://github.com/Canop/termimad
[glow]: https://github.com/charmbracelet/glow
[glamour]: https://github.com/charmbracelet/glamour
[comrak]: https://github.com/kivikakk/comrak
[syntect]: https://docs.rs/syntect/5.3.0/syntect/
