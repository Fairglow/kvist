# Artifact recovery

`kvist repair` repairs managed metadata without asking you to edit YAML, TOML,
or Markdown. It does not repair arbitrary source code or certify recovered
intent. Destructive recovery always requires approval of an exact proposal.

## Start with preservation

Run `kvist repair --dry-run`. Valid queues remain byte-for-byte unchanged.
Ordinary repair sorts/deduplicates invalid dependency and requirement lists and
stably orders otherwise-valid tasks so dependencies precede consumers.
Without `--dry-run`, those unambiguous repairs are applied with a backup.

For other queue errors or malformed Markdown structure, run:

```text
kvist repair --salvage
```

This proposes changes but writes nothing. It reports reconciled values,
discarded fields, removed tasks, and dependent-task consequences. Review the
proposal, then rerun the same options with `--approve` and the displayed
`sha256:...` token. Do not retain `--dry-run` when applying.

The token binds the project location, inspected originals and permissions,
backup location, replacements and reasons. A changed input invalidates approval.
One approved proposal never authorizes a different destructive repair.

## Recovery choices

| Problem | Available treatment |
|---|---|
| Unsorted/duplicate set lists; acyclic declaration order | Ordinary repair, with a backup. |
| Valid but noncanonical queue | Leave it untouched. |
| Valid timestamps in contradictory order | Salvage proposes moving update/completion forward, with exact old/new values and an explicit assumption. This reconciles metadata, not completion evidence. |
| Missing/invalid dates, invalid IDs, text bounds, requirement locators or task kinds | Keep independently valid tasks; propose quarantining invalid tasks and affected dependants. Missing completion dates are never invented. |
| Unknown task/top-level fields | Salvage can remove those fields while retaining valid task data; originals remain in the backup. |
| Identical duplicate task data | Propose merging into one task identity. |
| Conflicting duplicate identities | Do not choose an identity's meaning; propose removing conflicting entries and affected dependants. |
| Missing/self dependencies, cycles, missing lifecycle predecessors | Remove offending tasks and their dependent closure, rather than dropping edges or inventing tests/reviews. |
| Missing/contradictory status reasons | Preserve status; propose clearing an inapplicable reason or adding a truthful recovery blocker explaining the missing rationale. |
| Invalid component revision/revalidation metadata | Propose an explicitly stale, unaccepted draft. Salvaged queues require component acceptance before task selection. |
| Partially broken YAML | When the version-one header and same-indent task fragments parse independently, propose fragment salvage with an explicit segmentation assumption. Otherwise offer reset. |
| Unsupported schemas | Explicit reset, never silent migration. |
| Missing/malformed Markdown markers/headings, missing/duplicate/empty sections | Salvage preserves prose and adds labeled recovery-draft placeholders. Unsupported versions or ambiguous fenced-section rewrites require reset. |
| Malformed TOML, unknown configuration values, bad encoding, oversized artifacts or unrecoverable Markdown | Reset the named artifact to a native draft; review the original in the backup. |
| A directory, link or special object where an artifact belongs | Approved reset moves the object intact into the backup. Link targets are not followed or changed. |
| Obstructing component parent | Approved clean slate quarantines the obstructing object and creates real directories for drafts. |
| Corrupt/unresolved journals or execution evidence | Ordinary repair refuses to orphan evidence. Explicit clean slate archives it; prior effects remain unknown. |

Reset an individual artifact:

```text
kvist repair --reset maerg/TODOS.yaml
kvist repair --reset VISION.md --reset kvist.toml
```

`--reset` is repeatable and accepts only project-relative managed artifact
paths, never source, VCS, traversal, or backup paths. A queue with retained
workflow state requires clean slate instead of an isolated reset.
When resetting configuration, `--component-root PATH` chooses the replacement
component tree, for example `--reset kvist.toml --component-root src`.
Otherwise the reset configuration uses `.` to retain namespace components.

As the last resort:

```text
kvist repair --clean-slate
```

This proposes fresh root/component drafts and empty, stale queues, plus
quarantine of project-owned `.kvist` and `.kvist-attempts` workflow objects.
It preserves source and VCS files. Where `.kvist` itself contains a component,
its source is preserved and only known runtime manifest/legacy approval files
are archived. Global provider configuration and user-owned grants are not
silently modified. Fresh drafts grant no execution authority and are not a
reconstruction of stakeholder intent or independent implementation evidence.
Inspect retained effects before defining or replaying work.

## Backups and failures

Every application reports a private bundle under `.kvist-recovery/`, excluded
from component discovery. `originals/` contains exact file bytes with original
permissions, or intact quarantined objects. `manifest.json` records original
and replacement digests, operations, approval and progress. Regular originals
are copied, verified and synchronized before replacements begin.

If default storage is obstructed, choose unused private external storage:

```text
kvist repair --reset VISION.md --backup-dir /absolute/path/to/private-backups
```

Its parent must already be a real directory. Existing storage must be private;
recovery never follows backup links or overwrites conflicting storage.
Intact directory/link/special-object quarantine needs storage on the same
filesystem. `--backup-dir .kvist-recovery-alternative` provides a reserved,
discovery-excluded alternative inside the project. Each bundle also ignores its
contents in Git, reducing accidental staging of private originals.

Application is atomic per file, not across the whole project. After a partial
failure, the error names the retained bundle; the manifest and original objects
allow inspection of what happened. Request a new proposal for the remaining
damage rather than reusing an old approval or blindly replaying a task.
Progress is recorded after each mutation: an interruption or manifest-write
failure can leave the list behind actual changes. Inspect artifact bytes and
digests as well as the manifest before deciding what remains.

Configuration-independent inventory remains bounded: 64 levels, 10,000
directories and 10,000 entries per directory. Object snapshots allow 100,000
entries and 512 MiB; parsed artifact text is at most 1 MiB per file and 64 MiB
aggregate. Bound, permission, storage and inspection failures are explicit,
never a success report.
Directory exclusions in the project's `.gitignore` protect ignored scratch
worktrees from implicit recovery; the policy is included in approval binding.
Configured/selected component roots are still inspected under ignored
namespaces. A linked, non-UTF-8 or oversized `.gitignore` cannot safely define
recovery scope and is reported as an environmental blocker, not rewritten.

Live operations block changes. Stale or malformed operation locks are not
silently deleted: inspect the owning operation and use normal task recovery/
unlock when its prerequisites are valid. If old state cannot safely be
reconciled, `kvist init NEW_DIRECTORY` provides a separate clean project while
leaving the entire original project intact. No command can safely reconstruct
unreadable storage, missing meaning, or prove that unknown execution had no
effects.

Plain, JSON and interactive-shell repair use the same engine. Exit status is
nonzero while structural/discovery problems or unapplied proposals remain;
structural validity alone does not mean accepted, executable or compliant.
