#!/usr/bin/env python3
"""Archive built plugins, installation instructions and dependency notices."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def dependency_notices(metadata):
    overrides = json.loads((ROOT / "packaging/licenses/overrides.json").read_text())
    notices = [
        "Lullaby third-party notices",
        "This file includes notices for the locked Cargo dependency graph, including\n"
        "build tools and dependencies for other platforms. License alternatives\n"
        "remain alternatives; including their texts does not change Lullaby's MIT license.",
    ]
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if not package["source"]:
            continue
        directory = Path(package["manifest_path"]).parent
        name = package["name"] + "-" + package["version"]
        files = sorted(
            p for p in directory.rglob("*")
            if p.is_file() and (
                p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "OFL", "UFL"))
                or "license" in p.name.lower()
            )
        )
        # Hack's copyright and license are in a file named after the font.
        if package["name"] == "epaint_default_fonts":
            files += [directory / "fonts/Hack-Regular.txt"]
        texts = [(str(p.relative_to(directory)), p.read_text(encoding="utf-8")) for p in files]
        if not texts:
            for override in overrides.get(name, []):
                texts.append((override["url"], (ROOT / override["path"]).read_text(encoding="utf-8")))
        if not texts:
            raise RuntimeError(f"Missing license text for {name}; add a pinned upstream notice")
        notices += [
            "=" * 72,
            f"{name}\nLicense: {package['license']}\nRepository: {package['repository']}",
        ]
        for source, contents in texts:
            notices += [source, contents]
    return "\n\n".join(notices) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", required=True, help="e.g. Windows-X64, macOS-universal, Linux-X64")
    args = parser.parse_args()
    if not all(c.isalnum() or c in "-_" for c in args.platform):
        parser.error("platform may contain only letters, digits, hyphens and underscores")
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version=1", "--locked", "--offline"], cwd=ROOT, text=True,
    ))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "lullaby")
    target = Path(metadata["target_directory"])
    bundles = target / "bundled"
    for name in ("Lullaby.vst3", "Lullaby.clap", "FONT-LICENSES.txt"):
        if not (bundles / name).exists():
            raise RuntimeError(f"Missing {name}; build the plugin bundles first")
    if args.platform.startswith("macOS") and not (bundles / "Lullaby.component").is_dir():
        raise RuntimeError("macOS releases must also contain the AUv2 component")
    output = target / "release-assets"
    output.mkdir(exist_ok=True)
    filename = f"Lullaby-{version}-{args.platform}.zip"
    archive = output / filename
    prefix = filename.removesuffix(".zip")
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zipped:
        for path in sorted(bundles.rglob("*")):
            if path.is_file():
                zipped.write(path, f"{prefix}/{path.relative_to(bundles).as_posix()}")
        for name, source in (("LICENSE", "LICENSE"), ("README.md", "README.md"), ("INSTALL.md", "docs/install.md")):
            zipped.write(ROOT / source, f"{prefix}/{name}")
        zipped.writestr(f"{prefix}/THIRD-PARTY-NOTICES.txt", dependency_notices(metadata))
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_suffix(".zip.sha256").write_text(f"{checksum}  {filename}\n", encoding="utf-8")
    print(archive)


if __name__ == "__main__":
    main()
