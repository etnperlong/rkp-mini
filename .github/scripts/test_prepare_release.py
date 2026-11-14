#!/usr/bin/env python3
"""Tests for prepare_release.py against temp copies of the real manifests."""

import os
import re
import shutil
import sys
import tempfile
import tomllib
import unittest
from datetime import datetime, timezone
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import prepare_release

REPO = Path(__file__).resolve().parents[2]
CRATES = (
    "rkp-mini-issuer",
    "rkp-mini-core",
    "rkp-mini-runtime",
    "rkp-mini-sync",
    "rkp-mini-sync-core",
    "rkp-mini-deployer",
)
MODULES = ("issuer", "deployer")
SHA = "0123456789abcdef0123456789abcdef01234567"
NOW = datetime(2026, 11, 13, 12, 34, 56, tzinfo=timezone.utc)
EPOCH = 1794573296


class PrepareReleaseTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        shutil.copy2(REPO / "Cargo.toml", self.root / "Cargo.toml")
        shutil.copy2(REPO / "Cargo.lock", self.root / "Cargo.lock")
        for crate in CRATES:
            dest = self.root / "crates" / crate / "Cargo.toml"
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / "crates" / crate / "Cargo.toml", dest)
        for module in MODULES:
            dest = self.root / "modules" / module / "module.prop"
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(REPO / "modules" / module / "module.prop", dest)

    def tearDown(self):
        self._tmp.cleanup()

    def stamp(self, ref_type="branch", ref_name="main", sha=SHA, run_number="7", now=NOW):
        return prepare_release.prepare(self.root, ref_type, ref_name, sha, run_number, now)

    def read(self, relative):
        return (self.root / relative).read_text(encoding="utf-8")

    def tree(self):
        return {
            str(p.relative_to(self.root)): p.read_text(encoding="utf-8")
            for p in self.root.rglob("*")
            if p.is_file()
        }

    def crate_version(self, crate):
        return tomllib.loads(self.read(f"crates/{crate}/Cargo.toml"))["package"]["version"]

    def lock_version(self, name):
        found = [p for p in tomllib.loads(self.read("Cargo.lock"))["package"] if p["name"] == name]
        self.assertEqual(len(found), 1, name)
        return found[0]["version"]

    def prop(self, module, key):
        pattern = rf"(?m)^{key}=(.*)$"
        return re.search(pattern, self.read(f"modules/{module}/module.prop")).group(1)

    def assert_stamped(self, version, code):
        for crate in CRATES:
            self.assertEqual(self.crate_version(crate), version, crate)
            self.assertEqual(self.lock_version(crate), version, crate)
        for module in MODULES:
            self.assertEqual(self.prop(module, "version"), f"v{version}", module)
            self.assertEqual(self.prop(module, "versionCode"), code, module)

    def test_nightly_stamps_every_version(self):
        version = f"{self.crate_version('rkp-mini-issuer')}-nightly.20261113.7.g0123456"
        expected = {"version": version, "version_code": str(EPOCH), "prerelease": "true", "tag": "nightly"}
        self.assertEqual(self.stamp(), expected)
        self.assert_stamped(version, str(EPOCH))

    def test_stable_tag_beats_source_base(self):
        expected = {"version": "1.2.3", "version_code": str(EPOCH), "prerelease": "false", "tag": "v1.2.3"}
        self.assertEqual(self.stamp("tag", "v1.2.3"), expected)
        self.assert_stamped("1.2.3", str(EPOCH))

    def test_thirdparty_lock_entries_untouched(self):
        before = self.read("Cargo.lock").splitlines()
        self.stamp()
        after = self.read("Cargo.lock").splitlines()
        self.assertEqual(len(before), len(after))
        self.assertEqual(sum(a != b for a, b in zip(before, after)), len(CRATES))

    def test_malformed_inputs_fail_without_writing(self):
        before = self.tree()
        tags = ("1.2.3", "v1.2", "v1.2.3.4", "v01.2.3", "v1.02.3", "v1.2.03", "v1.2.3-rc.1")
        for ref_name in tags:
            with self.subTest(ref_name=ref_name), self.assertRaises(ValueError):
                self.stamp("tag", ref_name)
            self.assertEqual(self.tree(), before)
        for ref_type, ref_name in (("branch", "dev"), ("pull_request", "main")):
            with self.subTest(ref_type=ref_type), self.assertRaises(ValueError):
                self.stamp(ref_type, ref_name)
        for sha, run_number in (("abc", "7"), (SHA, "0"), (SHA, "01"), (SHA, "x")):
            with self.subTest(sha=sha, run_number=run_number), self.assertRaises(ValueError):
                self.stamp(sha=sha, run_number=run_number)
        self.assertEqual(self.tree(), before)

    def test_mismatched_base_fails_without_writing(self):
        path = self.root / "crates" / "rkp-mini-deployer" / "Cargo.toml"
        current = self.crate_version("rkp-mini-deployer")
        path.write_text(path.read_text(encoding="utf-8").replace(f'version = "{current}"', 'version = "0.0.0-mismatch"', 1), encoding="utf-8")
        before = self.tree()
        with self.assertRaises(ValueError):
            self.stamp()
        self.assertEqual(self.tree(), before)

    def test_bad_metadata_fails_without_writing(self):
        path = self.root / "modules" / "deployer" / "module.prop"
        path.write_text(re.sub(r"(?m)^versionCode=.*\n", "", path.read_text(encoding="utf-8")), encoding="utf-8")
        before = self.tree()
        with self.assertRaises(ValueError):
            self.stamp()
        self.assertEqual(self.tree(), before)

    def test_local_lock_package_with_source_fails_without_writing(self):
        path = self.root / "Cargo.lock"
        old = f'name = "rkp-mini-core"\nversion = "{self.crate_version("rkp-mini-core")}"\n'
        new = old + 'source = "registry+https://example.invalid"\n'
        path.write_text(path.read_text(encoding="utf-8").replace(old, new, 1), encoding="utf-8")
        before = self.tree()
        with self.assertRaises(ValueError):
            self.stamp()
        self.assertEqual(self.tree(), before)

    def test_main_writes_github_output(self):
        output = self.root / "github-output"
        env = {
            "GITHUB_REF_TYPE": "tag",
            "GITHUB_REF_NAME": "v2.0.0",
            "GITHUB_SHA": SHA,
            "GITHUB_RUN_NUMBER": "1",
            "GITHUB_OUTPUT": str(output),
        }
        with mock.patch.dict(os.environ, env, clear=True), mock.patch.object(prepare_release, "ROOT", self.root):
            self.assertEqual(prepare_release.main(), 0)
        written = dict(line.split("=", 1) for line in output.read_text(encoding="utf-8").splitlines())
        self.assertEqual((written["version"], written["prerelease"], written["tag"]), ("2.0.0", "false", "v2.0.0"))
        self.assertGreater(int(written["version_code"]), 0)
        self.assertEqual(self.crate_version("rkp-mini-issuer"), "2.0.0")

    def test_main_needs_github_output(self):
        with mock.patch.dict(os.environ, {}, clear=True):
            self.assertEqual(prepare_release.main(), 1)


if __name__ == "__main__":
    unittest.main()
