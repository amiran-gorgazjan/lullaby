import json
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch

from prepare_bundles import apple_version, prepare, stamp_plist


class BundlePreparationTests(unittest.TestCase):
    def test_prerelease_stamp_preserves_identity_and_records_full_version(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "Info.plist"
            path.write_bytes(plistlib.dumps({"CFBundleIdentifier": "audio.lullaby.resonance",
                                              "CFBundleVersion": "1.0.0", "AudioComponents": [{"version": 512}]}))
            stamp_plist(path, "0.2.0-beta.1")
            actual = plistlib.loads(path.read_bytes())
            self.assertEqual(actual["CFBundleIdentifier"], "audio.lullaby.resonance")
            self.assertEqual(actual["CFBundleVersion"], "0.2.0")
            self.assertEqual(actual["CFBundleShortVersionString"], "0.2.0")
            self.assertEqual(actual["LullabyVersion"], "0.2.0-beta.1")
            self.assertEqual(actual["AudioComponents"], [{"version": 512}])

    def test_unrepresentable_au_versions_are_rejected(self):
        for version in ("1.2", "1.256.0", "garbage", "1.2.3/other"):
            with self.subTest(version=version), self.assertRaises(ValueError):
                apple_version(version)

    def test_missing_attribution_never_mutates_or_resigns_existing_bundle(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            bundles = root / "bundled"
            for name in ("Lullaby.vst3", "Lullaby.clap"):
                (bundles / name).mkdir(parents=True)
            old = bundles / "LULLABY-LICENSES.txt"
            old.write_bytes(b"previous signed inputs")
            metadata = {"packages": [{"name": "lullaby", "version": "0.2.0-beta.1"}]}
            with patch("prepare_bundles.dependency_notices", side_effect=RuntimeError("missing notice")), \
                    patch("prepare_bundles.subprocess.run") as sign, self.assertRaises(RuntimeError):
                prepare(root, bundles, metadata, "darwin")
            self.assertEqual(old.read_bytes(), b"previous signed inputs")
            sign.assert_not_called()

    def test_mac_signing_observes_final_versions_and_complete_resources(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for name in ("LICENSE", "licenses/MIT-BASELINE.txt", "assets/fonts/CormorantGaramond-OFL.txt", "assets/fonts/Lato-OFL.txt"):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(name.encode())
            bundles = root / "bundled"
            for name in ("Lullaby.vst3", "Lullaby.clap"):
                path = bundles / name / "Contents/Info.plist"
                path.parent.mkdir(parents=True)
                path.write_bytes(plistlib.dumps({"CFBundleIdentifier": name}))
            metadata = {"packages": [{"name": "lullaby", "version": "0.2.0-beta.1"}]}
            manifest = {"schema_version": 1, "packages": []}

            def check_sign(args, **kwargs):
                bundle = Path(args[-1])
                self.assertEqual(plistlib.loads((bundle / "Contents/Info.plist").read_bytes())["LullabyVersion"], "0.2.0-beta.1")
                for name in ("FONT-LICENSES.txt", "LULLABY-LICENSES.txt", "THIRD-PARTY-NOTICES.txt", "DEPENDENCY-MANIFEST.json"):
                    self.assertEqual((bundles / name).read_bytes(), (bundle / "Contents/Resources" / name).read_bytes())

            with patch("prepare_bundles.dependency_notices", return_value="verified notices\n"), \
                    patch("prepare_bundles.dependency_notice_manifest", return_value=manifest), \
                    patch("prepare_bundles.subprocess.run", side_effect=check_sign) as sign:
                prepare(root, bundles, metadata, "darwin")
            self.assertEqual(sign.call_count, 4)
            self.assertEqual(json.loads((bundles / "DEPENDENCY-MANIFEST.json").read_bytes()), manifest)


if __name__ == "__main__":
    unittest.main()
