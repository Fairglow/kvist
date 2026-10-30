# Attention retention — narrow source-only observation

Observed 2026-10-30 by GitHub Copilot in a separate clean-slate observer context. Basis: only the specified source excerpts and optional publication helpers; no intent, prior records, history, execution output, commands, tests, or other agents. This is neither compliance nor security certification.

## Current behavior and limits

- **Identity:** the decision branch supplies `session_id:turn_index:decision_index`; the recorder combines that identity with the original call ID for filename generation. Reusing a call ID therefore does not alone identify the same proposal. Decisions are recorded sequentially; persistence errors propagate, and the branch returns without applying this turn's effects. [`maerg/src/agent.rs:1348–1382`, `1975–2008`]
- **Session allocation:** exclusive log creation tries the stem, then `_0001` through `_1023`. Existing paths are skipped, other errors propagate, and exhaustion fails rather than overwriting. This is uniqueness against existing log paths, not an unlimited or permanent identity reservation. [`maerg/src/agent.rs:1888–1917`]
- **Retention/publication:** the recorder redacts call ID, summary, rationale, and optional patch, then publishes JSON followed by its Markdown companion under `.kvist/authoring/proposals`. Markdown labels the proposal advisory, not human acceptance, and the draft unapplied. Each publication uses a same-directory staging file, writes and synchronizes its contents, then calls `persist_noclobber`. The recorder subsequently synchronizes the proposals directory on Unix; directory synchronization is a no-op on other platforms. [`maerg/src/agent.rs:1955–2025`; `maerg/src/file_io.rs:12–51`, `93–111`]
- **Definite correctness concern:** the pair is not transactional. JSON can remain published if Markdown publication fails; both can remain after a directory-sync failure. Errors propagate, but no rollback is shown. Repeating the same identity can then fail at the existing JSON instead of completing the companion. Neither pair-wide nor multi-decision all-or-nothing publication is established. [`maerg/src/agent.rs:1355–1381`, `1997–2007`]

## Unit assertions — inspected, not executed

`decisions_reusing_call_ids_retain_separate_evidence` uses one call ID with two identities. It asserts distinct JSON paths, preservation of the first JSON's bytes, and the respective summaries in both Markdown companions. Repeating the second identity with changed content must return an error and preserve the second JSON's bytes. It does not assert the error reason, Markdown byte preservation on collision, partial-publication recovery, or synchronization behavior. [`maerg/src/agent.rs:2027–2057`]

## Output-limit caveats

The attention header explicitly warns that its retained explanation may be output-limited and directs readers to raw Markdown proposal/review companions. It also labels evidence advisory and a retained-attempt snapshot, not current-state assurance. The proposal Markdown template has no output-limit warning, and the recorder shows no explicit size bound. These excerpts do not establish whether upstream evidence is complete. [`maerg/src/task_commands/attention.rs:62–86`; `maerg/src/agent.rs:1975–2025`]
