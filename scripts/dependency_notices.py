#!/usr/bin/env python3
"""Generate deterministic notices for locked Cargo archives and vendored source.

Python 3.9+, standard library only. No downloads are performed here: run Cargo's
locked dependency fetch first. This intentionally includes the complete lock
graph, including build tools and other platforms, rather than guessing which
copyrighted code survives linking.
"""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
HEX256 = re.compile(r"[0-9a-f]{64}\Z")
HEX160 = re.compile(r"[0-9a-f]{40}\Z")
FONT_NOTICES = (
    "assets/fonts/CormorantGaramond-OFL.txt",
    "assets/fonts/Lato-OFL.txt",
)
DEFAULT_FONT_NOTICES = (
    "fonts/Hack-Regular.txt",
    "fonts/OFL.txt",
    "fonts/UFL.txt",
    "fonts/emoji-icon-font-mit-license.txt",
)


class NoticeError(RuntimeError):
    """A release cannot provide complete, pinned attribution."""


def _sha(data):
    return hashlib.sha256(data).hexdigest()


def _text(data, label):
    try:
        text = data.decode("utf-8")
    except UnicodeError as error:
        raise NoticeError("Non-UTF-8 notice: " + label) from error
    if not text.strip():
        raise NoticeError("Empty notice: " + label)
    return text.replace("\r\n", "\n").replace("\r", "\n").rstrip() + "\n"


def _relative_file(root, name):
    path = PurePosixPath(name)
    if path.is_absolute() or ".." in path.parts or "\\" in name:
        raise NoticeError("Unsafe notice path: " + name)
    candidate = root / path
    if candidate.is_symlink() or not candidate.is_file():
        raise NoticeError("Missing notice file: " + name)
    if root.resolve() not in candidate.resolve().parents:
        raise NoticeError("Notice escapes project: " + name)
    return candidate


def _lock_packages(lock_path):
    """Read identity fields only; Cargo remains the authoritative TOML parser.

    Cargo.lock emits these fields as single-line basic strings. Keeping this
    narrow avoids requiring tomllib (Python 3.11) or a network-installed parser.
    Reject unknown encodings rather than interpreting an identity imprecisely.
    """
    entries = {}
    for block in lock_path.read_text(encoding="utf-8").split("[[package]]")[1:]:
        fields = {}
        for field in ("name", "version", "source", "checksum"):
            matches = re.findall(r"^" + field + r"\s*=\s*(.*?)\s*$", block, re.M)
            if len(matches) > 1:
                raise NoticeError("Duplicate Cargo.lock identity field: " + field)
            if matches:
                try:
                    fields[field] = json.loads(matches[0])
                except ValueError as error:
                    raise NoticeError("Unsupported Cargo.lock identity encoding: " + field) from error
                if not isinstance(fields[field], str):
                    raise NoticeError("Invalid Cargo.lock identity field: " + field)
        if "source" not in fields:
            continue
        if not fields.get("name") or not fields.get("version"):
            raise NoticeError("Incomplete Cargo.lock package identity")
        identity = (fields["name"], fields["version"], fields["source"])
        if identity in entries:
            raise NoticeError("Duplicate Cargo.lock package identity: " + repr(identity))
        entries[identity] = fields.get("checksum")
    return entries


def _notice_name(path):
    name = PurePosixPath(path).name.upper()
    # E.g. objc2-foundation's src/copying.rs implements object copying and is
    # ordinary source code, not a COPYING license. A declared license_file is
    # handled separately, so a deliberately named legal file remains covered.
    if PurePosixPath(path).suffix.lower() in (".rs", ".c", ".cc", ".cpp", ".h", ".hpp", ".py", ".js", ".ts", ".swift", ".m", ".mm"):
        return False
    return name.startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "OFL", "UFL")) or "LICENSE" in name


def _attribution_name(path):
    return _notice_name(path) or PurePosixPath(path).name.upper().startswith(("AUTHORS", "COPYRIGHT"))


