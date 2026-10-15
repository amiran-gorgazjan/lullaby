#!/usr/bin/env python3
"""Build a self-contained AUv2 from Lullaby's CLAP (macOS, Python 3.9+)."""

import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def run(*args, capture=False, env=None):
    print("+", " ".join(map(str, args)), flush=True)
    return subprocess.run(
        list(map(str, args)), cwd=ROOT, check=True, text=True,
        stdout=subprocess.PIPE if capture else None, env=env,
    ).stdout


def dependencies(offline):
    cache = ROOT / ".cache/au"
    pins = json.loads((ROOT / "packaging/au/dependencies.json").read_text())
    for name, pin in pins.items():
        path = cache / name
        if not path.exists():
            if offline:
                raise RuntimeError(f"Missing {path}; run once without --offline to fetch AU dependencies")
            cache.mkdir(parents=True, exist_ok=True)
            run("git", "clone", "--depth", "1", "--branch", pin["tag"], pin["url"], path)
        revision = run("git", "-C", path, "rev-parse", "HEAD", capture=True).strip()
        dirty = run("git", "-C", path, "status", "--porcelain", capture=True).strip()
        if revision != pin["commit"] or dirty:
            raise RuntimeError(f"{path} must be clean at pinned commit {pin['commit']}; move it aside and retry")
    return cache


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("package", nargs="?", default="lullaby", choices=["lullaby"])
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--locked", action="store_true")
    parser.add_argument("--offline", action="store_true", help="Use cached Cargo and AU dependencies only")
    parser.add_argument("--universal", action="store_true", help="Build arm64 + x86_64 (requires both Rust targets)")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("AU builds require macOS; use `cargo xtask bundle lullaby` for VST3/CLAP on other OSes")
    for tool in ("cargo", "cmake", "git", "patch", "xcrun", "codesign"):
        if not shutil.which(tool):
            raise RuntimeError(f"Missing {tool}; see docs/audio-unit.md for build prerequisites")
    cache = dependencies(args.offline)
    env = os.environ.copy()
    # Match Rust and C++ deployment targets. egui's macOS backend requires 10.15+;
    # 11.0 is the common baseline for Intel and Apple Silicon bundles.
    env.setdefault("MACOSX_DEPLOYMENT_TARGET", "11.0")
    flags = [flag for flag in ("--release", "--locked", "--offline") if getattr(args, flag[2:])]
    run("cargo", "xtask", "bundle-universal" if args.universal else "bundle", "lullaby", *flags, env=env)
    metadata = json.loads(run("cargo", "metadata", "--no-deps", "--format-version=1", "--offline", capture=True, env=env))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "lullaby")
    target = Path(metadata["target_directory"])
    clap = target / "bundled/Lullaby.clap"
    archs = run("lipo", "-archs", clap / "Contents/MacOS/Lullaby", capture=True).split()
    expected = {"arm64", "x86_64"} if args.universal else {"arm64" if platform.machine() == "arm64" else "x86_64"}
    if set(archs) != expected:
        raise RuntimeError(f"Expected CLAP architectures {expected}, found {archs}; unset cross-compilation overrides")
    build = target / "au" / ("universal" if args.universal else platform.machine()) / ("release" if args.release else "debug")
    patched = build / "wrapper-src/src/wrapasauv2.cpp"
    patched.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(cache / "clap-wrapper/src/wrapasauv2.cpp", patched)
    run("patch", "--batch", "--forward", "-p1", "-d", build / "wrapper-src",
        "-i", ROOT / "packaging/au/clap-wrapper.patch")
    config = "Release" if args.release else "Debug"
    run("cmake", "-S", ROOT / "packaging/au", "-B", build, "-G", "Unix Makefiles",
        f"-DCMAKE_BUILD_TYPE={config}", f"-DCMAKE_OSX_ARCHITECTURES={';'.join(sorted(archs))}",
        f"-DCMAKE_OSX_DEPLOYMENT_TARGET={env['MACOSX_DEPLOYMENT_TARGET']}",
        f"-DLULLABY_AU_DEPS={cache}", f"-DLULLABY_CLAP={clap}", f"-DLULLABY_VERSION={version}", env=env)
    run("cmake", "--build", build, "--config", config, "--parallel", str(os.cpu_count() or 2), env=env)
    component = build / "Lullaby.component"
    embedded = component / "Contents/PlugIns/Lullaby.clap"
    # Also refresh on no-op CMake builds and remove obsolete files from the nested bundle.
    if embedded.exists():
        shutil.rmtree(embedded)
    shutil.copytree(clap, embedded)
    # nice-plug-xtask 0.1.1 emits a placeholder version; correct the embedded copy.
    info_path = embedded / "Contents/Info.plist"
    info = plistlib.loads(info_path.read_bytes())
    info.update(CFBundleVersion=version, CFBundleShortVersionString=version)
    info_path.write_bytes(plistlib.dumps(info))
    resources = component / "Contents/Resources"
    resources.mkdir(exist_ok=True)
    notices = ["Lullaby AUv2 third-party notices", (ROOT / "LICENSE").read_text()]
    notices += ["Previously distributed MIT baseline", (ROOT / "licenses/MIT-BASELINE.txt").read_text()]
    shutil.copyfile(embedded / "Contents/Resources/LULLABY-LICENSES.txt", resources / "LULLABY-LICENSES.txt")
    notices += ["Interface fonts", (embedded / "Contents/Resources/FONT-LICENSES.txt").read_text()]
    for name, license_file in (("clap-wrapper", "LICENSE"), ("clap", "LICENSE"), ("AudioUnitSDK", "LICENSE.txt")):
        notices += [name, (cache / name / license_file).read_text()]
    # fmt is bundled in clap-wrapper and carries its MIT notice in the header.
    fmt = (cache / "clap-wrapper/libs/fmt/fmt/format.h").read_text()
    notices += ["fmt", fmt[:fmt.index("#ifndef")]]
    (resources / "THIRD-PARTY-NOTICES.txt").write_text("\n\n".join(notices))
    run("codesign", "--force", "--sign", "-", embedded)
    run("codesign", "--force", "--sign", "-", component)
    run("codesign", "--verify", "--deep", "--strict", component)
    # Exercise the final signed bundle directly, without installing it globally.
    run(build / "lullaby_au_smoke", component)
    output = target / "bundled/Lullaby.component"
    if output.exists():
        shutil.rmtree(output)
    shutil.copytree(component, output)
    print(f"\nBuilt and smoke-tested {output}\nAUv2 identity: aufx / Lulb / Llby; version {version}")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
