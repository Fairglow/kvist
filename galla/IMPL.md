<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

## Independent output-lock repair observations - 2026-11-08

Clean-slate `repair-source-observer` inspected current source and executable
assertions without intent, queues, prior records/reviews or history, running no
tests. Separate stdout/stderr threads read 4096-byte chunks with shared atomic
output accounting. Each acquires its host output lock after reading and releases
it before the next pipe read; neither holds that lock waiting for child output.
Boundary assertions require prompt timeout/output/process-limit failures and no
delayed descendant effects.

The observer retained a pre-existing out-of-scope limitation: host `write_all`
and `flush` can block on an undrained output sink, and unconditional forwarding
thread joins are not deadline-bounded (`src/enforcement.rs`). Killing the
payload does not unblock a blocked host write. Current boundary fixtures drain
captured output; they do not cover stalled host sinks. This scoped lock repair
does not establish a general nonblocking-output or whole-runner certification.

Separate source-blind `repair-intent-comparator` found chunk-scoped locking
consistent with the design, while retaining stalled-host-sink termination as
an unresolved broader limitation for human arbitration. It read intent and
fresh observations, not source. Implementer-attributed full-workspace nextest
`30302134-5ac4-45b4-8399-46f1963980d2` passed all 2727 executed tests with
20 skipped; affected strict all-target Clippy and formatting passed. The
real-runner denial now reaches the payload's explicit exit-73 path; finite
drained-output tests do not establish general output-sink deadlines.
Final all-feature workspace nextest
`92a39f59-3d6e-47bd-983e-566fcac6a64c` passed all 2748 executed tests with
20 skipped; final affected strict Clippy and formatting also passed.

## Scoped nesting regression repair - 2026-10-25

Independent clean-slate `nesting-source-observer` inspected only the current
marker-related source and executable tests, excluding intent, queues, prior
records, reviews and history. It ran no tests and made no compliance judgment.
The following observations are transcribed from its source-only report:

- `context.rs` uses `symlink_metadata` on `/.galla-sandbox`; any existing
  entry rejects, only `NotFound` permits continuation, and other inspection
  errors reject explicitly.
- Request mode checks before locking or reading stdin, parsing, validation or
  enforcement. Public `probe()` checks before backend discovery, hashing or
  namespace inspection. The CLI maps either rejection to status 3 with a
  diagnostic and no stdout. Public `enforcement::run()` retains its own guard.
  CLI argument-count/mode errors still return 2 before either mode runs.
- Enforcement constructs a mode-0555 read-only tmpfs marker after grants and
  vendor masking. Validation rejects grant destinations covering or below the
  marker. These source observations do not establish runtime mount behavior.
- The native conformance test runs the two modes in separate outer sandboxes.
  Each inner process has `env={}`, piped output and an open unwritten stdin
  pipe. It must exit 3 within two seconds, produce no stdout and emit both
  nesting-message substrings. Timeout kills/reaps the inner process.
  Unsuccessful outer runs collect mode-labelled diagnostics before one final
  assertion, so an ordinary probe failure cannot hide the request failure.
- The assertions exercise CLI subprocesses, not direct library rejection.
  Other metadata errors and every possible grant-overlap/error-precedence
  case are not directly exercised. The native fixture returns early if its
  own marker existence check reports a marked environment.

Implementer-attributed execution, separate from that source observation:
before the repair, the strengthened native test reported both probe status 0
and request stdin timeout. After restoring the early guards, all 80 default
Galla tests pass, with two existing opt-in inventory trials ignored. Strict
all-target, all-feature workspace Clippy and formatting pass. This is scoped
regression evidence, not component acceptance or runner-wide certification.

The separate source-blind `nesting-intent-comparator` compared this supplement
and attributed execution with local intent and the root contract, without
reading implementation/test source or running tests. It found no concrete
post-repair scoped discrepancy. Pre-repair probe success and request timeout
violated the nesting requirement. All-phase runtime marker immutability, full
diagnostic wording, direct-library rejection, metadata-error handling and
exhaustive grant/error precedence remain unverified by that comparison.
The native test's marked-environment early return remains a coverage limit.
Comparison completion is not acceptance or whole-runner certification.

## Scoped nesting observation - 2026-10-25

