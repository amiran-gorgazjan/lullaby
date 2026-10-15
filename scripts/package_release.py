#!/usr/bin/env python3
"""Create and independently verify versioned, deterministic beta ZIP packages.

A build receipt binds binaries to a clean source revision and build start time.
Repackaging identical files and the same receipt is deterministic; this does not
claim that compilers on different hosts produce identical binaries.
"""

import argparse
import datetime
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import plistlib
import re
import stat
import struct
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = ("Windows-x86_64", "macOS-universal", "Linux-x86_64")
DOCUMENTS = {
    "README.md": "docs/beta-README.md", "INSTALL.md": "docs/install.md",
    "LICENSE": "LICENSE", "MIT-BASELINE.txt": "licenses/MIT-BASELINE.txt",
}
NOTICE_NAMES = ("FONT-LICENSES.txt", "LULLABY-LICENSES.txt",
                "THIRD-PARTY-NOTICES.txt", "DEPENDENCY-MANIFEST.json")
HEX = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40,64}$")
EMBEDDED_CLAP = "Lullaby.component/Contents/PlugIns/Lullaby.clap"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def json_bytes(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode()


def run(args, root=ROOT):
    return subprocess.check_output(args, cwd=root, text=True).strip()


def version(root):
    text = (root / "Cargo.toml").read_text(encoding="utf-8")
    package = re.search(r"(?ms)^\[package\]\s*(.*?)(?=^\[|\Z)", text)
    found = re.search(r'(?m)^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?)"', package[1] if package else "")
    if not found:
        raise ValueError("Missing explicit package version in Cargo.toml")
    return found[1]


def source_provenance(root):
    if run(["git", "status", "--porcelain", "--untracked-files=normal"], root):
        raise ValueError("Release packaging requires a clean Git worktree; commit source changes first")
    commit = run(["git", "rev-parse", "HEAD"], root)
    if not COMMIT.fullmatch(commit):
        raise ValueError("Invalid source commit")
    epoch = int(os.environ.get("SOURCE_DATE_EPOCH", run(["git", "show", "-s", "--format=%ct", "HEAD"], root)))
    if epoch < 0 or epoch > 4354819199:
        raise ValueError("SOURCE_DATE_EPOCH is outside the ZIP timestamp range")
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=root).split(b"\0")
    sources = {}
    for raw in tracked:
        if not raw:
            continue
        relative = os.fsdecode(raw)
        path = root / relative
        if path.is_symlink() or not path.is_file():
            raise ValueError("Release source must contain regular tracked files: " + relative)
        sources[relative] = digest(path.read_bytes())
    return {
        "commit": commit, "version": version(root), "source_date_epoch": epoch,
        "tracked_tree_sha256": digest(json_bytes(sources)),
        "cargo_lock_sha256": digest((root / "Cargo.lock").read_bytes()),
        "rust_toolchain_sha256": digest((root / "rust-toolchain.toml").read_bytes()),
    }


def tool_metadata(root):
    tools = {
        "rustc": run(["rustc", "-vV"], root), "cargo": run(["cargo", "--version"], root),
        "python": platform.python_version(), "host_os": platform.system(),
        "host_release": platform.release(), "host_architecture": platform.machine(),
    }
    commands = {"cmake": ["cmake", "--version"]}
    if platform.system() == "Darwin":
        commands.update({"xcode": ["xcodebuild", "-version"], "macos_sdk": ["xcrun", "--show-sdk-version"],
                         "clang": ["clang++", "--version"]})
    elif platform.system() == "Linux":
        commands.update({"cc": ["cc", "--version"], "ld": ["ld", "--version"], "libc": ["ldd", "--version"]})
    elif platform.system() == "Windows":
        commands["msvc"] = ["cmd", "/c", "cl"]
    for name, args in sorted(commands.items()):
        try:
            result = subprocess.run(args, cwd=root, capture_output=True, text=True, timeout=30)
            text = (result.stdout + result.stderr).strip()
            tools[name] = {"exit_code": result.returncode, "version_output": text}
        except (OSError, subprocess.TimeoutExpired) as error:
            tools[name] = {"unavailable": type(error).__name__}
    tools["build_environment"] = {key: os.environ[key] for key in (
        "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_WORKFLOW", "GITHUB_JOB", "RUNNER_OS", "RUNNER_ARCH",
        "ImageOS", "ImageVersion", "MACOSX_DEPLOYMENT_TARGET", "SDKROOT", "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS", "CMAKE_OSX_DEPLOYMENT_TARGET") if key in os.environ}
    return tools


