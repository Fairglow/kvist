# Offline dependencies: caches are not installed environments

Kvist acquires dependencies on the host, separately from network-denied
execution. “Zero-copy” is sensible when it means **reuse one native dependency
store without an extra Kvist package-tree copy**. It does not mean “never unpack
an archive, install a package, generate build files or write build output.”

## Language distinctions

| Backend | Current provisioned material | What an offline consumer actually uses | Sensible zero-copy boundary |
| --- | --- | --- | --- |
| Rust, native strategy | Operator-selected Cargo home, with native registry/Git data roots | Native Cargo data through isolated writable administrative/build scratch | Reuse native archives, expanded sources and Git objects; no implicit directory export |
| Python, pip/uv | `.kvist/vendored-python` wheelhouse and `.kvist/venv`; a uv lock is exported to `.kvist/uv-requirements.txt` | Installed venv site-packages for `python3 -m unittest -v` | Reuse wheel archives and native uv artifacts; installation still materializes the venv |
| Node, npm | Native npm cache directly at `.kvist/vendored-js`, plus project `node_modules` | Existing `node_modules` for `node --test` | Keep npm's native content-addressed cache; do not copy it to a second package tree |
| Node, yarn | Yarn cache directly at `.kvist/vendored-js`, plus project `node_modules` | Existing installed modules | Reuse the native cache; cache shape depends on the installed Yarn version |
| Node, pnpm | Direct project-native store at `.kvist/vendored-js`, plus installed modules | Existing installed modules | Reuse that store without a global-store copy; arbitrary existing global-store reuse is not added |
| C/C++, Conan | Native Conan home directly at `.kvist/vendored-conan`, plus `.kvist/conan-build` generator output | Already provisioned headers/libraries and generated CMake files | Mount the native package store without another copy; compiling and generating files still require writable output |
| Go | Operator-selected native `GOMODCACHE`: expanded modules and download metadata/archives | `go test -mod=readonly ./...`, with the module cache read-only and compiler scratch writable | Reuse native modules directly; `go mod download` acquires missing material without a source vendor copy |
| C/C++, vcpkg | Entire project-local vcpkg root at `.kvist/vendored-vcpkg`, including installed ports and the tool | Installed ports and native CMake toolchain | Keep the existing native root; port installation/building still materializes files |

Language detection prefers Rust, Go, Python, JavaScript, Conan, then vcpkg.
Within Python, `requirements.lock.txt` takes precedence over `uv.lock`.
Within JavaScript, pnpm takes precedence over yarn, then npm; preparation now
uses that same lock for the mount identity.

### Python: a wheelhouse is not the uv cache

Current provisioning exports `uv.lock` with `uv export --locked`, when selected,
then runs `pip download` into the wheelhouse. That export requires uv; installer
acceleration is optional for pip-locked projects. It creates a venv using uv when
available (otherwise stdlib `venv`) and installs the wheelhouse offline using
`uv pip install --no-index --find-links` when uv is available (otherwise venv
pip). Once uv is selected, its installation failure is reported explicitly,
not silently retried through a different installer.

The wheelhouse contains distribution archives: wheels and potentially source
distributions. It is not uv's native cache layout. uv also maintains native
download/build artifacts and may link or copy suitable cached files into an
environment, depending on its configured link mode and filesystem. Treating all
of those artifacts as a read-only Cargo-style source registry would be wrong.
The current installer can use its ordinary native cache; Kvist does not
provision a dedicated shared uv cache or expose it as an offline sandbox store.

Installation needs writable environment directories and usually writable cache
or scratch state. Source distributions may additionally need compatible build
backends, compilers and build dependencies; retaining an sdist alone does not
prove an offline build can succeed. Native extensions and the venv must match
the interpreter and platform used for verification.

Verification currently imports from the read-only mounted venv using a system
`python3` and `PYTHONPATH`. Bytecode and user-site writes are disabled. It does
not run `uv sync` or reinstall the environment inside the sandbox.

**Reuse today:** pip can retain already downloaded wheelhouse files and native
installer caches can retain artifacts; the existing venv is not recreated when
its interpreter already exists. Provisioning still invokes download/install.
Offline-first, missing-only wheel acquisition and exact environment sync are
not implemented by this preparation change.

### Node: native cache reuse does not eliminate `node_modules`

For npm, provisioning runs `npm ci --cache .kvist/vendored-js`.
This is npm's native cache, including its content-addressed `_cacache` storage
and administrative files, not a separate Kvist collection of copied source
directories or `npm pack` tarballs. npm owns cache integrity and the lock's
integrity information. There is no reason to add Cargo checksum files.

`npm ci` still removes/reconstructs the installed dependency tree. npm needs
writable `node_modules` and may need writable cache/log/temporary locations
even when package bytes are already cached and installation is offline.
A read-only npm cache alone is not a promise that `npm ci --offline` works.
Verification currently runs `node --test` against the installed tree; it does
not install packages from the mounted cache. Packages with install scripts or
native addons may require additional materialization/build tools.