Independent clean-slate `source-observer` inspected only marker-related Galla
source/tests, excluding all intent, queues, prior records/reviews and history.
It ran no tests. A separate clean-slate `final-source-supplement` corrected its
initial incomplete test-source observation; assertions are not execution proof.
The selected observations below are transcribed from those source-only reports.

- The private marker is `/.galla-sandbox`. `require_host()` uses
  `symlink_metadata`: any existing entry rejects, including a dangling link;
  only `NotFound` permits execution. Other inspection errors reject explicitly.
- Request mode checks before reading stdin and exits 3 on rejection.
  Probe checks before backend discovery; the CLI maps unavailability to 3.
  The public enforcement entry point independently checks before backend lookup.
- Grant validation checks both containment directions against the reserved
  marker using slash-delimited path containment, not arbitrary string prefixes.
  After grants, enforcement adds `--perms 0555 --tmpfs /.galla-sandbox
  --remount-ro /.galla-sandbox` to Bubblewrap.
- The native conformance test grants the runner at `/tooling/galla-runner`
  and runs Python inside the outer sandbox. Its script loops over probe and
  request, uses `env={}` and `stdin=subprocess.PIPE`, and waits two seconds
  before writing/closing stdin. It asserts inner status 3, empty stdout and
  the "already inside" / "unnecessary" diagnostic; timeout kills/reaps the
  inner child, and Rust asserts outer success.
- The detector recognizes filesystem state, not process ancestry or marker
  contents. An unrelated host entry at that fixed pathname also rejects.
  Inspection permission failures are not directly exercised. This scoped
  observation does not assess or certify the rest of enforcement.

Final source SHA-256:

```text
6d0c394f2846a3813d8286b0cbaf0b2b01b974a45de16cd804260b387275a6e1  src/context.rs
8af33a76e052b2045e5527da93b294e0bd1a485396d94ac44daa1e3f670c7efc  src/main.rs
c68662e6bc8f1e29e1c4da53f377ec309283735e6ca6810b656eed2bdcc11e34  src/probe.rs
eedacfa2057e728fe53f5d2abda1e5987e4d09a5ceee94ce12a8db988f27e54f  src/enforcement.rs
63a364c580b2683afcc107d8af9ef400bec50f8d0614fae924a44b17d04c0309  src/validation.rs
f75de4433e679748db96d64412e3cba301448cf61e127a7943b9d95c0ceebfbc  tests/conformance.rs
```

### Independent comparison and execution evidence

The separate source-blind `intent-comparator` found no concrete scoped
no-nesting mismatch between current intent and independent observations.
It did not inspect implementation/test source or run tests. Its original
quiet-log input did not attribute the native conformance run; that is an
evidence-provenance limitation, not a demonstrated runtime failure.

The implementing context's subsequent labelled execution explicitly runs
`nested_sandbox_marker_destinations_are_reserved` and
`executable_executes_successfully_on_valid_request`, both passing. The latter
executes the source-observed inner probe/request checks with cleared
environment and open stdin under real Bubblewrap. All default Galla tests
also pass (80 executions, two opt-in inventory trials ignored), and strict
lint passes. This is bounded native evidence, not all-phase/mutation-schedule
proof or certification of the rest of enforcement.

Compared intent SHA-256:

```text
ba6a99fa4d8e8d3ded6260538342c6c78670314388678aac3338727de0260978  REQUIREMENTS.md
ac446c07f06f127e854760044b7c7595a8c512ab13a060451f4c9fa4961e47c7  CONTRACT.md
52c05e957b39f3ce1d09f4070d3f7ab344aa262c81c934bb1f4846b01450fda6  DESIGN.md
792841f99ed0e912336afe021947c1acabc3baf74ddf5c631750ccaab5baf240  source-blind-comparison.txt
ff97c832d95b635a9f6046e3146f50876ccc899e0caf8022db4b29ed62b4fd6d  blad-labelled-validation.log
```

The comparison and labelled log are session-local; the source observations and
execution scope are persisted above. No component acceptance or receipt is
implied, and earlier runner-wide review tasks remain untouched.

## Scoped portable native runtime observation - 2026-10-14

Independent clean-slate `portable-runtime-observer` inspected the native
toolchain fixture, workflow and relevant runner enforcement/validation source.
It excluded intent, queues, existing records, reviews and history. It executed
no tests; source-defined coverage is not independently observed execution.
A separate narrow clean-slate `runtime-guard-observer` inspected the final
mapping assertions and CI shell guards without intent or execution evidence.