def binary_architecture(data):
    """Read PE, ELF and every Mach-O fat slice; never infer architecture from names."""
    data = bytes(data)
    if data[:2] == b"MZ":
        if len(data) < 64:
            raise ValueError("Truncated PE header")
        offset = struct.unpack_from("<I", data, 60)[0]
        if len(data) < offset + 26 or data[offset:offset + 4] != b"PE\0\0":
            raise ValueError("Invalid PE signature")
        machine, = struct.unpack_from("<H", data, offset + 4)
        characteristics, = struct.unpack_from("<H", data, offset + 22)
        magic, = struct.unpack_from("<H", data, offset + 24)
        if machine != 0x8664 or magic != 0x20B or not characteristics & 0x2000:
            raise ValueError("Expected a Windows x86_64 PE32+ DLL")
        return "PE", ["x86_64"]
    if data[:4] == b"\x7fELF":
        if len(data) < 64 or data[4:7] != b"\x02\x01\x01":
            raise ValueError("Expected a little-endian ELF64 binary")
        kind, machine = struct.unpack_from("<HH", data, 16)
        if kind != 3 or machine != 62:
            raise ValueError("Expected an x86_64 ELF shared library")
        return "ELF", ["x86_64"]
    thin = {b"\xcf\xfa\xed\xfe": "<", b"\xfe\xed\xfa\xcf": ">"}
    if data[:4] in thin:
        endian = thin[data[:4]]
        if len(data) < 32:
            raise ValueError("Truncated Mach-O header")
        cpu, = struct.unpack_from(endian + "I", data, 4)
        kind, = struct.unpack_from(endian + "I", data, 12)
        if cpu not in (0x1000007, 0x100000C) or kind not in (6, 8):
            raise ValueError("Expected an x86_64/arm64 Mach-O dynamic library or bundle")
        return "Mach-O", ["x86_64" if cpu == 0x1000007 else "arm64"]
    fats = {b"\xca\xfe\xba\xbe": (">", False), b"\xbe\xba\xfe\xca": ("<", False),
            b"\xca\xfe\xba\xbf": (">", True), b"\xbf\xba\xfe\xca": ("<", True)}
    if data[:4] in fats:
        endian, wide = fats[data[:4]]
        if len(data) < 8:
            raise ValueError("Truncated fat Mach-O header")
        count, = struct.unpack_from(endian + "I", data, 4)
        record_size = 32 if wide else 20
        table_end = 8 + count * record_size
        if count != 2 or len(data) < table_end:
            raise ValueError("Universal macOS builds must contain exactly two complete slices")
        architectures, regions = [], []
        for index in range(count):
            offset = 8 + index * record_size
            cpu, = struct.unpack_from(endian + "I", data, offset)
            start, length = struct.unpack_from(endian + ("QQ" if wide else "II"), data, offset + 8)
            if start < table_end or length < 32 or start + length > len(data):
                raise ValueError("Invalid Mach-O slice bounds")
            if any(start < end and begin < start + length for begin, end in regions):
                raise ValueError("Overlapping Mach-O slices")
            kind, actual = binary_architecture(data[start:start + length])
            expected = {0x1000007: "x86_64", 0x100000C: "arm64"}.get(cpu)
            if kind != "Mach-O" or actual != [expected] or expected in architectures:
                raise ValueError("Mach-O slice architecture disagrees with fat header")
            architectures.extend(actual)
            regions.append((start, start + length))
        return "Mach-O", sorted(architectures)
    raise ValueError("Unrecognized plugin executable format")


