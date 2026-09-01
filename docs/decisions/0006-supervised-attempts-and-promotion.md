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

## Consequences

Dogfooding begins as a supervised workflow rather than the target unattended
executor. It adds a human finalization step and explicit fenced recovery, but
does not misrepresent runner success as task compliance. Automatic execution
remains gated on transactional promotion and independent review.
