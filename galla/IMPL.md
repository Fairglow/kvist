<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

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