def bundle_layout(target):
    if target not in PLATFORMS:
        raise ValueError("Unsupported release platform: " + target)
    if target == "macOS-universal":
        names = ("Lullaby.vst3", "Lullaby.clap", "Lullaby.component")
        required, optional, binaries = set(), set(), set()
        for name in names + (EMBEDDED_CLAP,):
            binary = name + "/Contents/MacOS/Lullaby"
            binaries.add(binary)
            required.update((binary, name + "/Contents/Info.plist", name + "/Contents/PkgInfo"))
            required.update(name + "/Contents/Resources/" + notice for notice in NOTICE_NAMES)
            optional.update(name + "/Contents/" + suffix for suffix in (
                "_CodeSignature/CodeResources", "CodeResources", "Resources/FONT-LICENSES.txt",
                "Resources/THIRD-PARTY-NOTICES.txt", "Resources/moduleinfo.json"))
            if name == "Lullaby.component":
                required.add(name + "/Contents/Resources/THIRD-PARTY-NOTICES.txt")
            else:
                required.add(name + "/Contents/Resources/FONT-LICENSES.txt")
    else:
        names = ("Lullaby.vst3", "Lullaby.clap")
        binary = ("Lullaby.vst3/Contents/x86_64-win/Lullaby.vst3" if target.startswith("Windows")
                  else "Lullaby.vst3/Contents/x86_64-linux/Lullaby.so")
        binaries = {binary, "Lullaby.clap"}
        required = binaries | {"Lullaby.vst3/Contents/Resources/" + name for name in NOTICE_NAMES}
        optional = {"Lullaby.vst3/Contents/Resources/moduleinfo.json"}
    return names, required, optional, binaries


def safe_name(name):
    if not name or "\\" in name or "\x00" in name or ":" in name or name.startswith("/"):
        raise ValueError("Unsafe archive path: " + repr(name))
    if any(part in ("", ".", "..") for part in name.split("/")):
        raise ValueError("Unsafe archive path: " + repr(name))
    return PurePosixPath(name)


def validate_payload(files, target, release_version):
    names, required, optional, binaries = bundle_layout(target)
    plugin_files = {name for name in files if name.split("/")[0] in names}
    if required - plugin_files:
        raise ValueError("Incomplete plugin bundle: " + ", ".join(sorted(required - plugin_files)))
    if plugin_files - required - optional:
        raise ValueError("Unexpected plugin bundle files: " + ", ".join(sorted(plugin_files - required - optional)))
    expected_format = "Mach-O" if target.startswith("macOS") else "PE" if target.startswith("Windows") else "ELF"
    expected_arch = ["arm64", "x86_64"] if target.startswith("macOS") else ["x86_64"]
    for name in binaries:
        if binary_architecture(files[name]) != (expected_format, expected_arch):
            raise ValueError("Wrong architecture or format for " + name)
    if target.startswith("macOS"):
        for name in names + (EMBEDDED_CLAP,):
            info = plistlib.loads(files[name + "/Contents/Info.plist"])
            accepted = (release_version, release_version.split("-", 1)[0])
            if "LullabyVersion" in info and info["LullabyVersion"] != release_version:
                raise ValueError("Stale full release version in " + name)
            if info.get("CFBundleExecutable") != "Lullaby":
                raise ValueError("Unexpected bundle executable in " + name)
            if info.get("CFBundleShortVersionString") not in accepted or info.get("CFBundleVersion") not in accepted:
                raise ValueError("Stale bundle version in " + name)
            if not isinstance(info.get("CFBundleIdentifier"), str) or not info["CFBundleIdentifier"]:
                raise ValueError("Missing bundle identifier in " + name)
    for name in NOTICE_NAMES:
        if name not in files or not files[name].strip():
            raise ValueError("Missing or empty notice: " + name)
    for name in plugin_files:
        basename = PurePosixPath(name).name
        if basename in NOTICE_NAMES:
            if name == "Lullaby.component/Contents/Resources/THIRD-PARTY-NOTICES.txt":
                if not files[name].startswith(files[basename]) or len(files[name]) <= len(files[basename]):
                    raise ValueError("AU notices must include Rust notices followed by native wrapper notices")
            elif files[name] != files[basename]:
                raise ValueError("Bundle notice differs from companion notice: " + name)
    return binaries


def collect_bundles(bundles, target, release_version):
    if bundles.is_symlink() or not bundles.is_dir():
        raise ValueError("Bundle directory must be a real directory")
    names, required, optional, binaries = bundle_layout(target)
    files, times = {}, {}
    for name in names + NOTICE_NAMES:
        base = bundles / name
        if not base.exists() or base.is_symlink():
            raise ValueError("Missing or symlinked bundle input: " + name)
        paths = sorted(base.rglob("*")) if base.is_dir() else [base]
        for path in paths:
            if path.is_symlink():
                raise ValueError("Symlinks are not allowed in plugin bundles: " + str(path))
            if path.is_dir():
                continue
            if not stat.S_ISREG(path.stat().st_mode):
                raise ValueError("Non-regular bundle input: " + str(path))
            relative = path.relative_to(bundles).as_posix()
            safe_name(relative)
            files[relative] = path.read_bytes()
            times[relative] = path.stat().st_mtime_ns
    validate_payload(files, target, release_version)
    return files, times


