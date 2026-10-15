# Private beta builds

The `Private beta packages` workflow builds versioned **Windows x64**, **macOS
Intel + Apple Silicon**, and **Linux x64** packages from the same committed source.
It runs on pushes to `main`, pull requests, and manual dispatch. It only uploads
artifacts to this private repository; it never creates a GitHub Release or changes
repository visibility.

## Packages and checks

| Package | Plugin formats | Automated execution |
| --- | --- | --- |
| `Lullaby-<version>-Windows-x86_64.zip` | VST3, CLAP | Windows Server 2022, x64 |
| `Lullaby-<version>-macOS-universal.zip` | VST3, CLAP, AUv2 | macOS 15 Apple Silicon; the **same archive** is then tested on macOS 15 Intel |
| `Lullaby-<version>-Linux-x86_64.zip` | VST3, CLAP | Ubuntu 24.04, x64 |

Every build runs default-feature workspace Rust tests (including editor/memory
guards), VST3 integration tests, optional offline DSP diagnostics tests scoped to
`lullaby-dsp`, packaging/licensing Python regression tests, and
a native CLAP loading/processing/state smoke test against the packaged binary.
The macOS build runs the AU smoke test and strict Apple `auval`. The Intel job
extracts the Apple Silicon job's universal archive with executable modes preserved,
checks both Mach-O slices and code signatures, and runs CLAP/AU smoke tests plus
`auval` natively on Intel. A final collection job requires all three packages and
all preceding checks to pass.

The final `Lullaby-private-beta-<commit>-<attempt>` artifact contains the three
ZIPs and combined `SHA256SUMS`. Download that artifact from the successful workflow
run's **Artifacts** section. Per-platform artifacts are intermediate candidates;
a failed workflow has not produced an accepted beta. Artifacts expire after 30
days, so retain an approved build and its checksums in controlled release storage.
Private workflow artifact access requires repository access; these are not public
customer download links.

Each distribution contains complete plugin bundles, installation instructions,
project/font/third-party notices, a dependency manifest, build provenance, and file
hashes. The packager rejects incomplete bundles, wrong binary architectures,
stale build receipts, mismatched source versions, and unexpected files. Its checker
reads binary headers and the actual ZIP contents rather than trusting filenames.
The package version comes from `Cargo.toml`; plugin IDs stay stable for session recall.

## Pinned inputs and reproducibility limits