The native fixture resolves host executable candidates and canonical paths.
System selection prioritizes `/usr/local/bin:/usr/bin:/bin`, then absolute
host PATH entries, rejecting candidates outside permitted System prefixes or
administrative paths. The inspected Go runtime must also be under System.
Python, Node, Go, GCC and G++ execute programs with exact output markers.
Host/sandbox canonical executable selection is compared. This uses substantial
read-only System trees, not only individual executable mounts; home/toolcache
language runtimes are not automatically granted.

Host rustup selection uses absolute PATH candidates with System fallbacks.
`RUSTUP_HOME` or `HOME/.rustup` locates the inventory. Canonical rustup maps
read-only to `/tooling/rustup`; generated rustup/Cargo/compiler/doc proxy links
resolve there. Generated settings and named registrations refer to individually
mounted complete roots at `/tooling/toolchains/<sorted-index>`, not host paths.
At least two installations and a non-host target are required. These are
System/authoring requests with network denied, not the closed Cargo phase.

The Cargo subprocess PATH supplied to rustup contains only private proxies and
the detected `cc`/`ld`/`as`/`ar` linker directory. Rustup's effective managed
child PATH is not separately asserted. Source-defined trials query versions,
target catalogues/components, compile every installed target as a library,
run native programs, and require unit-test and doctest summaries. A build script
launches compiler/doc queries. The final guard observer verified that canonical
`which` results for Cargo/rustc/rustdoc and compiler sysroots are asserted against
the same per-name mount destinations used by registration.

Snapshots compare modes, sizes, modification/change times, content hashes and
symlink targets across the host rustup home, separately located rustup executable
and external installation roots. External roots are first captured after
discovery. Access times, unrelated paths and transient restored changes are not
covered. Final snapshots run after exercise-block panics, not earlier setup
failures. Uninstall, target removal and generated-profile mutation must produce
read-only-filesystem errors. Tests do not provision host tools.

Diagnostics expose selected host path variables, candidates, mount mappings,
the fixture's configured environment, inside resolution, status/stdout/stderr
and denied mutations, not a whole ambient environment dump. These outputs are
unredacted and are not a generic secret-redaction facility. No concrete scoped
host-mutation or ambient-credential logging regression was identified; this is
not a full runner security audit or universal safety claim.

Stable and MSRV workflow jobs run ignored native trials serially with
`--nocapture` and retain separate diagnostic artifacts for seven days using
`always()`. The final guard observer verified explicit `shell: bash` for both
tee pipelines; failure propagation relies on GitHub Actions' Bash
`-e -o pipefail` invocation. Neither observer executed CI or claimed acceptance,
complete component compliance or remote success.

## Scoped filtered System observation - 2026-10-14

Independent clean-slate `bin-policy-observer` inspected only explicitly bounded
source/tests and workflow, excluding intent, queues, records, reviews and
history. It executed no tests and made no acceptance or compliance claim.

Administrative detection lexically normalizes paths and excludes `/sbin`,
`/usr/sbin`, `/usr/local/sbin` and descendants. Validation rejects
administrative PATH entries and grants covering or within those directories.
Only read-only Toolchain self-grants at exactly `/usr` or `/usr/local` receive
the filtered exemption; enforcement requires non-link directory sources and
skips ordinary creation/binding for them.

Filtered tmpfs views preserve other immediate children as read-only binds or
recreated links, without following administrative aliases. The administrative
destinations are read-only mode-000 tmpfs; payload capabilities are dropped.
Enumeration is sorted, nonrecursive and limited to 1024 immediate entries per
filtered directory, including excluded entries. Missing roots use namespace-only
views; linked roots, unsupported entries, inspection errors and non-UTF-8
names/targets fail explicitly, with runner exit code 3 on layout failure.
Unit fixtures cover alias preservation, host non-mutation, missing/linked roots
and both enumeration boundaries. The native conformance test exercises authoring
and verification without availability skips, asserting administrative access,
listing, creation and chmod denial, ordinary bin execution and preserved host
permissions.

The observer established no concrete high-confidence scoped regression.
Exclusion is path-based, not binary classification or race-resistant resolution.
Source-defined tests are not an observed execution result.

## Scoped workspace-vendor mask observation - 2026-10-14