def file_inventory(files, binaries):
    return {name: {"sha256": digest(data), "size": len(data), "mode": "0755" if name in binaries else "0644"}
            for name, data in sorted(files.items())}


def atomic_write(path, contents):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix="." + path.name, delete=False) as handle:
            temporary = Path(handle.name)
            handle.write(contents)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        if temporary and temporary.exists():
            temporary.unlink()


def signing_metadata(bundles, target):
    if target.startswith("macOS"):
        if platform.system() != "Darwin":
            raise ValueError("macOS build signing must be inspected on macOS")
        result = {}
        for name in bundle_layout(target)[0]:
            display = subprocess.run(["codesign", "--display", "--verbose=4", str(bundles / name)],
                                     capture_output=True, text=True)
            if display.returncode:
                if "not signed at all" not in display.stderr:
                    raise ValueError("Could not inspect macOS signing: " + display.stderr.strip())
                result[name] = {"signature": "unsigned", "notarization": "not_notarized"}
                continue
            verified = subprocess.run(["codesign", "--verify", "--deep", "--strict", str(bundles / name)],
                                      capture_output=True, text=True)
            if verified.returncode:
                raise ValueError("Invalid macOS code signature: " + verified.stderr.strip())
            adhoc = "Signature=adhoc" in display.stderr
            result[name] = {"signature": "adhoc" if adhoc else "signed_identity",
                            "signature_integrity": "verified",
                            "notarization": "not_notarized" if adhoc else "not_verified"}
        return result
    if target.startswith("Windows"):
        result = {}
        for name in sorted(bundle_layout(target)[3]):
            data = (bundles / name).read_bytes()
            pe_offset, = struct.unpack_from("<I", data, 60)
            directory = pe_offset + 24 + 112 + 4 * 8
            certificate = struct.unpack_from("<II", data, directory) if len(data) >= directory + 8 else (0, 0)
            result[name] = {"signature": "embedded_signature_not_trust_verified" if all(certificate) else "unsigned"}
        return result
    return {"signature": "not_applicable"}


def record_build(root, bundles, target, receipt_path, build_start):
    source = source_provenance(root)
    if build_start < 0:
        raise ValueError("Build start must be a nonnegative epoch timestamp captured before compiling")
    files, times = collect_bundles(bundles, target, source["version"])
    binaries = bundle_layout(target)[3]
    if any(times[name] < build_start * 1_000_000_000 for name in binaries):
        raise ValueError("Stale plugin binary predates build start; rebuild instead of relabeling it")
    receipt = {"schema": 1, "platform": target, "source": source, "build_started_at": build_start,
               "tools": tool_metadata(root), "signing": signing_metadata(bundles, target),
               "bundle_files": file_inventory(files, binaries)}
    if source_provenance(root) != source:
        raise ValueError("Source changed while recording build provenance")
    atomic_write(receipt_path, json_bytes(receipt))
    return receipt


def zip_timestamp(epoch):
    instant = datetime.datetime.fromtimestamp(max(epoch, 315532800), datetime.timezone.utc)
    return (instant.year, instant.month, instant.day, instant.hour, instant.minute, instant.second // 2 * 2)


def archive_bytes(prefix, files, modes, epoch):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9, allowZip64=True) as archive:
        for name, data in sorted(files.items()):
            safe_name(name)
            info = zipfile.ZipInfo(prefix + "/" + name, date_time=zip_timestamp(epoch))
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | int(modes[name], 8)) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    return stream.getvalue()


def validate_dependencies(data, source):
    dependencies = json.loads(data)
    if json_bytes(dependencies) != data or dependencies.get("schema_version") != 1:
        raise ValueError("Invalid or noncanonical dependency manifest")
    if dependencies.get("cargo_lock_sha256") != source["cargo_lock_sha256"]:
        raise ValueError("Dependency manifest does not match source Cargo.lock")
    packages = dependencies.get("packages")
    if not isinstance(packages, list) or not packages:
        raise ValueError("Dependency manifest must enumerate locked packages")
    identities = set()
    for package in packages:
        identity = (package.get("name"), package.get("version"), package.get("source"))
        if not all(isinstance(item, str) and item for item in identity) or identity in identities:
            raise ValueError("Invalid or duplicate dependency identity")
        if not HEX.fullmatch(package.get("checksum", "")) or not package.get("texts"):
            raise ValueError("Dependency source and license text hashes are required")
        for text in package["texts"]:
            if not HEX.fullmatch(text.get("sha256", "")):
                raise ValueError("Invalid dependency notice hash")
        identities.add(identity)
    return dependencies