def _source_notices(package, checksum):
    """Read legal texts from the checksum-verified .crate, never mutable sources."""
    name = package["name"] + "-" + package["version"]
    if not package["source"].startswith("registry+") or not checksum or not HEX256.fullmatch(checksum):
        raise NoticeError("Unsupported or unpinned Cargo source: " + name)
    directory = Path(package["manifest_path"]).parent
    if directory.name != name or len(directory.parents) < 3:
        raise NoticeError("Unexpected Cargo source layout: " + name)
    archive = directory.parents[2] / "cache" / directory.parent.name / (name + ".crate")
    if not archive.is_file():
        raise NoticeError("Missing locked source archive for " + name + "; run cargo fetch --locked")
    if _sha(archive.read_bytes()) != checksum:
        raise NoticeError("Cargo source checksum mismatch: " + name)
    found = {}
    upstream_commit = None
    try:
        with tarfile.open(archive, "r:gz") as source:
            for member in source.getmembers():
                parts = PurePosixPath(member.name).parts
                if not parts or parts[0] != name or ".." in parts or member.name.startswith("/") or "\\" in member.name:
                    raise NoticeError("Unsafe Cargo archive member: " + member.name)
                relative = PurePosixPath(*parts[1:]).as_posix()
                selected = _attribution_name(relative) or relative in DEFAULT_FONT_NOTICES or relative == package.get("license_file")
                if selected or relative == ".cargo_vcs_info.json":
                    if not member.isfile():
                        if member.isdir():
                            continue
                        raise NoticeError("Non-regular Cargo notice: " + member.name)
                    extracted = source.extractfile(member)
                    if extracted is None:
                        raise NoticeError("Unreadable Cargo notice: " + member.name)
                    data = extracted.read()
                    if relative == ".cargo_vcs_info.json":
                        upstream_commit = json.loads(data)["git"]["sha1"]
                    else:
                        if relative in found:
                            raise NoticeError("Duplicate Cargo notice: " + member.name)
                        found[relative] = data
    except (OSError, tarfile.TarError, ValueError, KeyError) as error:
        raise NoticeError("Invalid locked source archive: " + name) from error
    if package["name"] == "epaint_default_fonts":
        missing = sorted(set(DEFAULT_FONT_NOTICES) - set(found))
        if missing:
            raise NoticeError("Missing embedded default font notice: " + ", ".join(missing))
    license_file = package.get("license_file")
    if license_file and license_file not in found:
        raise NoticeError("Declared Cargo license_file is not included: " + name + ": " + license_file)
    return found, upstream_commit


def _vendored_source(package, override, root):
    manifest_path = override["vendored_manifest"]
    manifest = _relative_file(root, manifest_path)
    name = package["name"] + "-" + package["version"]
    if package.get("source") or Path(package["manifest_path"]).resolve() != manifest.resolve():
        raise NoticeError("Vendored Cargo manifest mismatch: " + name)
    if (override.get("name"), override.get("version")) != (package["name"], package["version"]):
        raise NoticeError("Vendored Cargo identity mismatch: " + name)
    if not override.get("source", "").startswith("registry+") or not HEX256.fullmatch(override.get("checksum", "")):
        raise NoticeError("Missing vendored Cargo archive provenance: " + name)

    directory = PurePosixPath(manifest_path).parent
    vcs_info = _relative_file(root, (directory / ".cargo_vcs_info.json").as_posix())
    upstream_commit = json.loads(vcs_info.read_bytes())["git"]["sha1"]
    license_path = (directory / "LICENSE").as_posix()
    license_data = _relative_file(root, license_path).read_bytes()

    # Hash the tracked build inputs independently of paths on the build machine.
    source_paths = [manifest, *sorted((manifest.parent / "src").rglob("*.rs"))]
    source_hashes = {
        path.relative_to(manifest.parent).as_posix():
        _sha(_relative_file(root, path.relative_to(root).as_posix()).read_bytes())
        for path in source_paths
    }
    checksum = _sha(json.dumps(source_hashes, sort_keys=True).encode("utf-8"))
    return "path+" + directory.as_posix(), checksum, upstream_commit, license_data


