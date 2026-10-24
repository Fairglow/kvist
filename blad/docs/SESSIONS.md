# Optional Kvist/Skott integration

This reference is for readers of Kvist/Skott transcript files, not standalone
Markdown viewing. Blad's published Cargo feature **`sessions`** is optional and
**disabled by default**, not a private feature. It adds file-format readers and
CLI commands; it does not introduce a runtime dependency on the Kvist or Skott
executables, a project service, credentials or a daemon. It neither creates nor
continues agent runs.

For ordinary documents, use the [standalone guide](USAGE.md).

## Install or build with the feature

The registry package is `kvist-blad`; the installed executable and imported
library remain `blad`. Rust **1.95 or newer** is required, not exactly 1.95.
Linux x86_64 is the verified executable target.

Version **0.3.0 is not yet published**. Install the current checkout from the
repository root:

```bash
cargo install --locked --path blad --features sessions
```

Inside `blad/`, use `cargo install --locked --path . --features sessions`.
For a repository build instead of installation:

```bash
cargo build --locked -p kvist-blad --features sessions
```

After the planned registry release is published:

```bash
cargo install --locked kvist-blad --version 0.3.0 --features sessions
```

Library consumers of that planned release can opt in explicitly:

```toml
[dependencies]
blad = { package = "kvist-blad", version = "0.3.0", features = ["sessions"] }
```

Without the feature, the session module, commands and session-only options are
absent. General file-viewing/rendering options remain available in both builds.

## Commands

```bash
blad list
blad list --limit 5
blad --log-dir /path/to/runs list

blad session 20261012T120000Z-12345
blad session 20261012T120000
blad session /path/to/runs/20261012T120000Z-12345
blad --dump session 20261012T120000

blad export 20261012T120000
blad export 20261012T120000 --output transcript.md
```

- **`list [--limit N]`** lists session IDs, model identifiers, manifest entry
  counts and completed/active status. The default limit is 20.
- **`session ID-OR-DIRECTORY`** reads a transcript by full ID, ID prefix or an
  existing session directory. It prints a rendered snapshot to stdout,
  **not the tabbed TUI**. Non-completed sessions get an explicit active/snapshot
  notice. Run the command again to read newer data; there is **no live follow**.
- **`export ID [--output FILE]`** combines a transcript into **source Markdown**,
  to stdout by default or to the named output. Export accepts an ID/prefix,
  not the directory-path route supported by `session`.
- **`--log-dir PATH`** selects the runs root (default `.skott/runs`, relative to
  the current working directory). It is global and can precede or follow a
  command. List `--limit` and export `--output` are command-specific.

General options such as `--theme`, `--theme-file`, `--color`, `--math`,
`--mermaid` and `--diagram-charset` apply to rendered snapshots. `--dump`
guarantees escape-free rendered snapshot text, including when `--color always`
is present. Source export is not a rendered dump: it retains Markdown math,
code, diagrams and authored controls regardless of rendering options. Treat
exported content as untrusted source, not sanitized terminal output.

Named export output is explicitly overwritten/truncated if it exists; it is
not no-clobber or an atomic-save protocol, and output symlinks are not protected
by the input no-follow policy. Choose a destination deliberately. All message
bodies load before snapshot/export output starts, so an input error does not
emit a successful partial transcript or truncate an export destination.
Rendering/output writer errors can still leave partial output.

Positional arguments are always filenames, never automatically detected IDs.
Use `blad FILE...` for ordinary documents. With this feature enabled, filenames
matching `list`, `session` or `export` go after `--`, for example `blad -- session`.
Do not combine file arguments with a session command. `show`, `view` and
`stream` are not commands or aliases.

## On-disk format

Blad reads the producer's structured run directory:

```text
.skott/runs/
└── 20261012T120000Z-12345/
    ├── session.json
    └── messages/
        ├── 000001.md
        └── 000002.md
```

A manifest contains metadata and ordered message entries, for example:

```json
{
  "schema_version": 1,
  "kind": "skott-conversation",
  "session_id": "20261012T120000Z-12345",
  "model": "example-model",
  "created_at": "2026-10-12T12:00:00Z",
  "status": "completed",
  "entries": [
    {"sequence": 1, "kind": "user", "path": "messages/000001.md"},
    {"sequence": 2, "kind": "assistant", "path": "messages/000002.md", "disposition": "final"}
  ]
}
```

Required manifest fields are `schema_version` (integer `1`), `kind`
(`"skott-conversation"`), nonempty strings `session_id` and `status` (neither
may contain only whitespace), and the required `entries` array (which may be empty).
`model` and `created_at` may be omitted or null;
when present they must be strings. Additional producer fields are allowed.
Only the exact status `"completed"` is completed; other nonempty statuses
display as active.

Each entry requires a positive integer `sequence` fitting the platform's
`usize`, a nonblank string `kind` (empty or whitespace-only strings are rejected),
and a string `path` resolving to the two
normal path components `messages/<filename>`. Absolute paths, `..` traversal
and deeper paths are rejected. `disposition` is an optional string (or null).
Additional entry fields are allowed. Missing or wrongly typed required fields,
invalid JSON and unsupported schema/kind are errors, not empty transcripts.

Message bodies are UTF-8 Markdown. Entries render in manifest order, not a
new sort by sequence. Known kinds display as User, Assistant, Thinking,
Tool Call, Tool Result or Notice; other kind labels remain visible.
`final`/`interim` dispositions annotate rendered message headers.
Export instead uses a `# Session Transcript` header, metadata, `##` message-kind
headings and horizontal separators, retaining the source bodies.

## Lookup, errors and filesystem boundary

`blad::session::{list_sessions, load_session, load_session_by_path, load_messages}`
return `std::io::Result`. CLI input failures report errors and exit unsuccessfully,
rather than becoming successful empty content.

- Listing a missing runs root succeeds with no sessions. Candidate directories
  without `session.json` are ignored. Malformed manifests, unreadable/nonregular
  artifacts and other read failures are errors, not silently skipped sessions.
- Listing sorts newest first by the tuple `(created_at string, session ID)`,
  descending. Missing timestamps sort after present timestamps; IDs break ties.
  Timestamp strings are compared lexically, not parsed into instants.
- ID lookup prefers an exact match, otherwise requires a unique nonempty prefix.
  Unknown IDs return `NotFound`; empty/ambiguous prefixes return `InvalidInput`.
  Lookup scans all candidate sessions, without a 1,000-session cutoff.
- Each manifest and message is a regular UTF-8 file, at most **16 MiB
  (16,777,216 bytes)** inclusively. Reads check opened size and actual bytes,
  with at most one extra probe byte. Oversized input is an error, not truncation.
- Session roots, candidate directories, manifests, message directories and
  message files reject final symlinks when opened. Descendant access uses
  no-follow, nonblocking opens relative to held directory descriptors;
  messages use the retained session directory and its `messages` descriptor,
  not arbitrary manifest-authored paths. Non-directory entries in the runs root
  are ignored, except symlinks, which are errors.

This integration is read-only with respect to its inputs, **not a sandbox**.
An explicitly selected root's ancestors are not isolated. Per-artifact byte
limits do not bound aggregate retained memory, render/search time or kernel/FUSE
I/O latency. Nonblocking opens are not hard I/O deadlines. Snapshot output is
an observation of files, not a transactionally consistent snapshot of concurrent
producer writes. There is no live-follow loop or automatic retry of a partially
written producer manifest.