def inspect_archive(data, expected_commit=None):
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        entries = archive.infolist()
        if len(entries) != len(set(entry.filename for entry in entries)):
            raise ValueError("Duplicate ZIP paths")
        files, modes, prefix = {}, {}, None
        for entry in entries:
            path = safe_name(entry.filename)
            if len(path.parts) < 2 or entry.is_dir() or entry.flag_bits & 1:
                raise ValueError("Archive must contain ordinary unencrypted files under one prefix")
            candidate = path.parts[0]
            if prefix is not None and prefix != candidate:
                raise ValueError("Multiple archive prefixes")
            prefix = candidate
            mode = entry.external_attr >> 16
            if stat.S_IFMT(mode) != stat.S_IFREG or stat.S_IMODE(mode) not in (0o644, 0o755):
                raise ValueError("Unsafe or unsupported ZIP file permissions")
            relative = "/".join(path.parts[1:])
            files[relative] = archive.read(entry)
            modes[relative] = format(stat.S_IMODE(mode), "04o")
        if "manifest.json" not in files:
            raise ValueError("Missing package manifest")
        raw_manifest = files.pop("manifest.json")
        manifest = json.loads(raw_manifest)
        if raw_manifest != json_bytes(manifest):
            raise ValueError("Manifest is not canonically encoded")
        if manifest.get("schema") != 1 or manifest.get("platform") not in PLATFORMS:
            raise ValueError("Unsupported manifest")
        source = manifest["source"]
        if not COMMIT.fullmatch(source["commit"]) or (expected_commit and source["commit"] != expected_commit):
            raise ValueError("Package source revision does not match")
        expected_prefix = "Lullaby-" + source["version"] + "-" + manifest["platform"]
        if prefix != expected_prefix:
            raise ValueError("Package name does not match manifest")
        binaries = validate_payload(files, manifest["platform"], source["version"])
        validate_dependencies(files["DEPENDENCY-MANIFEST.json"], source)
        legal = set(DOCUMENTS) | set(NOTICE_NAMES) | {"THIRD-PARTY-NOTICES.txt"}
        names, required, optional, unused = bundle_layout(manifest["platform"])
        if set(files) - legal - required - optional or legal - set(files):
            raise ValueError("Unexpected or missing package contents")
        if manifest["files"] != file_inventory(files, binaries):
            raise ValueError("Package content hash, size or permissions disagree with manifest")
        for name in files:
            if modes[name] != manifest["files"][name]["mode"]:
                raise ValueError("Archive permissions disagree with manifest")
        if modes["manifest.json"] != "0644":
            raise ValueError("Invalid manifest permissions")
        if any(entry.date_time != zip_timestamp(source["source_date_epoch"]) for entry in entries):
            raise ValueError("Archive timestamps disagree with source date")
        if [entry.filename for entry in entries] != sorted(entry.filename for entry in entries):
            raise ValueError("Archive entries are not deterministically sorted")
        return manifest