Independent clean-slate `mask-observer` inspected only enumerated source/tests,
excluding intent, queues, records, reviews and history. It executed the two
`enforcement::tests::mask_` unit tests: 2 passed, 26 filtered out. It did not
execute native suites or make a compliance or acceptance claim.

`add_workspace_vendor_mask` activates only in authoring with read-only
Toolchain `/rust/vendor` and read-write Authoring `/workspace`. It inspects the
workspace's `.kvist` and `vendored` using `symlink_metadata`. Missing paths
leave arguments unchanged; links, non-directories and other inspection errors
return errors. Inspection failures produce runner exit code 3 before launch.
After all grant binds, the runner appends mode-000 tmpfs over
`/workspace/.kvist/vendored`, remounts it read-only and drops all payload
capabilities. It constructs arguments rather than changing host permissions.
The filesystem mask targets the vendor alias; capability dropping is
command-wide. Unit tests cover missing paths, nonmatching phases/grant
attributes, links/files and the appended argument sequence.

The observer identified no high-confidence new scoped regression. Inspection
is pathname-based, not descriptor-pinned; concurrent replacement between
inspection and mount execution is not excluded. Unit results alone do not
establish native mount behavior, effective capability removal or
permission-denied inspection handling.

## Observed component status

The Rust package builds a library and an executable named `galla-runner`.
The runner strictly parses and validates the version-one sandbox protocol and
enforces isolated execution via Bubblewrap.

## Observed command behavior

- `--kvist-sandbox-probe-v1`: Evaluates host kernel namespace availability (mnt, net,
  pid, ipc, uts, user) and verified `bwrap` executable path and SHA256 digest,
  emitting a confirmed `SandboxProbe` JSON response on standard output with status 0.
- `--kvist-sandbox-request-v1`: Reads bounded JSON requests from standard input,
  validates grants, bounds, and origin policies, and executes authoring, verification,
  or acquisition phases within isolated Bubblewrap namespaces under `prlimit` caps.
  Invalid requests fail closed with status 2; missing kernel/backend prerequisites exit
  with status 3.

## Implemented subsystems

- Version-one protocol request parsing, bounds checking, and strict schema validation (`src/protocol.rs`, `src/validation.rs`).
- Origin parsing, canonicalization, loopback classification, and allowlist matching (`src/origin.rs`).
- Descriptor-relative immutable Cargo-home generation staging and `RENAME_NOREPLACE` atomic publication (`src/cache.rs`).
- Host kernel namespace and Bubblewrap backend capability probe (`src/probe.rs`).
- Bubblewrap namespace isolation, `prlimit` resource capping, proxy mediation, and execution supervision (`src/enforcement.rs`).

## Scoped CI fixture observation - 2026-11-02

Independent clean-slate `ci-fixture-source-observer` read only enumerated
source, tests and configuration, excluding intent, queues, implementation
records, reviews and history. It ran no tests and made no compliance claim.

The system-language trial searches the sandbox request's configured
`/usr/local/bin:/usr/bin:/bin`, not host PATH. Selection requires an executable
file whose original and canonical paths remain under allowed System prefixes,
excluding administrative directories. The trial checks canonical Go GOROOT
and asserts sandbox resolution matches each inspected host executable
(`tests/toolchain_e2e.rs:13`, `:56-90`, `:294-339`).

PATH validation implicitly accepts `/bin`, `/usr/bin` and `/usr/local/bin`.
Other entries require a read-only grant destination; administrative entries
are rejected separately (`src/validation.rs:202-234`, `:847-867`). The ignored
trial runs Python/Node scripts, builds and executes Go/C/C++ programs and
asserts five output markers (`tests/toolchain_e2e.rs:293-395`). Both stable
and MSRV CI explicitly enable the native trials with `KVIST_GALLA`,
`--ignored` and serial execution (`../.github/workflows/rust.yml:134-139`,
`:253-258`). The system trial constructs an authoring request with denied
network and writable component/scratch grants; it adds no toolchain grant.

The observer also inspected the repository's `.config` ignore exception and
its temporary-repository Python regression. Those checks are repository
tooling, not runner authority. Source observations do not verify actual Git
tracking, sandbox enforcement or remote CI outcomes.

### Separately reported execution evidence

