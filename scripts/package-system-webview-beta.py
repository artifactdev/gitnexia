#!/usr/bin/env python3
"""Package an already-built, isolated system-webview beta without the CEF bundler."""
import argparse
import hashlib
import json
import platform
import plistlib
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
APP = "GitNexia Beta"
IDENTIFIER = "com.gitnexia.beta"
NOTICES = ("LICENSE.txt", "THIRD-PARTY-NOTICES-rust.txt", "THIRD-PARTY-NOTICES-ui.txt")
WARNING = """GitNexia Beta - experimental system-webview build

Use disposable test repositories, not your only copy of important work.
Settings, accounts and UI storage are separate from regular GitNexia.
Automatic updates are disabled. Download and install later betas manually.
The app and installer are not Developer ID signed/notarized or Windows signed.

Windows needs the Microsoft WebView2 Evergreen Runtime already installed.
macOS needs 13.3 or later (Apple Silicon); some newer CSS may render differently.
No browser runtime or spelling dictionary is included.

The regular CEF build's verified network-blocking and sandbox guarantees do not
apply to this experimental build. It uses the system WebKit/WebView2 runtime.
The app's Content Security Policy remains enabled, but runtime/OS networking
(including spelling and runtime updates) has not been audited for this beta.
Review PRIVACY.md and SECURITY.md in the source repository before testing.
"""


def stage_files(binary, stage, notices):
    stage.mkdir(parents=True, exist_ok=False)
    if not binary.is_file():
        raise ValueError(f"Missing beta binary: {binary}")
    shutil.copy2(binary, stage / "GitNexiaBeta.exe")
    (stage / "BETA.txt").write_text(WARNING, encoding="utf-8")
    dest = stage / "licenses"
    dest.mkdir()
    for name in NOTICES:
        source = notices / name
        if not source.is_file() or source.stat().st_size == 0:
            raise ValueError(f"Missing license notice: {source}")
        shutil.copy2(source, dest / name)


def macos_plist(version):
    return {
        "CFBundleName": APP,
        "CFBundleDisplayName": APP,
        "CFBundleIdentifier": IDENTIFIER,
        "CFBundleExecutable": "GitNexiaBeta",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "CFBundleIconFile": "icon.icns",
        "LSMinimumSystemVersion": "13.3",
        "NSHighResolutionCapable": True,
    }


def package(version, build_id, target, out, makensis):
    # A fresh per-run folder avoids picking up a regular build or old package.
    work = out / f"work-{build_id}"
    if work.exists():
        raise ValueError(f"Staging folder already exists: {work}")
    stage = work / APP
    windows = sys.platform == "win32"
    binary = target / "release" / ("gitnexia.exe" if windows else "gitnexia")
    stage_files(binary, stage, ROOT / "ui/dist/licenses")
    stem = f"GitNexiaBeta_{version}_{build_id}"
    if windows:
        installer = out / f"{stem}_x64-setup.exe"
        subprocess.run([
            makensis, "/V2", f"/DSTAGE={stage}", f"/DOUTFILE={installer}",
            f"/DVERSION={version}", f"/DICON={ROOT / 'crates/gitnexia-app/icons/icon.ico'}",
            str(ROOT / "packaging/windows/gitnexia-beta.nsi"),
        ], check=True)
    elif sys.platform == "darwin" and platform.machine() == "arm64":
        image = work / "image"
        app = image / f"{APP}.app"
        contents = app / "Contents"
        (contents / "MacOS").mkdir(parents=True)
        shutil.move(stage / "GitNexiaBeta.exe", contents / "MacOS/GitNexiaBeta")
        shutil.move(stage, contents / "Resources")
        shutil.copy2(ROOT / "crates/gitnexia-app/icons/icon.icns", contents / "Resources/icon.icns")
        with (contents / "Info.plist").open("wb") as file:
            plistlib.dump(macos_plist(version), file)
        linked = subprocess.check_output(["otool", "-L", str(contents / "MacOS/GitNexiaBeta")], text=True)
        if "WebKit.framework" not in linked or "Chromium" in linked or "libcef" in linked:
            raise ValueError("Beta binary must link system WebKit, not CEF")
        subprocess.run(["codesign", "--force", "--sign", "-", str(app)], check=True)
        subprocess.run(["codesign", "--verify", "--deep", "--strict", str(app)], check=True)
        (image / "Applications").symlink_to("/Applications")
        installer = out / f"{stem}_aarch64.dmg"
        subprocess.run([
            "hdiutil", "create", "-volname", APP, "-srcfolder", str(image),
            "-format", "UDZO", str(installer),
        ], check=True)
        subprocess.run(["hdiutil", "verify", str(installer)], check=True)
    else:
        raise ValueError("Packaging supports Windows x64 and macOS Apple Silicon only")
    if not installer.is_file() or installer.stat().st_size == 0:
        raise ValueError(f"No installer was produced: {installer}")
    digest = hashlib.sha256(installer.read_bytes()).hexdigest()
    (out / "SHA256SUMS").write_text(f"{digest}  {installer.name}\n", encoding="utf-8")
    (out / "BETA.txt").write_text(WARNING, encoding="utf-8")
    shutil.rmtree(work)
    print(installer)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-id", required=True, type=int)
    parser.add_argument("--makensis", default="makensis")
    args = parser.parse_args()
    if args.build_id < 1:
        parser.error("--build-id must be positive")
    version = json.loads((ROOT / "crates/gitnexia-app/tauri.conf.json").read_text())["version"]
    out = ROOT / "target/system-webview-beta/dist" / str(args.build_id)
    out.mkdir(parents=True, exist_ok=True)
    package(version, args.build_id, ROOT / "target", out, args.makensis)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"package-system-webview-beta: {error}", file=sys.stderr)
        sys.exit(1)
