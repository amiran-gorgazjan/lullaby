"""Disposable CI-only investigation against an already built AU archive."""
import json
import os
from pathlib import Path
import pwd
import subprocess
import sys

import auval

auval.require_ci_refresh()
assert os.uname().machine == "x86_64"
source = Path(sys.argv[1]).resolve()
output = Path("target/au-discovery")
output.mkdir(parents=True, exist_ok=True)


def command(label, args, timeout=20):
    try:
        result = subprocess.run(args, capture_output=True, text=True, timeout=timeout, check=False)
        text = f"{args!r}\nexit={result.returncode}\n{result.stdout}\n{result.stderr}"
    except subprocess.TimeoutExpired as error:
        text = f"{args!r}\nTIMED OUT\n{error.stdout!r}\n{error.stderr!r}"
    (output / (label + ".txt")).write_text(text)
    print(label + ":\n" + text[-16000:], flush=True)


def snapshot(label, destination):
    print(label, "HOME", os.environ.get("HOME"), "pwd", pwd.getpwuid(os.getuid()), flush=True)
    for path in (Path.home(), destination.parent.parent.parent, destination.parent, destination,
                 destination / "Contents", destination / "Contents/Info.plist"):
        if path.exists():
            item = path.stat()
            print(str(path), oct(item.st_mode), item.st_uid, item.st_gid, item.st_mtime, flush=True)
    command(label + "-processes", ["ps", "-axo", "uid,pid,ppid,comm"])
    command(label + "-cache", ["ls", "-la", str(Path.home() / "Library/Caches/AudioUnitCache")])
    command(label + "-registrar", ["launchctl", "print", f"gui/{os.getuid()}/com.apple.audio.AudioComponentRegistrar"])


destination = Path.home() / "Library/Audio/Plug-Ins/Components/Lullaby.component"
command("plist", ["plutil", "-p", str(source / "Contents/Info.plist")])
command("attributes", ["xattr", "-lr", str(source)])
results = []


def attempt(label, mutation=None):
    if mutation:
        mutation()
    try:
        auval.wait_for_discovery(15)
        command(label + "-strict", ["auval", "-v", *auval.IDENTITY, "-strict"], timeout=90)
        results.append({"attempt": label, "discovered": True})
        return True
    except auval.DiscoveryTimeout as error:
        (output / (label + "-timeout.txt")).write_text(str(error))
        print(str(error), flush=True)
        results.append({"attempt": label, "discovered": False})
        return False


def touch_metadata():
    for path in (destination / "Contents/Info.plist", destination / "Contents", destination, destination.parent):
        os.utime(path, None)


with auval.temporary_component(source, destination):
    snapshot("installed", destination)
    success = attempt("baseline")
    if not success:
        success = attempt("metadata-touch", touch_metadata)
    if not success:
        success = attempt("current-user-refresh", auval.refresh_ci_registrar)
    if not success:
        tool = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
        success = attempt("explicit-bundle-registration", lambda: command("lsregister", [tool, "-f", str(destination)]))
    if not success:
        service = f"gui/{os.getuid()}/com.apple.audio.AudioComponentRegistrar"
        success = attempt("launchd-restart", lambda: command("launchd-kickstart", ["launchctl", "kickstart", "-kp", service]))
    snapshot("finished", destination)
    command("registrar-logs", ["log", "show", "--last", "10m", "--style", "compact", "--info", "--debug", "--predicate", 'process == "AudioComponentRegistrar" OR process == "auvaltool"'], timeout=30)
(output / "results.json").write_text(json.dumps(results, indent=2) + "\n")
print(json.dumps(results), flush=True)