The implementing context reproduced the native CI failure before repair by
adding `/usr/local/.ghcup/bin:/usr/games:/usr/local/games` to host PATH:
validation rejected the request with `PATH must select only granted read-only
directories`. After repair, all four `toolchain_e2e` tests, including both
otherwise ignored native trials, passed with that same noisy host PATH on
stable and Rust 1.95.0. All 22 repository Python tests passed, including the
isolated ignore checks. These are local results, not a remote workflow rerun,
component acceptance or runner-wide certification.

The implementing context also reports 2,219 passing all-feature workspace
nextest tests with 15 existing skips, passing workspace doctests, four passing
otherwise ignored maerg native Rust sandbox tests, and passing stable strict
workspace lint and formatting.

### Independent source-blind comparison

Separate `ci-fixture-intent-comparator` read local requirements, contract and
design, this scoped observation and the two native execution logs, without
source access or test execution. It found no concrete scoped mismatch against
SR-REQ-TOOLCHAIN-E2E or the related PATH/authority obligations. The local logs
show four passing tests each, but noisy PATH and compiler attribution are
separately reported rather than visible in those quiet logs. This is neither
remote CI verification nor component acceptance.

## One-way/live-resource implementation observation — 2026-11-07

Independent `source-only-live-records` observed current source/tests, Cargo
manifest and native schema without intent, queues, old repository IMPL, docs,
reviews, chat/session checkpoints or Git history/diffs. Its follow-up reread only
`enforcement/host_code.rs` and its private tests. The following publishes scoped
extracts of those observations, not an implementer compliance determination.
The observer ran no tests or builds.

### Observed implementation

- Request/probe entry points validate bounded version-one input and report
  validation/enforcement failures explicitly; there is no host fallback.
  Authoring denies networking but accepts canonical payload PATH and loader
  settings. Closed Cargo phases retain exact argv/environment/grant rules.
- `enforcement/source.rs::{pin,walk,ordinary}` walks absolute paths with
  `O_PATH|O_NOFOLLOW`, retaining ordinary inode descriptors. Bubblewrap consumes
  transferred bind descriptors before payload execution. Path replacement
  cannot redirect retained origins; mounted contents remain live, not copied,
  leased or content-frozen.
- System layout exposes existing usr/lib/bin/sbin trees read-only, preserves
  links and drops capabilities. Administrative names are not categorical bans.
  The exact Authoring workspace vendor overlay uses pinned-relative traversal,
  is readable/nonwritable and independent of Rust readiness; it skips linked,
  absent or nontraversable local nodes without creating material.
- `host_code.rs::protect` retains the running image through `/proc/self/exe`,
  separately protects installed replacements, backend/prlimit, native loader
  dependency closure, executable mappings and loader inputs. `Guard::check`
  checks writable containment and direct-file device/inode equality. Guard
  descriptors remain CLOEXEC; request environment reaches isolated payloads,
  not unconfined launcher commands.
- `Guard::selector_slots` inspects known origin ancestors, canonical parent
  slots and encountered link-target ancestors, including concealed writable
  selector slots whose final library is outside the grant. Both visited sets
  are bounded to 16,384 entries. Read-only-only checks skip the walk; missing
  entries retain named slots, while unexpected errors/budget overflow diagnose
  repair or scope narrowing. No arbitrary writable-tree inventory is performed.
- ELF/trace capture bounds metadata, maps/stdout to 1 MiB, stderr to 64 KiB and
  closure to 512 paths, with a three-second supervised trace deadline and owned
  group cleanup. Unsupported/incomplete traces fail explicitly.
- Per-process NOFILE and optional FSIZE are inherited before payload startup;
  there is no host-file polling/deletion or UID-wide NPROC. Private tmpfs RAM
  budgets and streamed output limits remain. Descendant/group thread accounting
  is sampled, not a hard kernel admission ceiling.
- The separate descriptor-relative cache primitive verifies/copies/hashes,
  fsyncs and publishes with `RENAME_NOREPLACE`, reporting post-publication sync
  uncertainty honestly. Wire-request promotion instead refuses absent trusted
  generation-parent capability after successful execution; attempt files
  remain and no sibling destination is inferred or written.

### Evidence and limitations

Source assertions cover retained source origins, live admin layout, native
dependencies, direct hardlinks, deleted/replaced images, descriptor closure,
bounded/stalled traces, concealed ancestor aliases, independent writes and
operator content/selector updates. These are finite fixtures, not adversarial
concurrent launcher transactions or exhaustive host alias inventories.

