# Skott tool ergonomics: source-blind comparison

Date: 2026-10-27. Scope: prompt-local read cursors, append and single-file
patching. This is a comparison of intent with recorded observations and finite
implementer execution evidence, not implementation inspection or certification.

## Exact inputs read

- `skott/REQUIREMENTS.md:650-725`, focusing on `RUN-REQ-TOOL-ERGONOMICS`.
- `skott/CONTRACT.md:501-588`, native file-tool inputs and results.
- `skott/DESIGN.md:454-531`, tool rendering.
- `ROOT_CONTRACT.md`, complete.
- `skott/IMPL.md:1-91`, only the new “Prompt-local read continuation and
  incremental tools” section, including its ancillary observations; nothing
  under “Repository scheduling”.
- `/home/stefan/.copilot/session-state/14ec4c14-df23-4135-9a4e-dc724ca734f7/files/tool-validation.log`,
  complete.

No implementation/test source, Git history/diffs, queues, earlier implementation
records or reviews were read. No agents were called and no tests were executed
by this reviewer.

## Supported correspondence

The independent source-derived observations support fresh per-run, exact-path
cursors; actual validated byte-boundary advancement; limit changes without
reset; explicit-offset precedence; digest-bound continuation; failure without
advancement; and terminal reads entering the existing repeat detector. An
additional empty EOF execution is consistent with bounded correction rather
than immediate rejection. Mutations retaining an old cursor and requiring an
explicit restart are consistent with stale-continuation protection.

The observations also support existing-file, digest-bound, verbatim append;
path-free, ordered, exact-coordinate/count patching; CRLF/missing-newline
handling; pre-install validation; bounded results; shared no-follow atomic
replacement and mode preservation; and private sandbox helper/payload routing
without new authority. The stated external-writer limitation matches the
contract's non-transactional guarantee. Post-rename I/O failure is distinct
from validation failure and must not be interpreted as guaranteed rollback.

## Finite execution evidence

The log records six successful groups of 224, 40, 8, 46, 36 and 6 tests
(360 total), with no failures or ignored tests. These groups are unnamed in the
log; their suite attribution comes from IMPL. It separately records
`real_sandbox_confines_native_files_and_denies_host_network` passing once, with
two tests filtered out. IMPL reports that trial exercised append/patch,
confinement, inaccessible outside content and denied host-loopback network;
the log itself exposes the test name and result, not its assertions.

A final dev-profile build completed. The log does not identify that command as
strict all-target Clippy or show formatting/whitespace invocations. Those gates,
pre-change failing tests and the specific stale-recovery assertions remain
implementer-reported evidence. Running a test from `live_llama` is not evidence
of live-model qualification; IMPL explicitly says live-model tests were not run.

## Arbitration and evidence gaps

- **Underspecified no-op patch behavior:** the observation says no-change
  patches fail before installation. The contract does not state this additional
  rejection. Decide whether valid context-only/equivalent-replacement hunks
  should succeed or document their rejection; no intent was changed here.
- **Later evidence boundary:** result-bound/binary-preimage/stale-recovery
  regressions and wording changes postdate the independent observer's scope.
  Aggregate passes corroborate execution, not their detailed assertions or the
  claim that no production algorithm changed.
- **Coverage limits:** these inputs do not independently establish every
  required boundary case, cursor survival across compaction/steering,
  simultaneous supplied/stored digest checking, documentation of host
  provisioning, tests-before-production chronology, or a separate security
  audit. Obtain targeted evidence where required before acceptance.
- **Concrete ancillary conflicts, outside this extension:** the recorded
  unstaged long host-shell script conflicts with the contract's full-bound
  executability promise. The recorded new-write staging-name collision exposes
  a destination before failed validation, conflicting with atomic publication.
  Retain both for separate arbitration/remediation. Append/patch requiring
  existing targets explains their narrower exposure, not a waiver or resolution
  of these existing-code findings.

## Scoped conclusion

The recorded observations substantially correspond to the bounded extension's
intent; these inputs establish no concrete contradiction in its new
cursor/append/patch behavior. Resolve the no-op policy and evidence gaps
explicitly. Passing finite tests does not establish formal compliance, human
acceptance, blanket security, or closure of the ancillary findings.
