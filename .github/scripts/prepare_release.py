#!/usr/bin/env python3
"""Stamp the CI version into the workspace before a release build.

GitHub Actions only: the ref decides the version; all six crate manifests, the
local Cargo.lock packages and both module.prop files are rewritten. Every new
text is built and validated in memory, then written in one pass.
"""

import os
import re
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
STABLE = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)"
# Workspace package -> module directory whose module.prop it owns.
MODULE_DIRS = {
    "rkp-mini-issuer": "modules/issuer",
    "rkp-mini-deployer": "modules/deployer",
}


def _sub(text: str, pattern: str, replacement: str, where: str) -> str:
    new, found = re.subn(pattern, replacement, text, flags=re.MULTILINE)
    if found != 1:
        raise ValueError(f"{where}: expected one line matching {pattern!r}, found {found}")
    return new


def prepare(
    root: Path, ref_type: str, ref_name: str, sha: str, run_number: str, now: datetime
) -> dict[str, str]:
    """Validate the ref, stamp every version file, and return the action outputs."""
    if now.tzinfo is None:
        raise ValueError("timestamp must be timezone-aware")
    if re.fullmatch(r"[0-9a-f]{40}", sha) is None:
        raise ValueError(f"GITHUB_SHA is not a full commit id: {sha!r}")
    if re.fullmatch(r"[1-9][0-9]*", run_number) is None:
        raise ValueError(f"GITHUB_RUN_NUMBER is not a positive integer: {run_number!r}")
    if ref_type == "branch" and ref_name != "main":
        raise ValueError(f"unsupported branch: {ref_name!r}")
    if ref_type == "tag" and re.fullmatch("v" + STABLE, ref_name) is None:
        raise ValueError(f"tag must be canonical vX.Y.Z: {ref_name!r}")
    if ref_type not in ("branch", "tag"):
        raise ValueError(f"unsupported ref type: {ref_type!r}")

    now = now.astimezone(timezone.utc)
    code = int(now.timestamp())
    # shortcut: signed-32-bit Unix versionCode lasts until 2038; change the scheme before then.
    if not 0 < code <= 2**31 - 1:
        raise ValueError(f"UTC timestamp is not a positive signed 32-bit integer: {code}")

    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    staged: dict[Path, str] = {}
    versions: dict[str, str] = {}
    for member in workspace["workspace"]["members"]:
        path = root / member / "Cargo.toml"
        text = path.read_text(encoding="utf-8")
        package = tomllib.loads(text)["package"]
        versions[package["name"]] = package["version"]
        staged[path] = text

    if ref_type == "tag":
        version = ref_name[1:]
    else:
        base, deployer = versions.get("rkp-mini-issuer"), versions.get("rkp-mini-deployer")
        if base is None or deployer is None:
            raise ValueError("workspace is missing rkp-mini-issuer or rkp-mini-deployer")
        if base != deployer:
            raise ValueError(f"issuer/deployer versions differ: {base!r} vs {deployer!r}")
        if re.fullmatch(STABLE, base) is None:
            raise ValueError(f"workspace base version is not canonical X.Y.Z: {base!r}")
        version = f"{base}-nightly.{now:%Y%m%d}.{run_number}.g{sha[:7]}"

    for path, text in staged.items():
        staged[path] = _sub(text, r'^version = "[^"]*"', f'version = "{version}"', str(path))

    for module_dir in MODULE_DIRS.values():
        path = root / module_dir / "module.prop"
        text = path.read_text(encoding="utf-8")
        current = re.search(r"^versionCode=([0-9]+)$", text, flags=re.MULTILINE)
        if current is None:
            raise ValueError(f"{path}: versionCode is missing or not numeric")
        if code <= int(current.group(1)):
            raise ValueError(f"{path}: versionCode {current.group(1)} is not below {code}")
        text = _sub(text, r"^version=.*$", f"version=v{version}", str(path))
        staged[path] = _sub(text, r"^versionCode=.*$", f"versionCode={code}", str(path))

    lock = root / "Cargo.lock"
    text = lock.read_text(encoding="utf-8")
    local = [p for p in tomllib.loads(text)["package"] if p["name"] in versions]
    if sorted(p["name"] for p in local) != sorted(versions):
        raise ValueError(f"{lock}: local packages do not match the workspace")
    for package in local:
        if "source" in package:
            raise ValueError(f"{lock}: local package {package['name']} has a source")
    for name in versions:
        pattern = rf'^name = "{re.escape(name)}"\nversion = "[^"]*"'
        text = _sub(text, pattern, f'name = "{name}"\nversion = "{version}"', f"{lock}: {name}")
    staged[lock] = text

    for path, text in staged.items():
        path.write_text(text, encoding="utf-8")

    return {
        "version": version,
        "version_code": str(code),
        "prerelease": "true" if ref_type == "branch" else "false",
        "tag": f"v{version}" if ref_type == "branch" else ref_name,
    }


def main() -> int:
    env = os.environ
    output = env.get("GITHUB_OUTPUT")
    if not output:
        print("prepare_release: GITHUB_OUTPUT is not set", file=sys.stderr)
        return 1
    try:
        outputs = prepare(
            ROOT,
            env["GITHUB_REF_TYPE"],
            env["GITHUB_REF_NAME"],
            env["GITHUB_SHA"],
            env["GITHUB_RUN_NUMBER"],
            datetime.now(timezone.utc),
        )
    except (KeyError, OSError, ValueError) as exc:
        print(f"prepare_release: {exc}", file=sys.stderr)
        return 1
    with open(output, "a", encoding="utf-8") as handle:
        handle.writelines(f"{key}={value}\n" for key, value in outputs.items())
    print(f"prepare_release: stamped {outputs['version']} ({outputs['version_code']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
