#!/usr/bin/env python3
"""Temporarily install a built AU for Apple's validator; never overwrite an installed copy."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("component", nargs="?", type=Path,
                        default=Path(__file__).resolve().parents[1] / "target/bundled/Lullaby.component")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("auval requires macOS")
    source = args.component.resolve()
    if not (source / "Contents/Info.plist").is_file():
        parser.error(f"No built component at {source}; run cargo xtask bundle-au lullaby --release --locked")
    destination = Path.home() / "Library/Audio/Plug-Ins/Components/Lullaby.component"
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Reserve the destination exclusively, including protection against dangling symlinks.
    try:
        destination.mkdir()
    except FileExistsError:
        parser.error(f"Refusing to overwrite {destination}; validate the installed copy with auval directly")
    try:
        shutil.copytree(source, destination, dirs_exist_ok=True)
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(destination)], check=True)
        # AudioComponentRegistrar discovers new bundles asynchronously. Touch the
        # completed bundle (copytree preserves old mtimes) and wait for discovery.
        os.utime(destination, None)
        deadline = time.monotonic() + 20
        while True:
            listing = subprocess.run(["auval", "-l"], capture_output=True, text=True, check=True).stdout
            if any(all(code in line.split() for code in ("aufx", "Lulb", "Llby"))
                   for line in listing.splitlines()):
                break
            if time.monotonic() >= deadline:
                sys.exit("AudioComponentRegistrar did not discover the temporary component within 20 seconds")
            time.sleep(0.5)
        return subprocess.run(["auval", "-v", "aufx", "Lulb", "Llby", "-strict"], check=False).returncode
    finally:
        shutil.rmtree(destination)


if __name__ == "__main__":
    sys.exit(main())
