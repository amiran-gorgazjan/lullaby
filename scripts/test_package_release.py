#!/usr/bin/env python3
"""Release packaging guards use format-specific binaries, not filename assertions."""

import io
import json
import os
from pathlib import Path
import plistlib
import stat
import struct
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import package_release as pkg


def elf(machine=62):
    data = bytearray(128)
    data[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HH", data, 16, 3, machine)
    return bytes(data)


def pe(machine=0x8664):
    data = bytearray(256)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 64)
    data[64:68] = b"PE\0\0"
    struct.pack_into("<H", data, 68, machine)
    struct.pack_into("<H", data, 86, 0x2000)
    struct.pack_into("<H", data, 88, 0x20B)
    return bytes(data)


def macho(cpu):
    data = bytearray(64)
    data[:4] = b"\xcf\xfa\xed\xfe"
    struct.pack_into("<I", data, 4, cpu)
    struct.pack_into("<I", data, 12, 6)
    return bytes(data)


def universal():
    data = bytearray(256)
    data[:4] = b"\xca\xfe\xba\xbe"
    struct.pack_into(">I", data, 4, 2)
    for index, cpu in enumerate((0x1000007, 0x100000C)):
        offset = 64 + index * 96
        struct.pack_into(">IIIII", data, 8 + index * 20, cpu, 0, offset, 64, 0)
        data[offset:offset + 64] = macho(cpu)
    return bytes(data)


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.bundles = self.root / "bundled"
        self.output = self.root / "output"
        self.receipt = self.root / "build.json"
        for relative in pkg.DOCUMENTS.values():
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("Fixture " + relative + "\n", encoding="utf-8")
        self.dependencies = {"schema_version": 1, "cargo_lock_sha256": "c" * 64,
                             "packages": [{"name": "fixture", "version": "1.0.0", "source": "registry+fixture",
                                           "checksum": "e" * 64, "texts": [{"path": "LICENSE", "sha256": "f" * 64}]}]}
        self.notices = "Locked dependency notices\n"
        self.source = {"commit": "a" * 40, "version": "0.2.0-beta.1", "source_date_epoch": 1780000001,
                       "tracked_tree_sha256": "b" * 64, "cargo_lock_sha256": "c" * 64,
                       "rust_toolchain_sha256": "d" * 64}
        self.addCleanup(patch.stopall)
        patch.object(pkg, "source_provenance", return_value=self.source).start()
        patch.object(pkg, "tool_metadata", return_value={"rustc": "fixture", "cargo": "fixture"}).start()
        patch.object(pkg, "signing_metadata", return_value={"signature": "fixture"}).start()

    def prepare(self, target="Linux-x86_64"):
        names, required, optional, binaries = pkg.bundle_layout(target)
        data = universal() if target.startswith("macOS") else pe() if target.startswith("Windows") else elf()
        for name in sorted(required | set(pkg.NOTICE_NAMES)):
            path = self.bundles / name
            path.parent.mkdir(parents=True, exist_ok=True)
            if name in binaries:
                contents = data
            elif name.endswith("Info.plist"):
                contents = plistlib.dumps({"CFBundleExecutable": "Lullaby", "CFBundleIdentifier": "audio.lullaby.test",
                                           "CFBundleVersion": "0.2.0", "CFBundleShortVersionString": "0.2.0"})
            elif name.endswith("DEPENDENCY-MANIFEST.json"):
                contents = pkg.json_bytes(self.dependencies)
            elif name.endswith("THIRD-PARTY-NOTICES.txt"):
                contents = self.notices.encode() + (b"Native AU wrapper notices\n" if name == "Lullaby.component/Contents/Resources/THIRD-PARTY-NOTICES.txt" else b"")
            else:
                contents = ("Fixture notice " + Path(name).name).encode()
            path.write_bytes(contents)
            os.utime(path, ns=(1780000010_000000000, 1780000010_000000000))
        pkg.record_build(self.root, self.bundles, target, self.receipt, 1780000005)
        return binaries

    def package(self, target="Linux-x86_64"):
        return pkg.make_package(self.root, self.bundles, target, self.output, self.receipt, notices=self.notices, dependency_manifest=self.dependencies)

    def rewrite(self, original, transform):
        stream = io.BytesIO()
        with zipfile.ZipFile(io.BytesIO(original)) as source, zipfile.ZipFile(stream, "w") as out:
            for info in source.infolist():
                info, data = transform(info, source.read(info))
                out.writestr(info, data)
        return stream.getvalue()

    def test_deterministic_zip_and_complete_manifest(self):
        binaries = self.prepare()
        (self.bundles / "private-research.txt").write_text("must be excluded")
        first = self.package().read_bytes()
        for name in binaries:
            os.utime(self.bundles / name, ns=(1780000020_000000000, 1780000020_000000000))
        second_path = self.package()
        self.assertEqual(first, second_path.read_bytes())
        manifest = pkg.inspect_archive(first, "a" * 40)
        self.assertNotIn("private-research.txt", manifest["files"])
        self.assertEqual(manifest["files"]["Lullaby.clap"]["mode"], "0755")
        self.assertEqual(manifest["files"]["LICENSE"]["mode"], "0644")
        self.assertEqual(manifest["source"], self.source)
        self.assertIn(pkg.digest(first), (self.output / "SHA256SUMS").read_text())
        self.assertIn(pkg.digest(first), second_path.with_suffix(".zip.sha256").read_text())

    def test_all_declared_platform_formats(self):
        for target in pkg.PLATFORMS:
            with self.subTest(target=target):
                self.bundles = self.root / target
                self.prepare(target)
                self.assertEqual(pkg.inspect_archive(self.package(target).read_bytes())["platform"], target)

    def test_wrong_architecture_rejected(self):
        self.prepare()
        (self.bundles / "Lullaby.clap").write_bytes(elf(183))
        with self.assertRaisesRegex(ValueError, "x86_64"):
            self.package()

    def test_wrong_executable_format_rejected(self):
        self.prepare()
        (self.bundles / "Lullaby.clap").write_bytes(pe())
        with self.assertRaisesRegex(ValueError, "Wrong architecture or format"):
            self.package()

    def test_thin_macos_cannot_be_labeled_universal(self):
        self.prepare("macOS-universal")
        (self.bundles / "Lullaby.clap/Contents/MacOS/Lullaby").write_bytes(macho(0x100000C))
        with self.assertRaisesRegex(ValueError, "Wrong architecture"):
            self.package("macOS-universal")

    def test_self_contained_au_has_complete_executable_embedded_clap(self):
        self.prepare("macOS-universal")
        manifest = pkg.inspect_archive(self.package("macOS-universal").read_bytes())
        binary = pkg.EMBEDDED_CLAP + "/Contents/MacOS/Lullaby"
        self.assertEqual(manifest["files"][binary]["mode"], "0755")
        self.assertIn(pkg.EMBEDDED_CLAP + "/Contents/Info.plist", manifest["files"])
        self.assertEqual(manifest["files"][pkg.EMBEDDED_CLAP + "/Contents/Resources/THIRD-PARTY-NOTICES.txt"]["sha256"],
                         manifest["files"]["THIRD-PARTY-NOTICES.txt"]["sha256"])

    def test_au_missing_embedded_clap_rejected(self):
        self.prepare("macOS-universal")
        import shutil
        shutil.rmtree(self.bundles / pkg.EMBEDDED_CLAP)
        with self.assertRaisesRegex(ValueError, "Incomplete plugin bundle"):
            self.package("macOS-universal")

    def test_au_embedded_clap_must_be_universal(self):
        self.prepare("macOS-universal")
        (self.bundles / pkg.EMBEDDED_CLAP / "Contents/MacOS/Lullaby").write_bytes(macho(0x100000C))
        with self.assertRaisesRegex(ValueError, "Wrong architecture"):
            self.package("macOS-universal")

    def test_au_embedded_clap_stale_full_version_rejected(self):
        self.prepare("macOS-universal")
        path = self.bundles / pkg.EMBEDDED_CLAP / "Contents/Info.plist"
        info = plistlib.loads(path.read_bytes())
        info["LullabyVersion"] = "0.2.0-beta.0"
        path.write_bytes(plistlib.dumps(info))
        with self.assertRaisesRegex(ValueError, "Stale full release version"):
            self.package("macOS-universal")

    def test_au_embedded_clap_tampering_rejected_by_receipt(self):
        self.prepare("macOS-universal")
        path = self.bundles / pkg.EMBEDDED_CLAP / "Contents/MacOS/Lullaby"
        path.write_bytes(path.read_bytes() + b"changed")
        with self.assertRaisesRegex(ValueError, "Bundle changed"):
            self.package("macOS-universal")

    def test_au_embedded_clap_notices_must_match_standalone(self):
        self.prepare("macOS-universal")
        path = self.bundles / pkg.EMBEDDED_CLAP / "Contents/Resources/THIRD-PARTY-NOTICES.txt"
        path.write_bytes(self.notices.encode() + b"wrong native AU notices")
        with self.assertRaisesRegex(ValueError, "Bundle notice differs"):
            self.package("macOS-universal")

    def test_macho_false_slice_header_rejected(self):
        data = bytearray(universal())
        struct.pack_into(">I", data, 8, 0x100000C)
        with self.assertRaisesRegex(ValueError, "architecture disagrees"):
            pkg.binary_architecture(data)

    def test_macho_overlapping_slice_rejected(self):
        data = bytearray(universal())
        struct.pack_into(">I", data, 36, 64)
        with self.assertRaisesRegex(ValueError, "Overlapping"):
            pkg.binary_architecture(data)

    def test_stale_macos_bundle_version_rejected(self):
        self.prepare("macOS-universal")
        info = self.bundles / "Lullaby.vst3/Contents/Info.plist"
        value = plistlib.loads(info.read_bytes())
        value["CFBundleVersion"] = "1.0.0"
        info.write_bytes(plistlib.dumps(value))
        with self.assertRaisesRegex(ValueError, "Stale bundle version"):
            self.package("macOS-universal")

    def test_missing_au_notice_rejected(self):
        self.prepare("macOS-universal")
        (self.bundles / "Lullaby.component/Contents/Resources/THIRD-PARTY-NOTICES.txt").unlink()
        with self.assertRaisesRegex(ValueError, "Incomplete"):
            self.package("macOS-universal")

    def test_missing_binary_rejected(self):
        self.prepare()
        (self.bundles / "Lullaby.vst3/Contents/x86_64-linux/Lullaby.so").unlink()
        with self.assertRaisesRegex(ValueError, "Incomplete"):
            self.package()

    def test_unexpected_bundle_contents_rejected(self):
        self.prepare()
        (self.bundles / "Lullaby.vst3/Contents/secret.txt").write_text("secret")
        with self.assertRaisesRegex(ValueError, "Unexpected plugin bundle files"):
            self.package()

    def test_symlink_bundle_input_rejected(self):
        self.prepare()
        link = self.bundles / "Lullaby.vst3/Contents/Resources/secret"
        try:
            link.symlink_to(self.root)
        except OSError:
            self.skipTest("Symlink creation unavailable on this host")
        with self.assertRaisesRegex(ValueError, "Symlinks"):
            self.package()

    def test_wrong_source_receipt_rejected(self):
        self.prepare()
        value = json.loads(self.receipt.read_text())
        value["source"]["commit"] = "b" * 40
        self.receipt.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, "source revision"):
            self.package()

    def test_changed_binary_receipt_rejected(self):
        self.prepare()
        with (self.bundles / "Lullaby.clap").open("ab") as handle:
            handle.write(b"tamper")
        with self.assertRaisesRegex(ValueError, "Bundle changed"):
            self.package()

    def test_stale_binary_before_build_start_rejected(self):
        self.prepare()
        with self.assertRaisesRegex(ValueError, "predates build start"):
            pkg.record_build(self.root, self.bundles, "Linux-x86_64", self.receipt, 1780000011)

    def test_package_tampering_is_detected(self):
        self.prepare()
        raw = self.package().read_bytes()
        damaged = self.rewrite(raw, lambda info, data: (info, data + b"tamper" if info.filename.endswith("Lullaby.clap") else data))
        with self.assertRaisesRegex(ValueError, "content hash"):
            pkg.inspect_archive(damaged)

    def test_expected_source_commit_required(self):
        self.prepare()
        with self.assertRaisesRegex(ValueError, "source revision"):
            pkg.inspect_archive(self.package().read_bytes(), "b" * 40)

    def test_zip_traversal_rejected(self):
        self.prepare()
        raw = self.package().read_bytes()
        def unsafe(info, data):
            if info.filename.endswith("LICENSE"):
                info.filename = "prefix/../LICENSE"
            return info, data
        with self.assertRaisesRegex(ValueError, "Unsafe archive path"):
            pkg.inspect_archive(self.rewrite(raw, unsafe))

    def test_zip_symlink_mode_rejected(self):
        self.prepare()
        def symlink(info, data):
            if info.filename.endswith("LICENSE"):
                info.external_attr = (stat.S_IFLNK | 0o777) << 16
            return info, data
        with self.assertRaisesRegex(ValueError, "permissions"):
            pkg.inspect_archive(self.rewrite(self.package().read_bytes(), symlink))

    def test_zip_executable_mode_tampering_rejected(self):
        self.prepare()
        def strip_exec(info, data):
            if info.filename.endswith("Lullaby.clap"):
                info.external_attr = (stat.S_IFREG | 0o644) << 16
            return info, data
        with self.assertRaisesRegex(ValueError, "permissions disagree"):
            pkg.inspect_archive(self.rewrite(self.package().read_bytes(), strip_exec))

    def test_unsafe_names(self):
        for name in ("../x", "x/../y", "/x", "x\\y", "C:/x", "x//y", "x/./y", "x\0y"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                pkg.safe_name(name)

    def test_dependency_manifest_lock_mismatch_rejected(self):
        self.prepare()
        changed = dict(self.dependencies, cargo_lock_sha256="0" * 64)
        with self.assertRaisesRegex(ValueError, "source Cargo.lock"):
            pkg.make_package(self.root, self.bundles, "Linux-x86_64", self.output, self.receipt,
                             notices=self.notices, dependency_manifest=changed)

    def test_generated_dependency_notices_must_match_build(self):
        self.prepare()
        with self.assertRaisesRegex(ValueError, "Built Rust dependency notices differ"):
            pkg.make_package(self.root, self.bundles, "Linux-x86_64", self.output, self.receipt,
                             notices="Different locked notices\n", dependency_manifest=self.dependencies)

    def test_au_must_retain_native_wrapper_notices(self):
        self.prepare("macOS-universal")
        (self.bundles / "Lullaby.component/Contents/Resources/THIRD-PARTY-NOTICES.txt").write_text(self.notices)
        with self.assertRaisesRegex(ValueError, "native wrapper notices"):
            self.package("macOS-universal")

    def test_source_mutation_during_packaging_rejected(self):
        self.prepare()
        changed = dict(self.source, commit="b" * 40)
        with patch.object(pkg, "source_provenance", side_effect=[self.source, changed]):
            with self.assertRaisesRegex(ValueError, "Source changed during"):
                self.package()
        self.assertFalse(self.output.exists())

    def test_bundle_notice_mismatch_rejected(self):
        self.prepare()
        (self.bundles / "Lullaby.vst3/Contents/Resources/LULLABY-LICENSES.txt").write_text("wrong notice")
        with self.assertRaisesRegex(ValueError, "notice differs"):
            self.package()

    def test_duplicate_zip_paths_rejected(self):
        self.prepare()
        stream = io.BytesIO(self.package().read_bytes())
        with zipfile.ZipFile(stream, "a") as archive:
            first = archive.infolist()[0]
            import warnings
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                archive.writestr(first, archive.read(first))
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            pkg.inspect_archive(stream.getvalue())

    def test_zip_timestamp_tampering_rejected(self):
        self.prepare()
        def timestamp(info, data):
            if info.filename.endswith("LICENSE"):
                info.date_time = (1980, 1, 1, 0, 0, 0)
            return info, data
        with self.assertRaisesRegex(ValueError, "timestamps"):
            pkg.inspect_archive(self.rewrite(self.package().read_bytes(), timestamp))

    def test_empty_dependency_notices_rejected(self):
        self.prepare()
        with self.assertRaisesRegex(ValueError, "must not be empty"):
            pkg.make_package(self.root, self.bundles, "Linux-x86_64", self.output, self.receipt, notices="\n", dependency_manifest=self.dependencies)


class SourceProvenanceTests(unittest.TestCase):
    def test_dirty_worktree_rejected_before_reading_build_files(self):
        with patch.object(pkg, "run", return_value=" M src/lib.rs"):
            with self.assertRaisesRegex(ValueError, "clean Git worktree"):
                pkg.source_provenance(Path("unused"))

    def test_signing_reports_unsigned_windows_binary(self):
        with tempfile.TemporaryDirectory() as directory:
            bundles = Path(directory)
            for name in pkg.bundle_layout("Windows-x86_64")[3]:
                path = bundles / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(pe())
            result = pkg.signing_metadata(bundles, "Windows-x86_64")
            self.assertEqual({item["signature"] for item in result.values()}, {"unsigned"})

    def test_version_parsing_is_package_scoped(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[workspace]\nversion = "9.9.9"\n[package]\nname = "lullaby"\nversion = "0.2.0-beta.1"\n')
            self.assertEqual(pkg.version(root), "0.2.0-beta.1")


if __name__ == "__main__":
    unittest.main()
