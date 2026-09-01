# ADR 0006: Supervised attempts, recovery, and promotion

## Status

Proposed

## Context

The current executor treats successful agent exit as completion for several
task kinds and has no command that reconciles an interrupted prepared attempt.
With live writable mounts, a crash or misleading zero exit can leave source
effects that do not correspond to a valid queue transition. Unlocking cannot
determine what happened and automatic retry may repeat uncertain effects.

## Decision

Add an explicit supervised execution tier before enabling a production runner.
It always names one task, disables automatic retry, records a unique attempt
identity, and never marks the task completed solely because the agent exited
successfully. Kvist records the approved policy, pre-state, scoped changes,
runner result, and verification result. A separate human finalization command
accepts or blocks that exact attempt.

Every successful acceptance creates a canonical acceptance set containing the
exact accepted paths, pre- and post-digests, creations and deletions, queue and
evidence changes, review or attempt identity, expected VCS head, and bounded
default commit message. Acceptance commands offer an explicit `--commit`
option. It creates a local commit containing only that set; it never stages all
worktree changes, pushes, stashes, resets, cleans, or rewrites unrelated user
state.

Git commit creation uses an isolated index initialized from the expected head.
Kvist rejects overlapping staged, unstaged, renamed, deleted, or concurrently
changed paths, verifies the exact commit tree, and updates the branch only when
the head still matches. Repository hooks do not run by default because they are
untrusted effectful programs that may alter the accepted set. Signing follows
an explicit policy and never silently degrades from required to unsigned.
Jujutsu commit automation requires a separately designed and promoted backend.

Acceptance remains durable if commit creation fails. A versioned commit journal
records the pending operation, and a later `vcs commit-accepted` operation can
retry the exact set without repeating acceptance. One commit per acceptance is
the default; explicit later batching may combine only compatible,
non-overlapping pending sets.

Add explicit attempt recovery. The journal records pre- and intended
post-queue digests, policy and runner identities, approved write-scope digests,
and durable phases. Recovery may finalize an unambiguous durable transition or
record an attempt that provably ended before effects. Any other state remains
fenced for explicit human disposition; recovery does not claim to roll back or
approve source changes.

The first dogfooding pilot may use narrowly scoped live writes in supervised
mode. Unattended execution requires a later private bounded workspace,
deterministic change set, conflict-checked promotion, crash-recoverable
multi-file journal, and independently reviewed cleanup behavior. Git worktrees
or overlay filesystems may implement a backend but are not the portable
authority model.

## Alternatives considered

- **Complete on process exit:** fast but treats an untrusted process status as
  evidence that expected outcomes were achieved.
- **Require verification only:** stronger for implementation tasks, but tests
  and reviews can still succeed without producing the expected evidence.
- **Automatically reset with Git:** destructive, Git-specific, and unsafe with
  uncommitted or ignored user state.
- **Build transactional promotion before any pilot:** safest final model but
  delays validation of the runner and protocol. Supervised narrow live writes
  expose less authority while retaining human control.
- **Use `git add -A` or the user's index:** simple, but can capture unrelated
  work or disturb the user's staged state.
- **Roll back acceptance when commit fails:** falsely erases an approval that
  already occurred and makes recovery depend on VCS success.
- **Run Git hooks implicitly:** preserves common Git behavior but executes
  repository-controlled programs outside the approved task boundary.

## Consequences

Dogfooding begins as a supervised workflow rather than the target unattended
executor. It adds a human finalization step and explicit fenced recovery, but
does not misrepresent runner success as task compliance. Automatic execution
remains gated on transactional promotion and independent review.

Users can accept and commit one exact change set atomically from their
perspective, while commit failure remains explicit and recoverable. Unrelated
dirty and staged changes are preserved when they do not overlap. Git is the
first write-capable VCS backend; unsupported VCS commit backends fail clearly.
