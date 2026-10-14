<!-- kvist-implementation-record-version: 1 -->
# Component Implementation Record

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
