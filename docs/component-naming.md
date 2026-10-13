# Component Naming

> Rationale and source of truth for the Kvist component names. Every component
> is named after a plant part, and each part is chosen because it mirrors the
> role that component plays in the system.

## Why a botanical theme?

The workspace originally had four components with interchangeable, self-describing
but forgettable names — `agent_runner`, `agent_runtime`, `engine`,
`sandbox_runner`. Those names told you what each component *does*, but not *which
one is which*.

The new names keep that one-to-one correspondence (each name still maps to exactly
one component) while giving each component an identity. The theme is a growing
plant: the workspace resolves to a single stem built from a **tip, a medium, a
core, a shell, and a leaf.**

## The naming convention

- One word, all lowercase.
- The name is a plant part (bud, sap, pith, gall, blade).
- The plant part is chosen to mirror the component's responsibility.
- The name is memorable rather than self-documenting, so this document is the
  source of truth and must be kept in sync with the code.

## The components

| Component (old → new) | Filesystem | Plant part | Role |
| --- | --- | --- | --- |
| `agent_runner` → **skott** | `agent_runner/` → `skott/` | bud / sprout / shoot | the active growth tip |
| `agent_runtime` → **sav** | `agent_runtime/` → `sav/` | sap / resin | the fluid runtime medium |
| `engine` → **maerg** | `engine/` → `maerg/` | pith / marrow | the vital core channel |
| `sandbox_runner` → **galla** | `sandbox_runner/` → `galla/` | gall | the protective quarantine shell |
| *(new) Markdown viewer → **blad*** | *(none yet) → `blad/` | blade / leaf | the visual surface you read |

## Each component

### skott — bud / sprout / shoot
- **Was:** `agent_runner`
- **Role:** the interactive/headless workspace agent — the part that actually
  executes and pushes new work out along the branch.
- **Why the name:** a shoot is the active growth tip, where the system does its
  moving. It consumes what `sav` carries and drives the rest of the stem to grow.

### sav — sap / resin
- **Was:** `agent_runtime`
- **Role:** the reusable, provider-neutral runtime the other components depend on —
  prompt acquisition, command rendering, process supervision, profiles, model
  transport.
- **Why the name:** sap is the fluid that carries activity, nutrition, and
  instructions through a living stem. Nothing would move without it flowing.

### maerg — pith / marrow
- **Was:** `engine`
- **Role:** project artifacts, discovery, validation, status, queues, policy, task
  lifecycle, evidence, and CLI dispatch — the coordinating core.
- **Why the name:** the pith is the innermost structural channel of a stem; it
  sustains and coordinates everything. This is the central piece the rest is
  organized around.

### galla — gall
- **Was:** `sandbox_runner`
- **Role:** the independently installed Linux enforcement boundary — strict request
  validation and Bubblewrap isolation for approved tasks.
- **Why the name:** a gall is an isolated, hardened protective capsule a plant forms
  around an encroaging organism so it cannot affect the rest of the tree. The
  sandbox runner is exactly that: a sealed, hardened boundary that quarantines
  untrusted execution so it can't reach the tree.

### blad — blade / leaf *(upcoming Markdown viewer)*
- **Will be:** the Markdown-viewer TUI (not yet created; name reserved).
- **Role:** the terminal UI for reading and navigating rendered Markdown.
- **Why the name:** a blade is the outer, formatted surface of a stem that you
  actually see and read — the right name for a viewer that turns structured content
  into something legible.

## Subdirectory-level renames

Not every rename is a whole component. A few are narrower — a single source
directory inside a component — and they live under that component's subtree:

| Old | New | Scope | Meaning |
| --- | --- | --- | --- |
| `engine/shell` → **bark** | subdirectory of `engine/` (now `maerg/`) | `engine/src/shell*` → `maerg/src/bark*` | durable, protective outer shell through which the outside world touches the tree |

- **Was:** `engine/shell` (the `shell.rs` module plus `shell/` subdirectory inside
  the `engine` component crate).
- **Now:** `bark` (`maerg/src/bark.rs` and `maerg/src/bark/`), the durable,
  protective outer shell through which the outside world touches the tree.
- **Why the name:** bark is the protective exterior of the stem — distinct from
  the pith/marrow core (`maerg`) it surrounds. It mirrors the shell's role as the
  boundary where commands, prompts, and the user meet the workspace.
- **Wiring:** `maerg/src/lib.rs` declares `pub mod bark;` and no `shell` module
  path remains anywhere in the crate. The rename moved files and updated every
  internal reference; remaining occurrences of the word "shell" in this crate are
  comments and user-facing strings (e.g. the `kvist shell` command), which are
  deliberately left as-is.

## Notes and tradeoffs

- **Stands out, but isn't self-documenting.** `agent_runner` told you its job;
  `skott` tells you nothing until you read this file. Keep this document updated as
  the source of truth.
- **Short names carry collision risk.** `sav` visually resembles "save", and short
  words are easy to mistype in CLI usage or scripts. Consider whether that's
  acceptable where these names surface.
- **Name vs. path vs. ID.** A component still maps to a directory, a crate, and (for
  the root) a stable ID. Decide which of each a rename touches so nothing is left
  inconsistent.
- **A deliberate touch:** `skott`, `galla`, and `blad` are genuine Swedish words for
  the plant parts they name — fitting for an author named Lindblad (blad = leaf,
  lind = linden/birch). This makes the set read as chosen rather than random.
