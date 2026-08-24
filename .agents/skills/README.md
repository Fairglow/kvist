# Agent Skills Directory

This directory contains **agent skills** — reusable, portable, self-contained components
that can be invoked by the `kvist` CLI.

## Architecture Overview

```
.agents/skills/
├── kvist-compliance/          ← Rust component (Phase 3 automation)
│   ├── SKILL.md              ← Human-readable documentation
│   └── component-design.rs   ← Rust implementation
├── kvist-architect/          ← Another Rust component
├── kvist-sqlite/            ← Another Rust component
└── markdown-skills/         ← Markdown-based skills
    ├── document-summarizer/SKILL.md
    └── text-classifier/SKILL.md
```

## Two Types of Skills

### 1. Markdown Skills (`.md` + `SKILL.md`)

**Purpose:** Simple, well-defined tasks that don't require Rust compilation.

**When to use:**
- Text processing (summarization, classification, extraction)
- Markdown manipulation (formatting, conversion, validation)
- Small utilities that don't need to integrate with Kvist's domain

**Structure:**
```
my-skill/
├── SKILL.md    ← Prompt + description + constraints
└── (optionally) context/  ← Supporting files
```

The `SKILL.md` file contains the full skill definition: description, inputs,
outputs, constraints, and the actual prompt template.

### 2. Rust Components (`.rs` + `SKILL.md`)

**Purpose:** Complex tasks that need to integrate with Kvist's CLI, access
domain types, or produce structured outputs.

**When to use:**
- Tasks that need to access Kvist's `Task`, `Queue`, `Component` types
- Tasks that produce structured JSON/TOML outputs
- Tasks that need to integrate with Kvist's sandbox or queue system
- Tasks that require compilation and distribution as binaries

**Structure:**
```
my-component/
├── SKILL.md      ← Human-readable documentation
└── component.rs  ← Rust implementation
```

The `.rs` file is compiled into a binary that can be invoked directly from the
CLI via `kvist skill <name> <args>`.

## Why Two Types?

| Aspect | Markdown Skills | Rust Components |
|--------|-----------------|-----------------|
| **Portability** | Pure text, runs anywhere | Compiled binary, platform-specific |
| **Integration** | No access to Kvist types | Full access to Kvist domain |
| **Output** | Text/markdown only | Structured data (JSON, TOML) |
| **Performance** | Interpreted, slower | Compiled, fast |
| **Deployment** | Copy the `.md` file | Build and install binary |
| **Complexity** | Low | High (full Rust project) |

## How Skills Are Invoked

### Markdown Skills

```bash
kvist skill document-summarizer /path/to/file.md
```

The CLI reads `SKILL.md`, parses it, and invokes the skill.

### Rust Components

```bash
kvist skill component-design --vision="build a web app" --contract="REST API"
```

The CLI compiles the Rust source and runs the resulting binary.

## Directory Naming

- **Dot-prefixed directories** (`.agents/skills/`) are **global** — they live
  outside the project and are meant to be shared across projects.
- **Subdirectories** like `kvist-compliance/` are **named by domain** — they
  are not hidden files, they are organized by skill domain.
- **Dot-prefixed files** (`.rs`, `.md`) are **unusual** but intentional:
  they signal "this is a skill artifact, not a normal project file."

## Project-Specific Skills

For skills that are specific to *this* project (rather than global), they go
into `.kvist/`:

```
.kvist/
├── components/
│   └── kvist-compliance/    ← Rust component for this project
│       ├── SKILL.md
│       └── component.rs
├── skills/
│   └── my-markdown-skill/   ← Markdown skill for this project
│       └── SKILL.md
└── SKILL.md                 ← Project-level skill
```

## Adding a New Skill

### Markdown Skill

1. Create a directory: `mkdir -p .agents/skills/my-skill`
2. Create `SKILL.md` with your prompt and description
3. (Optional) Add supporting files in a subdirectory
4. Build the project with `cargo build`
5. Run: `kvist skill my-skill <args>`

### Rust Component

1. Create a directory: `mkdir -p .agents/skills/my-component`
2. Create `SKILL.md` with documentation
3. Create `component.rs` with the implementation
4. Add dependencies to `Cargo.toml`
5. Build the project with `cargo build`
6. Run: `kvist skill my-component <args>`

## File Naming Conventions

- `SKILL.md` — Always uppercase, first file in directory
- `component.rs` or `skill.rs` — Implementation file
- Lowercase directory names: `kvist-compliance`, `markdown-summarizer`
- Dot-prefixed files: `.rs`, `.md` (signals "skill artifact")

## See Also

- [`SKILL.md`](../SKILL.md) — The meta-documentation for all skills
- [Phase 3 Backlog](../TODO.md#phase-3-independent-compliance-automation) —
  The skills being implemented in `kvist-compliance`