Yarn provisioning runs `yarn install --frozen-lockfile --cache-folder ...`.
Classic Yarn's native cache may include unpacked packages and retained tarballs;
it should not be described universally as only tarball archives. Modern Yarn
layouts and PnP runtime behavior require version-specific verification.

pnpm already has a native content-addressable store and creates its installation
using links/copies and a virtual store. Provisioning now runs
`pnpm install --frozen-lockfile --store-dir .kvist/vendored-js` directly into the
persistent project-native store. It no longer recursively copies the global
store or follows that global tree's links. Warm project-store content is reused
natively; installation still materializes `node_modules`.

This does **not** implement zero-copy reuse of an arbitrary pre-existing global
pnpm store. Selecting/aliasing that store safely, while allowing required
installation writes without exposing host aliases, needs separate authority and
topology work. A second application checksum manifest is not the answer.

### Conan: the native home contains more than downloaded archives

Current provisioning uses a project-local `CONAN_HOME`, detects its default
profile if absent, and runs `conan install .` with an existing
`--lockfile=conanfile.lock` or a new `--lockfile-out=conanfile.lock`.
It requests `--build=missing`, `CMakeToolchain` and `CMakeDeps`, producing
`.kvist/conan-build`.

The supported layout is Conan 2's native home: package/recipe directories under
`p/`, cache database/configuration, sources and possibly build output. Acquired
archives are unpacked into native recipe/package artifacts. The short native
directory identifiers do not make the entire home a cryptographically
content-addressed archive store. Conan's own manifests and revisions are
package-manager metadata and must be retained; no Cargo checksum file is needed.
Recipe locks alone are not a complete attestation of every binary package:
profile, compiler/settings/options and package identity matter too.

Conan naturally reuses available recipes/binaries and builds missing binaries.
An offline `conan install --no-remote` can still need writable native cache,
database/locks, generator output and build state. Current verification instead
consumes already generated CMake files and packages, with the Conan home mounted
read-only at its canonical **host path** so embedded absolute paths remain
valid. The approved C/C++ test command must put build output in writable scratch;
it cannot assume the mounted home permits install/build operations.

Conan 1's different cache layout is not validated by the current `p/` readiness
check. Moving a checkout may require regenerating build files with its new
absolute cache path.

### Go: native module data, private compiler writes

Explicit `GOMODCACHE` must be absolute and dedicated to module data; otherwise
a bounded `go env GOMODCACHE` query selects it. Parent traversal is refused;
missing cache paths resolve existing ancestor aliases before authority checks.
Filesystem root, the host home
and project ancestors are not acceptable cache roots. `kvist vendor` now runs
bounded native `go mod download`, always rather than trusting an application
receipt. Go reuses already expanded modules/download artifacts and acquires
missing ones. `go.mod` and `go.sum` must remain byte-identical; a changed lock
fails with retained downloads and no claimed rollback.

The native catalogue checks declared requirements from `go mod edit -json`,
expanded module directories, cached `.mod`/`.info` metadata, native `ziphash`
matching the package's `go.sum` entry, and absence of a partial extraction
marker. This does not hash module source files or prove the entire transitive
graph. In particular, `go mod tidy` alone can leave native version `.info`
metadata missing; the explicit download preparation still has useful work.

Verification mounts the module cache at `/workspace/go-modules` read-only and
runs `go test -mod=readonly ./...`, with `GOPROXY=off`, `GOSUMDB=off`,
`GOTOOLCHAIN=local` and private writable `GOCACHE`/temporary/build scratch.
`GOMODCACHE` is `/workspace/go-modules`; writable `GOCACHE`, `GOTMPDIR` and
`GOPATH` are respectively `/workspace/scratch/gocache`,
`/workspace/scratch/tmp` and `/workspace/scratch/gopath`.
Package compilation is serialized (`GOFLAGS=-mod=readonly -p=1`) and
`GOMAXPROCS=2` keeps cold compiler work within the existing sandbox process
ceiling, without increasing that ceiling.
No host home/configuration/credentials are mounted. `GOWORK=off` prevents
implicit workspace-peer discovery. Local `replace` source layouts need a
separately approved resource topology and are currently refused; remote
replacements select their declared native identity.

An existing project `vendor/` tree is not refreshed or deleted. There is no
Go application file-checksum inventory or new cache receipt.
This isolated helper currently has fixed 30-second inspection, 900-second
acquisition and 1 MiB output limits; the common vendor-budget file does not yet
configure these Go limits.

### vcpkg: installed ports are useful materialization

The supported project-local root contains the vcpkg tool and installed ports;
the tool must already be bootstrapped. It is not merely a collection of
downloaded archives, nor a newly implemented shared binary-cache topology.
Installed headers/libraries and the CMake toolchain are the consumer artifacts.
Building/installing ports still requires writable native output and can require
download/build dependencies; an `installed/` directory alone does not prove
every requested port, target triplet or ABI is available offline.

