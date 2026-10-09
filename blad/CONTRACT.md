# Blad Contract

## Version

`blad.contracts/v1`

## Artifact Layout

```
.blad/sessions/{session-id}/
├── manifest.json
└── messages/
    ├── 000001.md
    ├── 000002.md
    └── ...
```

## Session Identifier

`{utc-timestamp}-{pid}-{counter}`

- `{utc-timestamp}`: Fixed-width UTC microsecond stamp
- `{pid}`: Process ID
- `{counter}`: Per-process sequence counter

Example: `20261009T120000Z-12345-0`

## Manifest Schema (`manifest.json`)

```json
{
    "schema_version": 1,
    "session_id": "string",
    "created_at": "RFC3339 timestamp",
    "status": "streaming | completed | cancelled | failed",
    "entry_count": 0,
    "entries": [
        {
            "id": "string",
            "sequence": 0,
            "kind": "user | assistant | tool_dispatch | tool_result | system",
            "disposition": null | "final | interim",
            "media_type": "text/markdown",
            "path": "messages/000001.md",
            "bytes": 0,
            "complete": true,
            "truncated": false
        }
    ]
}
```

## Message File Format

- Path: `messages/{sequence:06}.md`
- Content: Markdown text
- Encoding: UTF-8
- Line endings: Unix (LF)

## Continuation Context

When continuing a session, the new session receives:

```
Session continuation context:
- Original session: {session-id}
- Original path: {path}
- Original size: {token-count} tokens
- Imported: {user-prompt-count} user prompts, {assistant-answer-count} assistant answers
```

## CLI Commands

### `blad new`

```
blad new [OPTIONS]
    --model <id>          Model identifier to use
    --effort <level>      Thinking effort level
    --cwd <path>          Working directory for session
    --no-logs             Skip writing durable artifacts
```

### `blad continue`

```
blad continue <session-id> [OPTIONS]
    --model <id>          Model identifier to use
    --effort <level>      Thinking effort level
```

### `blad list`

```
blad list [--limit <n>]
```

### `blad show`

```
blad show <session-id> [--message <n>]
```

### `blad export`

```
blad export <session-id> [OUTPUT.md]
```
