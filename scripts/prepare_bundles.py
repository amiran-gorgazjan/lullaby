#!/usr/bin/env python3
"""Stamp built plugin versions and complete license notices before final signing."""

import argparse
import json
from pathlib import Path
import plistlib
import re
import subprocess
import sys

from dependency_notices import cargo_metadata, dependency_notice_manifest, dependency_notices

ROOT = Path(__file__).resolve().parents[1]


def apple_version(version):
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?", version)
    if not match or any(int(part) > 255 for part in match.groups()):
        raise ValueError("Version cannot be represented by the AU component: " + version)
    return ".".join(match.groups())


def stamp_plist(path, version):
    if path.is_symlink() or not path.is_file():
        raise ValueError("Missing or unsafe bundle Info.plist: " + str(path))
    info = plistlib.loads(path.read_bytes())
    info.update(CFBundleVersion=apple_version(version),
                CFBundleShortVersionString=apple_version(version), LullabyVersion=version)
    path.write_bytes(plistlib.dumps(info, sort_keys=True))


def prepare(root, bundles=None, metadata=None, system=None):
    metadata = cargo_metadata(root) if metadata is None else metadata
    version = next(package["version"] for package in metadata["packages"] if package["name"] == "lullaby")
    bundles = Path(metadata["target_directory"]) / "bundled" if bundles is None else bundles
    system = sys.platform if system is None else system
    if bundles.is_symlink() or not bundles.is_dir():
        raise ValueError("Missing or unsafe bundle directory: " + str(bundles))
    for name in ("Lullaby.vst3", "Lullaby.clap"):
        path = bundles / name
        if path.is_symlink() or not path.exists():
            raise ValueError("Missing or unsafe plugin bundle: " + str(path))
        if name.endswith("vst3") or system == "darwin":
            if not path.is_dir():
                raise ValueError("Expected plugin bundle directory: " + str(path))
    # Fail before changing signed resources if exact dependency attribution is incomplete.
    third_party = dependency_notices(metadata, root=root).encode("utf-8")
    manifest = dependency_notice_manifest(metadata, root=root)
    fonts = b"Lullaby interface fonts\n\nCormorant Garamond\n" + (root / "assets/fonts/CormorantGaramond-OFL.txt").read_bytes()
    fonts += b"\n\nLato\n" + (root / "assets/fonts/Lato-OFL.txt").read_bytes()
    project = b"Lullaby project licensing\n\n" + (root / "LICENSE").read_bytes()
    project += b"\n\nPreviously distributed MIT baseline\n\n" + (root / "licenses/MIT-BASELINE.txt").read_bytes()
    notices = {
        "FONT-LICENSES.txt": fonts, "LULLABY-LICENSES.txt": project,
        "THIRD-PARTY-NOTICES.txt": third_party,
        "DEPENDENCY-MANIFEST.json": (json.dumps(manifest, sort_keys=True, indent=2) + "\n").encode("utf-8"),
    }
    for name, data in notices.items():
        (bundles / name).write_bytes(data)
    for name in ("Lullaby.vst3", "Lullaby.clap"):
        bundle = bundles / name
        if not bundle.is_dir():
            continue
        resources = bundle / "Contents/Resources"
        resources.mkdir(parents=True, exist_ok=True)
        for filename, data in notices.items():
            (resources / filename).write_bytes(data)
        if system == "darwin":
            stamp_plist(bundle / "Contents/Info.plist", version)
            subprocess.run(["codesign", "--force", "--sign", "-", str(bundle)], check=True)
            subprocess.run(["codesign", "--verify", "--deep", "--strict", str(bundle)], check=True)
    print("Prepared bundle versions and complete notices for Lullaby " + version)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundles", type=Path)
    args = parser.parse_args()
    prepare(ROOT, args.bundles)


if __name__ == "__main__":
    main()