def _collect(metadata, *, root=ROOT, lock_path=None):
    root = Path(root)
    lock_path = Path(lock_path) if lock_path else root / "Cargo.lock"
    locked = _lock_packages(lock_path)
    external = [package for package in metadata["packages"] if package.get("source")]
    identities = [(p["name"], p["version"], p["source"]) for p in external]
    if len(identities) != len(set(identities)) or set(identities) != set(locked):
        missing = sorted(set(locked) - set(identities))
        extra = sorted(set(identities) - set(locked))
        raise NoticeError("Cargo metadata does not exactly match Cargo.lock (missing=%r, extra=%r)" % (missing, extra))
    override_path = _relative_file(root, "packaging/licenses/overrides.json")
    overrides_data = json.loads(override_path.read_text(encoding="utf-8"))
    if overrides_data.get("schema_version") != 1:
        raise NoticeError("Unsupported dependency-notice override schema")
    overrides = overrides_data["packages"]
    vendored = [
        package for package in metadata["packages"]
        if not package.get("source")
        and "vendored_manifest" in overrides.get(package["name"] + "-" + package["version"], {})
    ]
    used = set()
    records = []
    for package in sorted(external + vendored, key=lambda p: (p["name"], p["version"], p["source"] or "")):
        identity = (package["name"], package["version"], package["source"])
        name = package["name"] + "-" + package["version"]
        override = overrides.get(name)
        vendored_manifest = override and "vendored_manifest" in override
        source = package["source"]
        if vendored_manifest:
            source, checksum, upstream_commit, vendored_license = _vendored_source(package, override, root)
            files = {}
        else:
            checksum = locked[identity]
            files, upstream_commit = _source_notices(package, checksum)
        texts = []
        for path, data in sorted(files.items()):
            texts.append({"path": path, "sha256": _sha(data), "text": _text(data, name + "/" + path), "origin": "locked-crate", "license_text": path == package.get("license_file")})
        vendored_license_verified = False
        if override is not None:
            expected = (override.get("name"), override.get("version"), override.get("source"))
            if not vendored_manifest and (expected != identity or override.get("checksum") != checksum):
                raise NoticeError("Override source-package mismatch: " + name)
            if not upstream_commit or override.get("upstream_commit") != upstream_commit or not HEX160.fullmatch(upstream_commit):
                raise NoticeError("Override upstream commit mismatch: " + name)
            if name in used:
                raise NoticeError("Duplicate dependency-notice override use: " + name)
            used.add(name)
            for file in sorted(override["files"], key=lambda f: (f["path"], f["url"])):
                url = file["url"]
                if not url.startswith("https://") or upstream_commit not in url:
                    raise NoticeError("Override source URL is not pinned: " + name)
                data = _relative_file(root, file["path"]).read_bytes()
                if not HEX256.fullmatch(file.get("sha256", "")) or _sha(data) != file["sha256"]:
                    raise NoticeError("Override content checksum mismatch: " + file["path"])
                if vendored_manifest and _notice_name(file["path"]) and data == vendored_license:
                    vendored_license_verified = True
                texts.append({"path": file["path"], "url": url, "sha256": _sha(data), "text": _text(data, file["path"]), "origin": "pinned-supplement", "license_text": file.get("license_text", False) is True})
        if vendored_manifest and not vendored_license_verified:
            raise NoticeError("Vendored license differs from pinned upstream notice: " + name)
        # r-efi's reviewed, checksum-pinned AUTHORS actually contains its license.
        # An arbitrary attribution-only AUTHORS cannot stand in for license text.
        if not any(_notice_name(text["path"]) or text["path"].endswith("/MIT.txt") or text.get("license_text") for text in texts):
            raise NoticeError("Missing license text for " + name + "; add a pinned upstream notice")
        if not package.get("license") and not package.get("license_file"):
            raise NoticeError("Missing Cargo license declaration: " + name)
        records.append({
            "name": package["name"], "version": package["version"], "source": source,
            "checksum": checksum, "license": package.get("license"),
            "repository": package.get("repository"), "texts": texts,
        })
        if vendored_manifest:
            records[-1]["upstream_source"] = override["source"]
            records[-1]["upstream_checksum"] = override["checksum"]
    unused = sorted(set(overrides) - used)
    if unused:
        raise NoticeError("Stale dependency-notice overrides: " + ", ".join(unused))
    fonts = []
    for path in FONT_NOTICES:
        data = _relative_file(root, path).read_bytes()
        fonts.append({"path": path, "sha256": _sha(data), "text": _text(data, path)})
    return {"schema_version": 1, "cargo_lock_sha256": _sha(lock_path.read_bytes()), "overrides_sha256": _sha(override_path.read_bytes()), "packages": records, "interface_fonts": fonts}