- Rust **1.95.0**, with `Cargo.lock` and `--locked` dependency fetching.
- Python **3.12.10** in CI; packaging/notices use the standard library. This is
  the last 3.12 patch with official runner binaries for all four required
  OS/architecture combinations. Later 3.12 security releases have Linux-only
  entries in the official distribution manifest. Coverage was verified against
  [manifest revision 49858b9](https://github.com/actions/python-versions/blob/49858b91c92d27403416ee43117032e91d00ce24/versions-manifest.json):
  Windows x64, Ubuntu 24.04 x64, macOS arm64, and macOS x64.
- CMake **3.31.10** for the AU build.
- Exact AU dependency commits in `packaging/au/dependencies.json`, plus the tracked
  wrapper patch. Changed checkout contents are rejected.
- GitHub actions pinned to immutable commit SHAs in the workflow, with repository
  permissions restricted to `contents: read` and checkout credentials not persisted.
- Fixed runner labels: `windows-2022`, `ubuntu-24.04`, `macos-15`, and
  `macos-15-intel`. These remain moving hosted images; inspect the recorded tool
  versions and the run's **Set up job** image details when comparing builds.

Cargo fetches complete locked dependency archives before compilation. Those
archives are cached, with source caches retained for inspection and license
extraction. The notice generator validates registry archives against lockfile
SHA-256 checksums and fails when required texts or dependencies are missing. AU
repositories are fetched and checked before the offline release build. Release
bundles use default plugin features: diagnostic tests do not enable diagnostics in
the distributed plugin. Build output directories are not restored from CI caches.

`SOURCE_DATE_EPOCH` is the source commit's Unix timestamp. ZIP entry ordering,
timestamps and permissions are normalized, so packaging the **same bundle bytes,
receipt and source** repeatedly produces an identical ZIP and checksum. The build
receipt binds those bundles to a clean committed worktree, toolchain/lockfile and
build host. It is a provenance record, not a cryptographic source-to-binary proof.

We do **not** claim bit-identical compilation across hosted runner image updates,
Xcode/MSVC/linker versions, signing timestamps, or different operating systems.
No native-CPU optimization flags are used. Linux packages are built against Ubuntu
24.04's system libraries; compatibility with older glibc distributions is not
established. Automated checks also do not establish support for every DAW or every
macOS version back to the declared 11.0 deployment target.

## Reproduce a package locally

Start with a clean committed worktree and the repository's Rust toolchain. Install
GUI development libraries on Linux and MSVC C++ build tools on Windows; see
[building](building.md). Use the commands below in Bash, or substitute equivalent
PowerShell environment assignments on Windows. `python` must resolve to the desired
Python installation. CI fixes its Python version to 3.12.10.

```sh
cargo fetch --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --workspace --all-targets --locked --offline
cargo test -p lullaby-dsp --all-targets --locked --offline --features dsp-diagnostics
python -m unittest discover -s scripts -p 'test_*.py' -v
python scripts/dependency_notices.py --output target/beta-notices/THIRD-PARTY-NOTICES.txt --manifest target/beta-notices/manifest.json
```

For Windows or Linux, build with the matching native Rust toolchain:

```sh
export SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
BUILD_STARTED_EPOCH="$(python -c 'import time; print(int(time.time()))')"
cargo xtask bundle lullaby --release --locked --offline
```

On macOS, install both Rust targets and the pinned CMake, fetch AU sources, and
build universal bundles:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
python -m pip install --only-binary=:all: --no-deps 'cmake==3.31.10'
python scripts/build_au.py --fetch-dependencies-only
export SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
export MACOSX_DEPLOYMENT_TARGET=11.0
BUILD_STARTED_EPOCH="$(python -c 'import time; print(int(time.time()))')"
cargo xtask bundle-au lullaby --release --locked --offline --universal
python scripts/clap_smoke.py target/bundled/Lullaby.clap/Contents/MacOS/Lullaby
python scripts/auval.py
```

The AU build runs its native smoke test automatically. On Windows and Linux, test
the actual bundled CLAP before recording the receipt:

```sh
python scripts/clap_smoke.py target/bundled/Lullaby.clap
```

Set `BETA_PLATFORM` to `Windows-x86_64`, `Linux-x86_64`, or `macOS-universal`, then
record the completed build and create its archive:

```sh
python scripts/package_release.py --platform "$BETA_PLATFORM" --bundles target/bundled --receipt target/beta-build.json --record-build --build-start "$BUILD_STARTED_EPOCH"
python scripts/package_release.py --platform "$BETA_PLATFORM" --bundles target/bundled --receipt target/beta-build.json --output target/beta-packages
python scripts/package_release.py --check "target/beta-packages/Lullaby-<version>-$BETA_PLATFORM.zip" --expected-commit "$(git rev-parse HEAD)"
```

An existing release binary from an incremental build may be older than a new
build-start timestamp and will correctly fail freshness validation. Use a fresh
target directory/build when creating a new receipt; preserve the original receipt
when only reproducing an unchanged archive.

## Signing and beta readiness

Current beta packages are **unsigned on Windows** and **ad hoc signed on macOS,
not Developer ID signed and not notarized**. Ad hoc signature integrity checks do
not establish an identified publisher or satisfy Gatekeeper's distribution policy.
The workflow contains no signing identity, private key, Apple credentials, or
Windows certificate. Signature status is stated in package provenance and the
workflow summary.

Before a public commercial release, add a separately reviewed signing stage with
protected secret access. Sign macOS nested code from the inside out with Developer
ID, submit the intended distribution to `notarytool`, staple supported containers,
verify Gatekeeper assessment, then generate the final hashes from those signed
bytes. Windows Authenticode signing must likewise precede the final receipt/archive.
Signing or changing any bundled file invalidates the earlier hashes and requires
new package validation. Never relabel these beta archives as notarized or signed.

Manual DAW validation remains a separate release gate: installation/scanning,
editor operation, session recall, automation, multiple instances, latency changes,
small audio buffers, and long-running CPU/memory behavior. Record actual host/OS
versions and results before offering broad compatibility claims.

Runner labels and action inputs were checked against the official
[GitHub-hosted runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners),
[action repositories](https://github.com/actions), and their pinned `action.yml`
files on 2026-10-15. See also the official
[Python 3.12.10 release](https://www.python.org/downloads/release/python-31210/) and
[CMake 3.31.10 distribution](https://pypi.org/project/cmake/3.31.10/).
