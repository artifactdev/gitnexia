# Releasing GitNexia

A release is a tag. `just release <version>` prepares the release commit and tag locally;
pushing them starts [.github/workflows/release.yml](../.github/workflows/release.yml), which
builds the `.deb`, the Arch package, the `.rpm`, the two Windows installers and the macOS
`.dmg`, checks them, and publishes a GitHub release with the packages and `SHA256SUMS`.

## Experimental system-webview packages

The manually dispatched **System webview beta** workflow is independent of this release flow.
It uploads a Windows x64 per-user setup installer and a macOS Apple Silicon DMG as Actions
artifacts, with checksums and test warnings. It does not publish a release or update the website.
The packages install as **GitNexia Beta**, use separate profiles and require manual updates.
They are unsigned/not notarized experimental builds, not replacements for the CEF releases.
See [the system-webview notes](macos-wkwebview-spike.md) for requirements and limitations.

## Before you start

- **Python 3.11 or newer must be available as `python3`.** Check `python3 --version`; on macOS,
  install it with `brew install python` and put Homebrew's `bin` directory before `/usr/bin`
  on `PATH`. The version editor uses `tomllib`, which older Python versions don't include.
- **main must be green.** The release workflow builds and checks the packages, but it doesn't run
  the test suite (there's no CI on main yet). Run `just lint`, `just test` and `just e2e` on the
  commit you're releasing.
- **CHANGELOG.md's `## [Unreleased]` section lists the release's changes.** Its contents become
  the release notes. `just release` refuses an empty section. The dated sections are also the
  packages' AppStream release history (`scripts/package-meta.py`), and the newest one's tag pins
  the metainfo's screenshot URL, so push the tag with the release.

## Steps

1. **Prepare the release** on an up-to-date main, with no uncommitted changes:

   ```sh
   just release 0.1.0-alpha.1
   ```

   It sets the version in `crates/gitnexia-app/tauri.conf.json`, the workspace `Cargo.toml` and
   `Cargo.lock`, and `ui/package.json` and its lockfile. Then it turns `## [Unreleased]` into
   `## [0.1.0-alpha.1] - <today>` under a fresh, empty `## [Unreleased]`, commits
   "Release 0.1.0-alpha.1" and creates the annotated tag `v0.1.0-alpha.1`. It never pushes.

   It refuses to run when you're not on main, when tracked files have uncommitted changes, when
   main is behind `origin/main`, or when the version is invalid, already tagged, or not newer
   than every `v*` tag. If a step fails partway, it restores the files it edited.
   To undo a release you haven't pushed: `git tag -d v0.1.0-alpha.1 && git reset --hard HEAD~1`.

2. **Push the commit and the tag together** (the command `just release` prints):

   ```sh
   git push origin main v0.1.0-alpha.1
   ```

   GitNexia's Push does the same with **Push tags with branches** on (the default), since the
   tag is annotated. The build then runs on main's push, warm (see
   [Which run builds](#which-run-builds)). Pushed apart, it still releases: see there.

3. **Wait for the workflow:** Actions → Release (see [Build time and cost](#build-time-and-cost)).
   It fails fast when the tag doesn't match `tauri.conf.json`'s version or CHANGELOG.md has no
   section for it.

4. **Review the published release** under Releases. It's titled "GitNexia 0.1.0-alpha.1" and has the
   changelog section as its notes, plus the `.deb`, the `.pkg.tar.zst`, the `.rpm`, the Windows
   `-setup.exe` and `.msi`, the macOS `.dmg`, and `SHA256SUMS`. A
   version with a pre-release part (`-alpha.1`, `-rc.2`) is marked pre-release. Install the
   packages from the release to try them.

5. **The website's downloads update by themselves.** Publishing starts the Site downloads
   workflow: it writes `site/data/release.json` for the release, commits it to main as
   "Site: downloads for 0.1.0" and deploys the website. A pre-release is offered only while
   there's no other release. Pull main (`git pull --rebase`) before you next push
   it. See [docs/website.md](website.md#downloads).

## Asset names are an API

Installed GitNexias find their update by these names (`crates/gitnexia-core/src/updates/release.rs`),
in the releases of `artifactdev/gitnexia` only. The repository was `artifactdev/gitnex` until
1.6.0, and 1.6.0 publishes every package under both the new `GitNexia_*`/`GitNexia-*` names and
the old `GitNex_*`/`GitNex-*` ones: an installed 1.5.x still asks for the old names, and this
version reads either. From 1.6.1 on, the new names alone are enough. GitBolt 0.6.1 also takes
these names from the old repositories and installs GitNexia. So keep the names exactly as they
are, and keep `SHA256SUMS` in every release, listing every copy: an update whose package
`SHA256SUMS` doesn't list is never installed. The website's downloads (`scripts/site-release.py`)
find the packages by their new names too.

| Install | Asset |
|---|---|
| `.deb` | `GitNexia_<version>_amd64.deb` |
| Arch | `GitNexia-<pkgver>-1-x86_64.pkg.tar.zst` (`<pkgver>`: the version without the pre-release's `-`) |
| `.rpm` (Fedora, openSUSE) | `GitNexia-<version>-1.x86_64.rpm` (the version as in the `.deb`'s name: `GitNexia-0.1.0-alpha.1-1.x86_64.rpm`, although the package's Version is `0.1.0~alpha.1`) |
| Windows (NSIS) | `GitNexia_<version>_x64-setup.exe` |
| Windows (MSI) | `GitNexia_<version>_x64.msi` |
| macOS | `GitNexia_<version>_aarch64.dmg` (Apple Silicon; an Intel Mac takes `GitNexia_<version>_x64.dmg`, which no release has yet). The image holds `GitNexia.app` at its top level |
| all | `SHA256SUMS` (`sha256sum` output listing each package) |

For 1.6.0, the same rows exist with `GitNexia` replaced by `GitNex` (`GitNex_<version>_amd64.deb`,
`GitNex-<version>-1.x86_64.rpm`, …), byte for byte identical and listed in `SHA256SUMS` too.

The tag stays `v<version>`, and the release's body (the CHANGELOG section) is what the update
dialog shows. Drafts are never offered; a release marked pre-release (or with a pre-release
version) is offered only to users who include pre-releases, or who run a pre-release.

Each package says how it was installed in an `install-kind` file beside the binary, which the
app reads at run time (one build goes into both packages of a platform, so the binary itself
can't know):

- the `.deb` ships `/usr/share/GitNexia/install-kind` = `deb`
  (`crates/gitnexia-app/packaging/install-kind-deb`, through tauri.conf.json), and
  `scripts/package-arch.sh` rewrites it to `arch`, `scripts/package-rpm.sh` to `rpm`;
- the NSIS installer writes `install-kind` = `nsis` beside `GitNexia.exe`, and the MSI installs
  `packaging/windows/install-kind-msi` (`msi`) there; `scripts/package-windows.ps1` checks the
  MSI has it;
- the `.dmg`'s GitNexia.app has `Contents/Resources/install-kind` = `dmg` (the bundle's place for
  its files: `crates/gitnexia-app/packaging/install-kind-dmg`, through
  `packaging/macos/dmg.conf.json`, which only `just package-macos` passes, so a plain
  `cargo tauri build` bundle counts as a build from source); `scripts/package-macos.sh` checks it.
  An update mounts the new `.dmg` and copies its `GitNexia.app` over the running bundle, so the
  image must keep holding `GitNexia.app` at its top level.

No file: a build from source, which is only offered the release page. `just package`,
`just package-windows` and `just package-macos` also bake the stamped version in
(`GITNEXIA_BUILD_VERSION`), which the status bar and the update check use.

## The moves from older names

### GitNex → GitNexia (1.6.0)

The app was GitNex until 1.6.0: the repository was `artifactdev/gitnex`, the crates `gitnex-*`,
the binary `gitnex`, the bundle identifiers `com.gitnex.desktop` and `dev.gitnex.desktop`. The
rename to GitNexia renamed the repository to `artifactdev/gitnexia` (GitHub redirects the old
address) and changed all of those to `gitnexia`. The first start of 1.6.0 copies GitNex's config,
data and CEF profile, and its keyring items (`gitnex`), to GitNexia's; the old folders and items
stay for a downgrade. Stack tables, Compare refs and interactive-rebase notes written by GitNex
are still read.

- **Publish the old asset names with 1.6.0.** The release workflow copies each package to its
  `GitNex_*` name, and `SHA256SUMS` lists both. An installed 1.5.x checks `artifactdev/gitnex`;
  if GitHub's repository redirect doesn't carry its API calls or downloads, it needs one manual
  install of 1.6.0, after which updates work as before. The README says so.
- **Keep the `artifactdev/gitnex` redirect.** Don't create a new repository with that name: it
  would take over the old address and capture the update checks and downloads of installs that
  still use it.
- **The installers replace a GitNex install.** The MSI keeps the UpgradeCode GitNex's used (and
  GitBolt's before it); the NSIS installer runs a GitNex install's uninstaller first; the Debian,
  Arch and RPM packages replace/conflict with/obsolete `gitnex`. Settings and data live in the
  user's profile, so they stay and the first start moves them.
- **The old keyring services stay readable.** `gitnex` and `gitvolt` entries are read once and
  copied to `gitnexia`; the old items stay.

### GitBolt → GitVolt → GitNex (0.7.0, 1.5.1)

The project was called GitBolt until 0.7.0, and its repository was `GitBoltApp/gitbolt`. On
2026-10-14 it was transferred to `artifactdev/gitvolt` and renamed GitVolt in one call, so GitHub
redirects the old addresses. 0.6.1 bridged the two repository and package names; 0.6.0 and
earlier only know `GitBoltApp/gitbolt` and the GitBolt asset names, so they can't follow (the
README says so). GitVolt became GitNex at 1.5.1 (repository `artifactdev/gitnex`) and GitNexia at
1.6.0. Older GitNexia builds still check a previous repository; because the updater doesn't
follow API redirects or accept downloads from another repository name, users may need to install
the first build with the move manually. Keep the old repository redirects for browser links.

- **Never delete or rename the `GitBoltApp` organization, nor create a `gitbolt` repository in
  it.** It would take over the old address: an install still on 0.6.0 (or on 0.6.1, if its
  repository were missing) would check it and its updates would be captured, and GitHub's
  redirect for old links would end. The old Pages address redirects through the
  `GitBoltApp/gitboltapp.github.io` repository for the same reason.
- **GitNexia doesn't take over a GitBolt install.** Its packages don't replace GitBolt's, and its
  first start doesn't move GitBolt's folders or keyring items (0.7.x did): a GitBolt user installs
  GitNexia fresh. What stays: the metainfo `<replaces>` `dev.gitbolt.desktop`, the MSI's
  UpgradeCode, every package named `gitnexia` (`fix-deb.sh` renames Tauri's `gitnexia`), and every
  `GitNexia_*.dmg` containing `GitNexia.app`.

## Versions

A release version is [SemVer](https://semver.org) without build metadata: `X.Y.Z` or
`X.Y.Z-<pre-release>`. The pre-release is a lowercase word (`alpha`, `beta`, `rc`), then
optional dot-separated lowercase words or numbers: `0.1.0-alpha.1`, `0.2.0-rc.2`. Identifiers
that mix letters and digits, like `alpha10`, are refused, because SemVer and pacman would sort
them differently. The rules are in `scripts/version.py`.

Each package manager gets a version that sorts the same way as SemVer, with pre-releases
before the release:

| SemVer (tag `v…`) | `.deb` and `.rpm` Version | Arch `pkgver` | Files |
|---|---|---|---|
| `0.1.0-alpha.1` | `0.1.0~alpha.1` | `0.1.0alpha.1` | `GitNexia_0.1.0-alpha.1_amd64.deb`, `GitNexia-0.1.0alpha.1-1-x86_64.pkg.tar.zst`, `GitNexia-0.1.0-alpha.1-1.x86_64.rpm` |
| `0.1.0-rc.2` | `0.1.0~rc.2` | `0.1.0rc.2` | `GitNexia_0.1.0-rc.2_amd64.deb`, `GitNexia-0.1.0rc.2-1-x86_64.pkg.tar.zst`, `GitNexia-0.1.0-rc.2-1.x86_64.rpm` |
| `0.1.0` | `0.1.0` | `0.1.0` | `GitNexia_0.1.0_amd64.deb`, `GitNexia-0.1.0-1-x86_64.pkg.tar.zst`, `GitNexia-0.1.0-1.x86_64.rpm` |
| local build of `0.1.0` | `0.1.0+202610051325.ab4dbf9e` | `0.1.0.202610051325.ab4dbf9e` | stamped, as before |

- Debian's `~` sorts before anything, so `0.1.0~alpha.1` is older than `0.1.0`
  (`scripts/fix-deb.sh` writes it; Tauri only accepts SemVer).
- rpm sorts `~` before anything too, and treats `+` as a separator, so the RPM takes the `.deb`'s
  Version as is (Release `1`; `scripts/rpm-pkg.py`). Its file name keeps the SemVer form, as
  the `.deb`'s does.
- pacman's `vercmp` treats a leftover part that starts with a letter as older, but one that
  starts with a `.` as newer: `0.1.0.alpha.1` would sort *after* `0.1.0`, so the word follows the
  number directly (`scripts/arch-pkg.py`).
- `scripts/test-version-order.sh` (part of `just test-scripts`) checks the whole order with
  `dpkg --compare-versions`, and with `vercmp` and rpm's `rpm.vercmp` when they're installed. Run
  it with `GITNEXIA_TEST_DOCKER=1` to use them in `archlinux:latest` and `fedora:latest`
  containers.

Windows wants numeric versions: the MSI's `ProductVersion` and the files' `FILEVERSION` are
`X.Y.Z.0` for every build of `X.Y.Z`, its pre-releases and local builds included. Windows Installer
compares only the first three fields, so the MSI allows same-version upgrades
(`AllowSameVersionUpgrades`): any build of `0.1.0` replaces whichever `0.1.0` build is installed,
pre-release or not, and an older `X.Y.Z` is refused. The full version stays in the file names
(`GitNexia_0.1.0-alpha.1_x64-setup.exe`, `GitNexia_0.1.0-alpha.1_x64.msi`), in the files' version
strings (Properties > Details) and in the NSIS installer's entry in Settings > Apps; the MSI's
entry shows `0.1.0.0`.

Local `just package` builds stay stamped (`<version>+<UTC time>.<commit>`), so each one installs
over the last. `GITNEXIA_RELEASE_VERSION=<version> just package` builds the plain version the way
the workflow does; it must equal `tauri.conf.json`'s version.

Local builds also reuse what hasn't changed since the last one (the license notices, the UI build,
`dpkg-shlibdeps`) and compress fast (zstd 3); see [dev-setup.md](dev-setup.md#packaging-just-package).
The workflow sets `GITNEXIA_REBUILD=1`, which regenerates all of it, and
`GITNEXIA_PACKAGE_COMPRESSION=release` (`GITNEXIA_RELEASE_VERSION` alone implies it too): the
`.deb` in gzip 6, the Arch package and the RPM in zstd -19, as they have always shipped. To build
locally exactly as the workflow does, set all three.

## What the workflow does

After Plan (see [Which run builds](#which-run-builds)), the Linux, Windows and macOS builds run
side by side, then the release job:

1. **Build** (read-only access to the repository):
   - checks the tag against `tauri.conf.json`'s version and extracts the release notes from
     CHANGELOG.md (`scripts/changelog.py notes <version> CHANGELOG.md`);
   - installs the toolchain: the system packages (`libgtk-4-dev`, `patchelf`, `dpkg-dev`, `zstd`,
     `binutils`, `cmake`, `ninja-build`, and `appstream` and `desktop-file-utils` for
     `check-deb.sh`), stable
     Rust, Node 22, `just`, `tauri-cli` 3.0.0-alpha.5 and cargo-about 0.9.2;
   - runs `just package` with `GITNEXIA_RELEASE_VERSION`, `GITNEXIA_REBUILD=1` and
     `GITNEXIA_PACKAGE_COMPRESSION=release` set (no local caches, the release compression). That
     generates the third-party license notices (`scripts/licenses.sh`, failing on a license
     outside `about.toml`'s allow-list; see [licensing.md](licensing.md)), builds, and runs
     `fix-deb.sh`, `check-deb.sh` (which also checks the notices are in the `.deb`), then
     `package-arch.sh` and `package-rpm.sh` side by side;
   - runs `just check-arch-pkg` (Docker), which installs the Arch package in `archlinux:latest`
     and checks it, its notices included, then `just check-rpm-pkg`, which does the same for the
     RPM in `fedora:latest` with dnf;
   - writes `SHA256SUMS`, and keeps the packages, the checksums and the notes as a workflow
     artifact for 3 days.
2. **Windows** (`windows-latest`, read-only): stable Rust, Node 22, `just`, cargo-about 0.9.2
   and Visual Studio's Ninja; runs `just package-windows` with `GITNEXIA_RELEASE_VERSION` set (see
   [Windows installers](#windows-installers)), and keeps both installers and their checksums as
   a workflow artifact.
3. **macOS** (`macos-15` by default, or `macos-26` selected in **Run workflow**; both
   GitHub-hosted Apple Silicon, read-only): stable Rust, Node 22, `just`, CMake and
   Ninja, `tauri-cli` and cargo-about; imports the Developer ID certificate into a throwaway
   keychain (see [macOS signing](#macos-signing)), runs `just package-macos` with
   `GITNEXIA_RELEASE_VERSION` set (see [macOS disk image](#macos-disk-image)), notarizes the app
   and the `.dmg`, and keeps the `.dmg` and its checksum as a
   workflow artifact. Its caches (cargo, the two cargo tools, the CEF archive) are saved from main
   only, as the others are. Cargo and compiled cargo-tool caches are separate for each macOS
   image; the CEF archive is shared.
4. **Release** (`contents: write`): verifies the three jobs' checksums and merges them into one
   `SHA256SUMS`, adds build provenance attestations for every package (see below), then publishes
   the release after uploading all assets. A failed or re-run release updates the draft before
   publication; an already-published release is left untouched.

The packages' `Depends` come from the build machine's libraries (dpkg-shlibdeps), so packages
built on the 24.04 runner install on Ubuntu 24.04 and later. The RPM's `Requires` come from the
same libraries (glibc 2.39's symbol versions): Fedora 40 and later. A local `just package` on a newer
distribution can require newer library versions; publish the workflow's packages, not local ones.

## Which run builds

Only runs on main save caches: a tag's run can restore caches saved on main, but not another
tag's. So a release builds on main, and each release warms the next one, as long as GitHub keeps
the caches (it evicts them after 7 days unused; the next release then builds cold). One build
per release, decided by the workflow's first job, **Plan**:

- **A push to main that changes `Cargo.lock`** (a release's version bump does): if the pushed
  commit carries a `v*` tag, this run releases it. Otherwise it does nothing (a few seconds).
- **A pushed `v*` tag:** waits for main's run on the same commit. If that one releases the tag,
  this run stops. If not (the tag was pushed after its commit, or main's push didn't run), it
  builds and publishes the release itself, from cold caches. Nothing is lost either way.
- **Run workflow** (Actions → Release → Run workflow, on main) with a **tag**: releases that tag
  (its commit, warm). The way to redo or rescue a release by hand.

## Windows installers

`just package-windows` (on Windows; `scripts/package-windows.ps1`) builds once and packages twice,
into `target\release\bundle\windows\`:

- **`GitNexia_<version>_x64-setup.exe`** (NSIS): installs for the current user, no administrator
  rights, into `%LOCALAPPDATA%\Programs\GitNexia`. Silent: `/S`, and `/D=<folder>` (last,
  unquoted) for another folder. Settings > Apps lists it; its uninstaller is
  `uninstall.exe` (`/S` for silent). Installing a newer one over it runs the old uninstaller first.
- **`GitNexia_<version>_x64.msi`** (WiX v5): installs for all users into `Program Files\GitNexia`
  (administrator rights), for managed deployment: `msiexec /i GitNexia_<version>_x64.msi /qn`.

Both add a Start menu shortcut and "Open in GitNexia" on folders in File Explorer, and leave the
user's settings and data (`%APPDATA%\gitnexia`, `%LOCALAPPDATA%\gitnexia`,
`%LOCALAPPDATA%\com.gitnexia.desktop`) on uninstall.

The MSI keeps the UpgradeCode GitBolt's and GitNex's MSIs used (the app's names before 0.7.0 and
1.6.0): never change it. The NSIS installer runs a GitNex install's uninstaller first, and leaves
a GitVolt or GitBolt install alone; GitNexia's first start moves GitNex's and GitVolt's settings
and data over.

The installed folder is CEF's sandboxed layout: `GitNexia.exe` is CEF's `bootstrap.exe` (with
GitNexia's icon, version information and `packaging/windows/GitNexia.exe.manifest`), which loads
`GitNexia.dll` (gitnexia-app built as a DLL) and runs Chromium sandboxed; then CEF's runtime files,
`dictionaries\` and the license notices in `licenses\`. The bootstrap must come from the CEF build
the DLL links, and nothing at run time checks that, so the script compares `bootstrap.exe`'s and
`libcef.dll`'s versions with `Cargo.lock`'s CEF before staging.

The packaging tools are pinned and downloaded on first use into `target\windows-tools` (checked
against their SHA-256; WiX through `dotnet tool install`): NSIS 3.11, WiX 5.0.2 and rcedit 2.0.0.
The MSI also goes through Windows Installer's ICE checks.

### Signing

The installers aren't signed yet, so SmartScreen warns about them ("Windows protected your PC" >
More info > Run anyway). The hook is in place: `GITNEXIA_SIGN_COMMAND`, a command that signs the
file path appended to it in place (for example `signtool sign /fd sha256 /tr <timestamp URL> /td
sha256 /f <certificate>`). With it set, `package-windows.ps1` signs, in order:

1. `GitNexia.exe` and `GitNexia.dll`, **with the same certificate**: a signed bootstrap only loads a
   DLL signed with its own certificate, so signing one without the other, or with two
   certificates, makes the app refuse to start;
2. the NSIS uninstaller (while the installer is built, through NSIS's `!uninstfinalize`), then the
   installer;
3. the MSI.

In the workflow, set it in the "Build the installers" step of the Windows job, from a secret (the
step's comment marks the place).

## macOS disk image

`just package-macos` (on a Mac; `scripts/package-macos.sh`) builds the release `GitNexia.app` and
`target/release/bundle/dmg/GitNexia_<version>_aarch64.dmg` with the Tauri CLI's own bundlers: the
`.dmg` holds `GitNexia.app` and a link to `/Applications`. The app is CEF's layout (the framework
and the helper apps in `Contents/Frameworks`; docs/dev-setup.md), with the spell-check
dictionary, the `install-kind` marker and the license notices in `Contents/Resources`, and an
`Info.plist` that declares folders as a document type (Open With). The script then checks the
signature (see below), the marker, the notices and the document type, in the bundle and again in
the mounted image.

### macOS signing

The release's app is signed with **an Apple Developer ID Application certificate** and notarized,
so macOS recognises each new version as the same app and Gatekeeper accepts a downloaded copy:

| | |
|---|---|
| Subject | CN "Developer ID Application: Jeremias Arnstadt (CJKNBR45T2)" |
| Team ID | `CJKNBR45T2` |
| SHA-1 | `F7:90:B0:B4:69:63:FC:69:DB:79:B6:4A:90:AA:DB:76:82:1F:A0:8B` |
| Valid until | 2027-02-01 |

macOS remembers an app's Privacy & Security permissions (folder access such as Downloads) by the
app's *designated requirement*. For an ad-hoc signature, as up to 1.2.0, that is the build's hash
(`cdhash H"…"`), new with every build, so each update asked again (#11). Signed with the
Developer ID it is the same in every release:

```
identifier "com.gitnexia.desktop" and anchor apple generic and certificate leaf[subject.OU] = "CJKNBR45T2"
```

(codesign's default for a Developer ID: the identifier, Apple's `anchor apple generic` and the
Team ID, plus its certificate-field clauses.) The first release signed with it still asks once,
since 1.5.x's requirement named a certificate hash; the ones after it don't. The Team ID also
means the Keychain no longer asks after every update: its items' partition list names the app by Team ID, so **Always Allow**
carries over (with a self-signed certificate it named the build's cdhash instead).

**Notarization.** The app and the `.dmg` are notarized by Apple's notary service. The Tauri CLI
does the app while bundling, given `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and
`APPLE_TEAM_ID`: it signs, submits, waits and staples the ticket. The job then notarizes and
staples the `.dmg` itself (`xcrun notarytool submit --wait`, `xcrun stapler staple`), so
Gatekeeper accepts the image when it is opened. `scripts/package-macos.sh` checks the stapled
ticket in the app and in the mounted image when the credentials are set. The `.dmg`'s pre-rename
`GitNex_*` compatibility copy is byte-identical, so it carries the same signature and ticket. An
update downloaded by GitNexia itself carries no quarantine flag, so it needs neither.

**How the release signs.** The certificate and its key are two repository secrets:
`MACOS_CERTIFICATE_P12` (the `.p12`, base64, exported from Keychain Access with the "Developer ID
Certification Authority" intermediate) and `MACOS_CERTIFICATE_PASSWORD`. Notarization takes three
more: `APPLE_ID` (the Apple ID), `APPLE_PASSWORD` (an app-specific password, not the Apple ID's)
and `APPLE_TEAM_ID`. The macOS job's "Signing keychain" step (`scripts/macos-keychain.sh import`)
puts the certificate in a throwaway keychain on the runner, unlocked and searched for the build,
tries a signature, and sets `GITNEXIA_MACOS_SIGNING_IDENTITY` to the certificate's SHA-1; the job
deletes the keychain after the build, whatever happens. A run on `artifactdev/gitnexia` without
the certificate or the notarization secrets fails there; a fork's run builds ad-hoc.
`mac-port.yml`'s package job imports the certificate too, minus the requirement, and doesn't
notarize.

`just package-macos` signs with `GITNEXIA_MACOS_SIGNING_IDENTITY` when it's set (an identity in
the keychain search list, by SHA-1 or name), and ad-hoc otherwise (`signingIdentity: "-"` in
`tauri.macos.conf.json`; a local build, a fork). The Tauri bundler signs inside out: the helper
apps, the framework's libraries and the framework, the main binary, then the bundle, each with the
hardened runtime and CEF's entitlements (JIT, unsigned executable memory, and no library
validation, without which the hardened runtime would refuse CEF's framework under any identity but
Apple's); with an identity, the `.dmg` too. Each helper app gets its own identifier
(`com.gitnexia.desktop.helper`, `.helper.gpu`, `.helper.renderer`, `.helper.plugin`,
`.helper.alerts`), through `scripts/macos-codesign/codesign` ahead on the bundler's `PATH`. The
script then checks, in the bundle and in the mounted `.dmg`: `codesign --verify --deep --strict`,
the hardened runtime, the library-validation entitlement, the identifiers and, with an identity,
that the app's and every helper's designated requirement is their identifier and Apple's anchor
with the Team ID (or, for a self-signed certificate, the certificate's hash; never a cdhash), and
that the app carries its notarization ticket when the credentials are set. It prints the signature
(`Authority=Developer ID Application: Jeremias Arnstadt (CJKNBR45T2)`,
`TeamIdentifier=CJKNBR45T2`) and the requirement.

**Where it's kept.** The `.p12`, its password and the notarization credentials are in the
repository's secrets, which GitHub never shows again, and in the maintainer's password manager,
the only copy to restore them from. Neither belongs in the repository, a log or an issue.

**Renewing it** (Developer ID certificates expire; this one on 2027-02-01): every user gets **one
more round of folder-permission prompts** after the first update signed with the new certificate,
as with the move from ad-hoc. To replace it, create or renew the "Developer ID Application"
certificate in the Apple Developer portal, then:

```sh
# In Keychain Access: select the certificate and its private key, plus "Developer ID
# Certification Authority", and export them as a .p12.
base64 < DeveloperID.p12 | gh secret set MACOS_CERTIFICATE_P12 -R artifactdev/gitnexia
gh secret set MACOS_CERTIFICATE_PASSWORD -R artifactdev/gitnexia
gh secret set APPLE_ID -R artifactdev/gitnexia
gh secret set APPLE_PASSWORD -R artifactdev/gitnexia        # app-specific password
gh secret set APPLE_TEAM_ID -R artifactdev/gitnexia
```

Keep the `.p12` and its password in the password manager and update the SHA-1 and Team ID above
and in SECURITY.md: `security find-identity -v -p codesigning` prints them, and
`openssl x509 -noout -fingerprint -sha1` on the exported certificate prints the hash.
`mac-port.yml` (run it on a branch) shows the new `Authority` and requirement before a release.

**Versions.** `CFBundleShortVersionString` and `CFBundleVersion` are the package version as is
(`0.4.0-rc.1`, or a local build's stamp): macOS shows it in Finder and nothing compares it, since
the updater uses GitNexia's own version.

**Intel Macs.** Not built yet: the release has only the `aarch64` `.dmg`. An Intel build would
be a second, separate `GitNexia_<version>_x64.dmg` (the updater already picks its own
architecture's), cross-compiled on the same runner with CEF's x64 distribution. A universal app
would need both CEF distributions, two builds, and `lipo` on the app, every helper and the
framework's libraries, doubling the download for everyone.

## Dry run

Actions → Release → **Run workflow**, on main, with the tag left empty. It builds and checks the
packages from the branch's version, as the plain version, and keeps them as a workflow artifact
(3 days). It creates no release and no tag. A missing CHANGELOG section is only a warning. It
also warms the caches.

## Re-running and fixing

- **No free `macos-15` runner:** cancel the queued release run, then use Actions → Release →
  **Run workflow** on main, enter the same release tag and select **macos-26** in
  **macos_runner**. This starts a new build of all platforms for that tag. There is no automatic
  fallback on queue timeouts, and "Re-run jobs" keeps the original runner selection.
  Run a dry run on `macos-26` first and test its DMG on the oldest supported macOS before
  using the new build image for a release.
- **A transient failure** (a download, the Arch mirror in `check-arch-pkg`, Fedora's in
  `check-rpm-pkg`): use "Re-run failed
  jobs". Re-running only the release job reuses the run's build artifact (kept 3 days).
- **Re-running before publication** updates the draft's title, notes and pre-release flag, and
  replaces its assets (`gh release upload --clobber`) before publishing. If the release is already
  published, the job fails and leaves it untouched.
- **A broken release that needs a code fix:** delete the draft (if one remains), delete the tag
  (`git push origin :refs/tags/v0.1.0-alpha.1` and `git tag -d v0.1.0-alpha.1`), fix it on
  main, and release again. Once a version is published, don't reuse it: release the next one.

## Build provenance

The release job attests every package with `actions/attest-build-provenance`, so anyone can
check that a file came from this workflow:

```sh
gh attestation verify GitNexia_0.1.0-alpha.1_amd64.deb --repo artifactdev/gitnexia
```

Attestations need a public repository, or GitHub Enterprise Cloud for a private one. While the
repository is private, the step is skipped unless the repository variable `GITNEXIA_ATTEST` is
`true` (set it only on a plan that supports them). `SHA256SUMS` is always uploaded.

## Build time and cost

Estimated times on a GitHub-hosted runner (private repositories get the 2-vCPU, 8 GB Linux
runner; public ones get 4 vCPUs, roughly halving the compile steps):

| Step | Cold caches (first run) | Warm caches |
|---|---|---|
| Setup (checkout, apt, Rust, Node, `npm ci`) | 3–4 min | 2–3 min |
| `tauri-cli` and `cargo-about` installs | 10–15 min (compiled) | seconds (cached) |
| CEF download (~310 MB) | 1 min | restored from the cache |
| `just package`: Rust release build | 30–45 min | 10–15 min (only GitNexia's own crates) |
| `just package`: UI build, `.deb`, Arch package, `.rpm` | 6–8 min | 6–8 min |
| `just check-arch-pkg` | 3–5 min | 3–5 min |
| `just check-rpm-pkg` | 2–3 min | 2–3 min |
| Release job | 1 min | 1 min |
| **Total** | **about 55–80 min** | **about 25–35 min** |

The job's timeout is 120 minutes. The Windows and macOS jobs run at the same time and take about
as long (their Rust builds are the longest steps; the macOS job also waits for Apple's notary
service, usually a few minutes); on a private repository Windows minutes count double and macOS
minutes ten times.

The repository is private, so the runs count against the account's included Actions minutes
(Linux runners at a 1× rate: 2,000 minutes a month on GitHub Free, 3,000 on Team). A release is
roughly 30 minutes warm and 65 or more cold, and so is each dry run. Caches (about 3–5 GB:
the Rust target and registry, the CEF archive, the two cargo tools) use the repository's 10 GB cache
allowance, not the storage quota, and are evicted after 7 days unused. The workflow artifact
(about 300 MB) counts against the Actions storage quota (500 MB on Free) for its 3 days.
