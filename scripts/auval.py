#!/usr/bin/env python3
"""Temporarily install a built AU for Apple's validator; never overwrite an installed copy."""

import argparse
from contextlib import contextmanager
import ctypes
import math
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time

IDENTITY = ("aufx", "Lulb", "Llby")


class DiscoveryTimeout(RuntimeError):
    """The registrar has not listed the temporary AU within its time budget."""


def rename_exclusive(source, destination):
    """Publish a complete sibling bundle atomically without replacing any path."""
    # Darwin's RENAME_EXCL rejects files, directories and dangling symlinks.
    # Do not fall back to os.rename: its overwrite behavior loses this guarantee.
    library = ctypes.CDLL(None, use_errno=True)
    rename = library.renamex_np
    rename.argtypes = (ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint)
    rename.restype = ctypes.c_int
    if rename(os.fsencode(source), os.fsencode(destination), 0x00000004) != 0:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error), str(destination))


def same_directory(path, identity):
    try:
        actual = path.lstat()
    except FileNotFoundError:
        return False
    return stat.S_ISDIR(actual.st_mode) and (actual.st_dev, actual.st_ino) == identity


@contextmanager
def temporary_component(source, destination):
    """Expose only a complete verified bundle, and clean up only our own copy."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    if os.path.lexists(destination):
        raise FileExistsError(f"Refusing to overwrite {destination}; validate the installed copy with auval directly")
    # The registrar scans .component entries. A hidden sibling without that suffix
    # keeps an empty/partially copied bundle out of its discovery cache.
    staging = Path(tempfile.mkdtemp(prefix=".lullaby-auval-", dir=destination.parent))
    staged = staging.lstat()
    identity = (staged.st_dev, staged.st_ino)
    published = False
    try:
        shutil.copytree(source, staging, dirs_exist_ok=True)
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(staging)], check=True)
        rename_exclusive(staging, destination)
        published = True
        if not same_directory(destination, identity):
            raise RuntimeError("Temporary Audio Unit was replaced during publication: " + str(destination))
        # copytree preserves old bundle mtimes. Notify both the completed bundle
        # and its scan directory after publication, never before it is complete.
        os.utime(destination, None, follow_symlinks=False)
        os.utime(destination.parent, None)
        yield destination
    finally:
        if published:
            if same_directory(destination, identity):
                shutil.rmtree(destination)
                os.utime(destination.parent, None)
        elif same_directory(staging, identity):
            shutil.rmtree(staging)


def wait_for_discovery(timeout=60.0):
    """Wait a bounded time for asynchronous registration and retain diagnostics."""
    if not math.isfinite(timeout) or timeout <= 0:
        raise ValueError("Discovery timeout must be a positive finite number")
    deadline = time.monotonic() + timeout
    last_stdout, last_stderr, last_status = "", "", "not run"
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise DiscoveryTimeout(
                f"AudioComponentRegistrar did not discover {'/'.join(IDENTITY)} within {timeout:g} seconds. "
                f"Last auval -l status: {last_status}.\n"
                f"Last listing:\n{last_stdout[-4000:] or '<empty>'}\n"
                f"Last stderr:\n{last_stderr[-4000:] or '<empty>'}"
            )
        try:
            result = subprocess.run(["auval", "-l"], capture_output=True, text=True,
                                    check=False, timeout=min(10.0, remaining))
            last_stdout, last_stderr, last_status = result.stdout, result.stderr, str(result.returncode)
            if result.returncode == 0 and any(tuple(line.split()[:3]) == IDENTITY
                                              for line in result.stdout.splitlines()):
                return
        except subprocess.TimeoutExpired:
            last_status = "listing timed out"
        remaining = deadline - time.monotonic()
        if remaining > 0:
            time.sleep(min(0.5, remaining))


def require_ci_refresh():
    if sys.platform != "darwin" or os.environ.get("GITHUB_ACTIONS") != "true":
        raise RuntimeError("--ci-refresh-registrar is allowed only in a macOS GitHub Actions job")


def refresh_ci_registrar():
    """Restart only this CI user's launchd service and verify a new process."""
    require_ci_refresh()
    uid = os.getuid()
    service = f"gui/{uid}/com.apple.audio.AudioComponentRegistrar"
    previous = subprocess.run(["launchctl", "print", service],
                              capture_output=True, text=True, check=False, timeout=10)
    match = re.search(r"^\s*pid = ([1-9][0-9]*)\s*$", previous.stdout, re.MULTILINE) if previous.returncode == 0 else None
    old_pid = int(match.group(1)) if match else None
    # SIGTERM can leave the registrar running with outstanding transactions.
    # kickstart -k forces a real restart; -p reports the replacement PID. The
    # GUI-domain target excludes root and other users' registrars.
    result = subprocess.run(["launchctl", "kickstart", "-kp", service],
                            capture_output=True, text=True, check=False, timeout=10)
    if result.returncode != 0:
        raise RuntimeError(f"CI registrar restart failed (exit {result.returncode}): {result.stderr.strip()[:2000]}")
    if not re.fullmatch(r"[1-9][0-9]*", result.stdout.strip()):
        raise RuntimeError("CI registrar restart did not report a valid process ID")
    new_pid = int(result.stdout.strip())
    if new_pid == old_pid:
        raise RuntimeError(f"CI registrar restart left the same process running: {new_pid}")
    print(f"CI-only registrar restart for UID {uid}: PID {old_pid or '<not running>'} -> {new_pid}", flush=True)


def validate(source, destination, discovery_timeout=60.0, ci_refresh_registrar=False):
    if ci_refresh_registrar:
        require_ci_refresh()
    with temporary_component(source, destination):
        try:
            wait_for_discovery(discovery_timeout)
        except DiscoveryTimeout as error:
            if not ci_refresh_registrar:
                raise
            print(str(error), flush=True)
            refresh_ci_registrar()
            wait_for_discovery(discovery_timeout)
        return subprocess.run(["auval", "-v", *IDENTITY, "-strict"], check=False).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("component", nargs="?", type=Path,
                        default=Path(__file__).resolve().parents[1] / "target/bundled/Lullaby.component")
    parser.add_argument("--discovery-timeout", type=float, default=60.0,
                        help="Maximum seconds for asynchronous AU registration (default: 60)")
    parser.add_argument("--ci-refresh-registrar", action="store_true",
                        help="GitHub Actions only: refresh this UID\'s registrar once after discovery times out")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("auval requires macOS")
    if not math.isfinite(args.discovery_timeout) or args.discovery_timeout <= 0:
        parser.error("--discovery-timeout must be a positive finite number")
    source = args.component.resolve()
    if not (source / "Contents/Info.plist").is_file():
        parser.error(f"No built component at {source}; run cargo xtask bundle-au lullaby --release --locked")
    destination = Path.home() / "Library/Audio/Plug-Ins/Components/Lullaby.component"
    try:
        return validate(source, destination, args.discovery_timeout, args.ci_refresh_registrar)
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        parser.exit(1, str(error) + "\n")


if __name__ == "__main__":
    sys.exit(main())
