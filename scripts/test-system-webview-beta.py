#!/usr/bin/env python3
"""Synthetic packaging checks: no keyring, forge or user repositories."""
import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import sys
from unittest.mock import patch

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location(
    "beta", Path(__file__).with_name("package-system-webview-beta.py")
)
beta = importlib.util.module_from_spec(spec)
spec.loader.exec_module(beta)


class BetaPackagingTests(unittest.TestCase):
    def test_identity_matches_the_compiled_config(self):
        config = json.loads((beta.ROOT / "crates/gitnexia-app/tauri.beta.conf.json").read_text())
        plist = beta.macos_plist("0.2.0")
        self.assertEqual(config["identifier"], plist["CFBundleIdentifier"])
        self.assertEqual(config["productName"], plist["CFBundleName"])
        self.assertNotEqual(config["identifier"], "com.gitnexia.desktop")
        self.assertEqual(plist["CFBundleExecutable"], "GitNexiaBeta")
        self.assertEqual(plist["LSMinimumSystemVersion"], "13.3")

    def test_only_the_beta_binary_and_required_notices_are_staged(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "gitnexia"
            binary.write_bytes(b"synthetic executable")
            notices = root / "notices"
            notices.mkdir()
            for name in (*beta.NOTICES, "CEF-LICENSE.txt"):
                (notices / name).write_text("synthetic license")
            stage = root / "stage"
            beta.stage_files(binary, stage, notices)
            self.assertEqual((stage / "GitNexiaBeta.exe").read_bytes(), binary.read_bytes())
            self.assertEqual(sorted(p.name for p in (stage / "licenses").iterdir()), sorted(beta.NOTICES))
            self.assertIn("Automatic updates are disabled", (stage / "BETA.txt").read_text())
            with self.assertRaises(FileExistsError):
                beta.stage_files(binary, stage, notices)

    def test_missing_notices_fail_packaging(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "gitnexia"
            binary.write_bytes(b"synthetic executable")
            with self.assertRaisesRegex(ValueError, "Missing license notice"):
                beta.stage_files(binary, root / "stage", root / "missing")

    def test_windows_package_has_a_checksum_and_no_staging_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "target"
            binary = target / "release/gitnexia.exe"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"synthetic executable")
            notices = root / "ui/dist/licenses"
            notices.mkdir(parents=True)
            for name in beta.NOTICES:
                (notices / name).write_text("synthetic license")
            out = target / "dist"
            out.mkdir()

            def compile_installer(command, check):
                self.assertTrue(check)
                self.assertEqual(command[0], "test-makensis")
                destination = next(arg.removeprefix("/DOUTFILE=") for arg in command if arg.startswith("/DOUTFILE="))
                Path(destination).write_bytes(b"synthetic installer")

            with patch.object(beta, "ROOT", root), patch.object(beta.sys, "platform", "win32"), patch.object(beta.subprocess, "run", compile_installer):
                beta.package("0.2.0", 42, target, out, "test-makensis")
            name = "GitNexiaBeta_0.2.0_42_x64-setup.exe"
            digest = hashlib.sha256(b"synthetic installer").hexdigest()
            self.assertEqual((out / "SHA256SUMS").read_text(), f"{digest}  {name}\n")
            self.assertEqual(sorted(p.name for p in out.iterdir()), ["BETA.txt", name, "SHA256SUMS"])


if __name__ == "__main__":
    unittest.main()