def make_package(root, bundles, target, output, receipt_path, notices=None, dependency_manifest=None):
    source = source_provenance(root)
    files, times = collect_bundles(bundles, target, source["version"])
    binaries = bundle_layout(target)[3]
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    if receipt.get("schema") != 1 or receipt.get("platform") != target or receipt.get("source") != source:
        raise ValueError("Build receipt does not match the clean source revision/platform")
    if receipt.get("bundle_files") != file_inventory(files, binaries):
        raise ValueError("Bundle changed since its successful build receipt")
    if any(times[name] < receipt["build_started_at"] * 1_000_000_000 for name in binaries):
        raise ValueError("Plugin binary predates recorded build start")
    for name, relative in DOCUMENTS.items():
        path = root / relative
        if path.is_symlink() or not path.is_file():
            raise ValueError("Missing or unsafe package documentation: " + relative)
        files[name] = path.read_bytes()
    if notices is None or dependency_manifest is None:
        from dependency_notices import cargo_metadata, dependency_notices, dependency_notice_manifest
        metadata = cargo_metadata(root=root)
        notices = dependency_notices(metadata, root=root)
        dependency_manifest = dependency_notice_manifest(metadata, root=root)
    if not notices.strip():
        raise ValueError("Dependency notices must not be empty")
    dependency_bytes = json_bytes(dependency_manifest)
    validate_dependencies(dependency_bytes, source)
    if files["THIRD-PARTY-NOTICES.txt"] != notices.encode("utf-8"):
        raise ValueError("Built Rust dependency notices differ from the locked source notices")
    if files["DEPENDENCY-MANIFEST.json"] != dependency_bytes:
        raise ValueError("Built dependency manifest differs from the locked source manifest")
    inventory = file_inventory(files, binaries)
    manifest = {"schema": 1, "product": "Lullaby", "platform": target, "source": source,
                "build": {"started_at": receipt["build_started_at"], "tools": receipt["tools"],
                          "signing": receipt["signing"]},
                "files": inventory,
                "reproducibility": "Deterministic repackaging of identical built inputs and build receipt; no cross-host compiler bit-identity claim."}
    files["manifest.json"] = json_bytes(manifest)
    modes = {name: item["mode"] for name, item in inventory.items()}
    modes["manifest.json"] = "0644"
    prefix = "Lullaby-" + source["version"] + "-" + target
    data = archive_bytes(prefix, files, modes, source["source_date_epoch"])
    inspect_archive(data, source["commit"])
    if data != archive_bytes(prefix, files, modes, source["source_date_epoch"]):
        raise ValueError("Deterministic repackaging verification failed")
    if source_provenance(root) != source:
        raise ValueError("Source changed during packaging")
    current_files, unused_times = collect_bundles(bundles, target, source["version"])
    if file_inventory(current_files, binaries) != receipt["bundle_files"]:
        raise ValueError("Bundle changed during packaging")
    archive = output / (prefix + ".zip")
    atomic_write(archive, data)
    if archive.read_bytes() != data:
        raise ValueError("Written archive differs from verified bytes")
    checksum = digest(data)
    atomic_write(archive.with_suffix(".zip.sha256"), (checksum + "  " + archive.name + "\n").encode())
    # Preserve checksums for previously built platforms in the same output directory.
    sums = []
    for path in sorted(output.glob("Lullaby-*.zip")):
        inspect_archive(path.read_bytes())
        sums.append(digest(path.read_bytes()) + "  " + path.name + "\n")
    atomic_write(output / "SHA256SUMS", "".join(sums).encode())
    return archive


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=PLATFORMS)
    parser.add_argument("--bundles", type=Path, default=ROOT / "target/bundled")
    parser.add_argument("--output", type=Path, default=ROOT / "target/beta-packages")
    parser.add_argument("--receipt", type=Path, help="Build provenance receipt, created after a successful build")
    parser.add_argument("--record-build", action="store_true", help="Validate and record a completed build without archiving")
    parser.add_argument("--build-start", type=int, help="Unix timestamp captured immediately before the build")
    parser.add_argument("--check", type=Path, help="Independently inspect a package without building or extracting it")
    parser.add_argument("--expected-commit", help="Required source commit when checking an archive")
    args = parser.parse_args()
    try:
        if args.check:
            manifest = inspect_archive(args.check.read_bytes(), args.expected_commit)
            print(json.dumps({"archive": str(args.check), "sha256": digest(args.check.read_bytes()),
                              "platform": manifest["platform"], "version": manifest["source"]["version"],
                              "source_commit": manifest["source"]["commit"], "verified": True}, sort_keys=True))
        else:
            if not args.platform or not args.receipt:
                parser.error("--platform and --receipt are required for building or packaging")
            if args.record_build:
                if args.build_start is None:
                    parser.error("--record-build requires --build-start captured before compiling")
                record_build(ROOT, args.bundles, args.platform, args.receipt, args.build_start)
                print(args.receipt)
            else:
                print(make_package(ROOT, args.bundles, args.platform, args.output, args.receipt))
    except (ValueError, KeyError, OSError, zipfile.BadZipFile, plistlib.InvalidFileException, subprocess.CalledProcessError, RuntimeError) as error:
        parser.exit(1, "Packaging failed: " + str(error) + "\n")


if __name__ == "__main__":
    main()
