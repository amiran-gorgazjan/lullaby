"""Offline guards for complete, reproducible binary attribution."""

import copy
import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import dependency_notices as notices


class DependencyNoticeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "packaging/licenses").mkdir(parents=True)
        (self.root / "assets/fonts").mkdir(parents=True)
        for path in notices.FONT_NOTICES:
            (self.root / path).write_text("SIL OPEN FONT LICENSE 1.1\n", encoding="utf-8")
        self.metadata = {"packages": []}
        self.locked = []
        self.overrides = {"schema_version": 1, "packages": {}}
        self.flush()

    def flush(self):
        (self.root / "Cargo.lock").write_text(
            "version = 4\n\n" + "\n\n".join(
                "[[package]]\n" + "\n".join(key + " = " + json.dumps(value) for key, value in record.items())
                for record in self.locked
            ) + "\n", encoding="utf-8",
        )
        (self.root / "packaging/licenses/overrides.json").write_text(json.dumps(self.overrides), encoding="utf-8")

    def package(self, name="example", version="1.0.0", files=None, source="registry+https://github.com/rust-lang/crates.io-index", commit="a" * 40):
        prefix = name + "-" + version
        source_dir = self.root / "cargo/registry/src/example-index" / prefix
        source_dir.mkdir(parents=True)
        archive = self.root / "cargo/registry/cache/example-index" / (prefix + ".crate")
        archive.parent.mkdir(parents=True, exist_ok=True)
        contents = {"LICENSE-MIT": b"MIT license\n"} if files is None else dict(files)
        contents[".cargo_vcs_info.json"] = json.dumps({"git": {"sha1": commit}}).encode()
        with tarfile.open(archive, "w:gz") as output:
            for path, data in contents.items():
                member = tarfile.TarInfo(prefix + "/" + path)
                member.size = len(data)
                output.addfile(member, io.BytesIO(data))
        checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
        package = {"name": name, "version": version, "source": source, "manifest_path": str(source_dir / "Cargo.toml"), "license": "MIT", "license_file": None, "repository": "https://example.com/upstream"}
        self.metadata["packages"].append(package)
        self.locked.append({"name": name, "version": version, "source": source, "checksum": checksum})
        self.flush()
        return package, archive, commit

    def supplement(self, package, commit, path="LICENSE-MIT", text=b"Pinned MIT license\n", license_text=False):
        relative = "packaging/licenses/example-commit/" + path
        target = self.root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(text)
        locked = next(p for p in self.locked if p["name"] == package["name"] and p["version"] == package["version"])
        key = package["name"] + "-" + package["version"]
        self.overrides["packages"][key] = {
            **locked, "upstream_commit": commit,
            "files": [{"path": relative, "url": "https://example.com/" + commit + "/" + path, "sha256": hashlib.sha256(text).hexdigest(), "license_text": license_text}],
        }
        self.flush()
        return target

    def render(self):
        return notices.dependency_notices(self.metadata, root=self.root)

    def test_ordering_and_local_cache_paths_do_not_affect_output(self):
        self.package("zebra")
        self.package("alpha")
        first = self.render()
        self.metadata["packages"].reverse()
        self.assertEqual(first, self.render())
        self.assertLess(first.index("alpha-1.0.0"), first.index("zebra-1.0.0"))
        self.assertNotIn(str(self.root), first)
        manifest = notices.dependency_notice_manifest(self.metadata, root=self.root)
        self.assertNotIn("text", manifest["packages"][0]["texts"][0])
        self.assertIn("checksum", manifest["packages"][0])

    def test_missing_license_fails(self):
        self.package(files={"README.md": b"No license here"})
        with self.assertRaisesRegex(notices.NoticeError, "Missing license text"):
            self.render()

    def test_pinned_license_supplement_covers_missing_crate_license(self):
        package, _, commit = self.package(files={})
        self.supplement(package, commit)
        self.assertIn("Pinned MIT license", self.render())

    def test_override_content_tamper_fails(self):
        package, _, commit = self.package()
        target = self.supplement(package, commit)
        target.write_text("modified", encoding="utf-8")
        with self.assertRaisesRegex(notices.NoticeError, "Override content checksum mismatch"):
            self.render()

    def test_override_package_checksum_mismatch_fails(self):
        package, _, commit = self.package()
        self.supplement(package, commit)
        self.overrides["packages"]["example-1.0.0"]["checksum"] = "0" * 64
        self.flush()
        with self.assertRaisesRegex(notices.NoticeError, "Override source-package mismatch"):
            self.render()

    def test_override_source_mismatch_fails(self):
        package, _, commit = self.package()
        self.supplement(package, commit)
        self.overrides["packages"]["example-1.0.0"]["source"] = "registry+https://different.example/index"
        self.flush()
        with self.assertRaisesRegex(notices.NoticeError, "Override source-package mismatch"):
            self.render()

    def test_override_upstream_commit_mismatch_fails(self):
        package, _, commit = self.package()
        self.supplement(package, commit)
        self.overrides["packages"]["example-1.0.0"]["upstream_commit"] = "b" * 40
        self.flush()
        with self.assertRaisesRegex(notices.NoticeError, "Override upstream commit mismatch"):
            self.render()

    def test_override_unpinned_url_fails(self):
        package, _, commit = self.package()
        self.supplement(package, commit)
        self.overrides["packages"]["example-1.0.0"]["files"][0]["url"] = "https://example.com/main/LICENSE"
        self.flush()
        with self.assertRaisesRegex(notices.NoticeError, "Override source URL is not pinned"):
            self.render()

    def test_source_archive_checksum_mismatch_fails(self):
        _, archive, _ = self.package()
        archive.write_bytes(archive.read_bytes() + b"changed")
        with self.assertRaisesRegex(notices.NoticeError, "Cargo source checksum mismatch"):
            self.render()

    def test_changed_extracted_source_is_not_trusted(self):
        package, _, _ = self.package()
        (Path(package["manifest_path"]).parent / "LICENSE-MIT").write_text("UNTRUSTED", encoding="utf-8")
        self.assertNotIn("UNTRUSTED", self.render())
        self.assertIn("MIT license", self.render())

    def test_missing_source_archive_fails(self):
        _, archive, _ = self.package()
        archive.unlink()
        with self.assertRaisesRegex(notices.NoticeError, "Missing locked source archive"):
            self.render()

    def test_declared_license_file_with_custom_name_is_included(self):
        package, _, _ = self.package(files={"legal/terms.txt": b"Custom license terms"})
        package["license"] = None
        package["license_file"] = "legal/terms.txt"
        self.assertIn("Custom license terms", self.render())

    def test_authors_and_copyright_files_from_source_are_retained(self):
        self.package(files={"LICENSE-MIT": b"MIT license", "AUTHORS": b"Extra authors", "COPYRIGHT": b"Extra copyright"})
        self.assertIn("Extra authors", self.render())
        self.assertIn("Extra copyright", self.render())

    def test_copying_source_module_is_not_a_license(self):
        self.package(files={"LICENSE-MIT": b"MIT license", "src/copying.rs": b"PRIVATE_IMPLEMENTATION"})
        self.assertNotIn("PRIVATE_IMPLEMENTATION", self.render())

    def test_omitted_lock_package_and_extra_metadata_package_fail(self):
        self.package()
        original = copy.deepcopy(self.metadata)
        self.metadata["packages"].clear()
        with self.assertRaisesRegex(notices.NoticeError, "does not exactly match Cargo.lock"):
            self.render()
        self.metadata = original
        self.metadata["packages"][0]["version"] = "2.0.0"
        with self.assertRaisesRegex(notices.NoticeError, "does not exactly match Cargo.lock"):
            self.render()

    def test_stale_override_fails(self):
        self.package()
        self.overrides["packages"]["unused-1.0.0"] = {}
        self.flush()
        with self.assertRaisesRegex(notices.NoticeError, "Stale dependency-notice overrides"):
            self.render()

    def test_default_fonts_require_hack_and_other_font_notices(self):
        self.package("epaint_default_fonts")
        with self.assertRaisesRegex(notices.NoticeError, "Missing embedded default font notice"):
            self.render()

    def test_default_fonts_texts_are_included(self):
        self.package("epaint_default_fonts", files={**{p: ("Font license " + p).encode() for p in notices.DEFAULT_FONT_NOTICES}, "LICENSE-MIT": b"MIT"})
        rendered = self.render()
        for path in notices.DEFAULT_FONT_NOTICES:
            self.assertIn("Font license " + path, rendered)

    def test_project_font_notice_is_required(self):
        self.package()
        (self.root / notices.FONT_NOTICES[0]).unlink()
        with self.assertRaisesRegex(notices.NoticeError, "Missing notice file"):
            self.render()

    def test_authors_supplement_is_included_even_with_native_license(self):
        package, _, commit = self.package()
        self.supplement(package, commit, path="AUTHORS", text=b"Additional contributor attribution")
        self.assertIn("Additional contributor attribution", self.render())

    def test_authors_alone_requires_explicit_reviewed_license_marker(self):
        package, _, commit = self.package(files={})
        self.supplement(package, commit, path="AUTHORS", text=b"License inside AUTHORS")
        with self.assertRaisesRegex(notices.NoticeError, "Missing license text"):
            self.render()
        self.overrides["packages"]["example-1.0.0"]["files"][0]["license_text"] = True
        self.flush()
        self.assertIn("License inside AUTHORS", self.render())

    def test_archive_traversal_is_rejected(self):
        self.package(files={"../LICENSE": b"MIT"})
        with self.assertRaisesRegex(notices.NoticeError, "Unsafe Cargo archive member"):
            self.render()

    def test_override_cannot_escape_repository(self):
        package, _, commit = self.package()
        self.supplement(package, commit)
        self.overrides["packages"]["example-1.0.0"]["files"][0]["path"] = "../LICENSE"
        self.flush()
        with self.assertRaisesRegex(notices.NoticeError, "Unsafe notice path"):
            self.render()

    def test_packager_entry_point_invokes_only_locked_offline_metadata(self):
        self.package()
        with patch.object(notices.subprocess, "check_output", return_value=json.dumps(self.metadata)) as command:
            self.assertEqual(self.render(), notices.generate_notices(self.root))
        command.assert_called_once_with(["cargo", "metadata", "--format-version=1", "--locked", "--offline"], cwd=self.root, text=True)


if __name__ == "__main__":
    unittest.main()