The existing root is mounted read-only for verification. There is no new
vcpkg source copy, per-file inventory, arbitrary shared-store selection or
missing-only acquisition implementation in this change. Real vcpkg execution
was not validated by the native-manager checks described for the other tools.

## Current checks and recovery limits

Non-Rust readiness is deliberately coarse:

- Python checks for a nonempty wheelhouse and a venv interpreter.
- JavaScript checks for a nonempty native cache/store directory.
- Conan checks for a nonempty `p/` directory; vcpkg checks `installed/`.
- Go checks declared native module metadata/checksum presence; its combined
  identity covers `go.mod` and `go.sum`.

These checks do **not** validate every locked package, compare against an older
provisioned lock digest, or attest the installed environment. Cache logs alone
can satisfy JavaScript's coarse presence check. The lock digest identifies a
mount plan; it is not an independent checksum of the mounted package bytes.
Most non-Rust dependency counts are approximate; Go counts its checked declared
modules, not full transitive completeness evidence.

Native package managers supply their own integrity checks when acquiring or
installing material. Kvist does not generate additional non-Rust per-file hash
inventories, and this change removes none of their native metadata.

Non-Rust acquisition mutates its selected cache/installed environment in place,
without a non-Rust transactional publication receipt. Failed acquisition may leave useful
partial downloads or a partial installation. Keep those artifacts and rerun the
explicit host `kvist vendor` operation after correcting the cause; do not
blindly delete operator caches, lock files or interruption state.
Verification never falls back to a host build. Existing automatic provisioning
can run when a strategy returns a non-ready report; lock/read errors instead
fail explicitly, and this preparation repair does not grant new acquisition
authority. Native stores may reuse retained material on a retry, but Kvist
does not promise missing-only downloads for every backend.

This local change avoids rewriting identical generated Python, JavaScript and
Conan configuration, atomically repairs changed configuration, and refuses
linked/special configuration paths. It does not migrate dependency caches.
Independent source observation and compliance comparison remain separate;
this document and passing preparation tests are not compliance certification.

### Observed native-tool checks

Owned, isolated Linux fixtures exercised genuine package-manager artifacts:
repeated offline npm installs reused native content-cache material while
reconstructing `node_modules`; pip built a genuine wheel that uv installed
offline into a venv; Conan 2 reused an already installed locked binary package
and a CMake consumer linked it. Their installed consumers returned the fixture's
expected value.

Go downloaded a genuine UUID module into a dedicated native cache, completed
missing `.info` metadata, retained locked inputs on warm offline downloads and
passed actual network-denied sandbox tests with read-only module data and
writable compiler scratch. These observations do not certify other platforms,
Yarn versions, arbitrary global pnpm-store reuse or vcpkg execution.

## Rust-native cache architecture and proposed topology

**The Rust strategy integration is implemented in source; the broader coordinated
provisioning/execution topology is owned and verified by the shared Rust work.
The proposal below is not a claim of completed frontend/sandbox integration.**

Replace the duplicated unpacked default Cargo directory-vendor tree with the
operator-selected native Cargo dependency cache (`CARGO_HOME`, otherwise
`HOME/.cargo`). Reuse native registry index,
downloaded crate archives, unpacked registry sources and Git objects/checkouts
without manufacturing `.cargo-checksum.json` files for arbitrary directories.
Provisioning acquires only missing material under coordinated ownership.
The default Rust strategy returns only present native `registry` and `git`
roots at `/workspace/cargo-registry` and `/workspace/cargo-git`, not the entire
home or host configuration. It reads no legacy export receipt/fence or generated
directory-source configuration. Explicit directory export remains a separate
operator API, not the default portable/version-controlled registry design.
Runtime source selection uses the process-free `cache::sources` view. Canonical
catalogues can report presence; noncanonical source mapping needs actual
offline native metadata through the selected frontend admission helper.
Configured admission stays in the approved sandbox, without host fallback.
Recorded lock admission is not readiness, and direct context-free inspection
refuses to authorize noncanonical host admission.
One consumer limitation remains: generic automatic vendoring handles a
non-ready report, but propagates a typed missing-source admission error before
its acquisition branch. Partial noncanonical caches producing that error need
explicit operator `kvist vendor` preparation rather than a claimed automatic fix.

Proposed coordinated network-denied Cargo execution consumes shared material with isolated
writable Cargo administrative/build scratch. Cargo owns its native lock and
integrity semantics.

This proposal is not interchangeable with uv, npm, pnpm or Conan layouts.
Archive extraction and native environment/build materialization remain real
operations, and read-only cache access must account for each tool's locking and
write requirements. The Rust owner will update this section with the verified
paths, configured-acquisition behavior and supported offline topology.