def dependency_notice_manifest(metadata, *, root=ROOT, lock_path=None):
    """Return portable source identities and hashes, without local cache paths/text."""
    manifest = _collect(metadata, root=root, lock_path=lock_path)
    for package in manifest["packages"]:
        for notice in package["texts"]:
            del notice["text"]
    for notice in manifest["interface_fonts"]:
        del notice["text"]
    return manifest


def dependency_notices(metadata, *, root=ROOT, lock_path=None):
    """Generate complete notices; fail closed on an incomplete or changed graph."""
    manifest = _collect(metadata, root=root, lock_path=lock_path)
    chunks = [
        "Lullaby third-party notices",
        "Includes the complete locked Cargo graph: runtime dependencies, build tools,\n"
        "and dependencies for other platforms. Listed license alternatives remain\n"
        "alternatives. These third-party grants do not license Lullaby's own code.\n"
        "Separate AU wrapper notices are included in the macOS component resources.",
        "Cargo.lock SHA-256: " + manifest["cargo_lock_sha256"],
    ]
    for package in manifest["packages"]:
        if "upstream_checksum" in package:
            source_digest = (
                "Vendored source SHA-256: " + package["checksum"] + "\n"
                + "Upstream registry: " + package["upstream_source"] + "\n"
                + "Upstream Cargo archive SHA-256: " + package["upstream_checksum"] + "\n"
            )
        else:
            source_digest = "Cargo archive SHA-256: " + package["checksum"] + "\n"
        chunks += ["=" * 72,
            package["name"] + "-" + package["version"] + "\n"
            + "Source: " + package["source"] + "\n"
            + source_digest
            + "Declared license: " + (package["license"] or "See license_file") + "\n"
            + "Repository: " + (package["repository"] or "Not declared")]
        for notice in package["texts"]:
            label = notice.get("url", notice["path"])
            chunks += [label + "\nSHA-256: " + notice["sha256"], notice["text"].rstrip()]
    chunks += ["=" * 72, "Embedded Lullaby interface fonts"]
    for notice in manifest["interface_fonts"]:
        chunks += [notice["path"] + "\nSHA-256: " + notice["sha256"], notice["text"].rstrip()]
    return "\n\n".join(chunks) + "\n"


def cargo_metadata(root=ROOT):
    return json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version=1", "--locked", "--offline"], cwd=root, text=True,
    ))


def generate_notices(root=ROOT, metadata=None):
    """Archive packager entry point; no network access or mutable generated inputs."""
    return dependency_notices(cargo_metadata(root) if metadata is None else metadata, root=root)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--metadata", type=Path, help="Existing full locked Cargo metadata JSON")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, help="Optional portable source/hash manifest")
    args = parser.parse_args()
    metadata = json.loads(args.metadata.read_text(encoding="utf-8")) if args.metadata else cargo_metadata(args.root)
    text = dependency_notices(metadata, root=args.root)
    manifest = dependency_notice_manifest(metadata, root=args.root) if args.manifest else None
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8", newline="\n") as output:
        output.write(text)
    if args.manifest:
        args.manifest.parent.mkdir(parents=True, exist_ok=True)
        with args.manifest.open("w", encoding="utf-8", newline="\n") as output:
            output.write(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(args.output)


if __name__ == "__main__":
    main()
