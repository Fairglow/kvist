<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

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