Observed limits remain: selector checks use synchronous pathname operations,
not retained selector capabilities or an atomic topology snapshot; host trace
and later launch remain pathname-based. Reader joins ultimately use blocking
I/O/waits. CLI validation is not repeated by public `enforcement::run`, and
identity fields do not all attest executed inputs. No active Cargo-home cache
quota was observed. Package-source proxying is environment-based, not mandatory
socket mediation; setup failure, CONNECT/DNS and blocking proxy behavior remain
limitations. The native schema purpose enum does not cover every parser variant.
These observations do not establish closed acquisition/network conformance.

Parent-attributed final execution: Galla+Sav+Skott 1046/1046 passed
`d98adaee-bb82-463d-849b-a366a791ee8a`; explicit native isolation passed
`42ed4b28-e551-48a6-8a73-816bab9bfe25`, and exclusive checkout passed
`c901e06d-9fe5-4523-90da-50145bd8433b`. Strict affected Clippy, workspace
all-target check, formatting and matching release builds passed. The independent
security reviewer separately marked its four original findings and related
ancestor-selector finding fixed; final selector review confidence was 9/10.
Neither attributed execution nor review changes accepted revisions or certifies
the whole runner.

### Separate source-blind comparison

`source-blind-live-comparison` read current local intent, relevant global
intent/ADR and fresh independent records, without source/tests, old repository
IMPL, queues, Git/history or previous reviews. It ran nothing. It found scoped
correspondence for live read-only views, optional FSIZE, no operator-file
polling/deletion, explicit pinned origins/closed descriptors, bounded host-code
and selector protection, and refusal to infer cache publication authority.
Its finding about superseded mode-000 prose was retained chronologically and
resolved by a narrow policy-directed documentation cleanup/follow-up, not a
new implementation or acceptance.

Known-origin/direct-file protection is not an exhaustive directory-alias/IPC
inventory or atomic launcher transaction. Existing package-network, cache-quota,
output-join and schema limitations remain outside whole-runner certification.
The separate Skott lost-update qualification and external-linked snapshot gaps
remain for explicit human arbitration; Galla evidence does not certify Skott.
Attributed final executions remain distinct from independent observation.
Accepted revision/revalidation metadata is unchanged.

## Native cache and transport observation (2026-11-07 UTC)

Independent source-only observer `d38462f8-d2f9-4d78-a5bb-c512f6458890`
derived the Galla section of
[`native-cache-observation.md`](../docs/reviews/native-cache-observation.md).
The observer excluded intent, queues, prior records/reviews and history and
executed no checks.

Observed native requests separate read-only registry/Git endpoints from private
administration; acquisition alone grants data writes. Commands, environments,
aliases and grant purposes are closed. The proxy preserves client-first bytes,
full-duplex traffic and half-closes with bounded admission and deadlines.
HTTPS CONNECT authorizes authority, not encrypted paths or Git revisions.
Uninterruptible DNS, unbounded lock-digest streaming inside its helper and absent
native aggregate cache/scratch quotas remain explicit observation limits.
These observations do not certify package-network confinement or acceptance.

Source-blind comparer `a6add327-2a55-4006-b3f6-b2750041ff26` recorded scoped
native/transport correspondence and evidence limits in
[`native-cache-comparison.md`](../docs/reviews/native-cache-comparison.md),
without source/test-body access. Legacy promotion/resource failures are not
converted into passing evidence or global certification.

### Hermetic resolver follow-up

Fresh source-only observer `721f6eeb-1581-42b9-afd0-6eeadbe74b50` derived the
resolver supplement in the observation document. Production selects native
lookup; the test injects named resolution and uses shared normalization,
bounded queue and pinned relay. The observer executed no checks.

Implementer-attributed native execution now passes all 22 transport tests and
all 25 Skott Rust-environment cases, including the previously failing nested
repository trial. Its cause was ambient DNS fixture dependence, not native
cache acquisition or concurrency. No workspace DNS grants, increased production
limits or skipped assertions were introduced. The DNS diagnostic regression
also reproduced lost native cause before the fix and passes afterwards.

Security reviewer `bf1b95b7-8a98-4777-b332-d8740de4e31f` inspected the
Git-index fixture's local bootstrap/protected-request separation and reported no
additional security finding. Its missing fixture-subprocess deadlines remain
unqualified; neither that inspection nor the real trial certifies general
cleanup/deadline behavior.
