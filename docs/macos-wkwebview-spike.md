# macOS on the system webview (WKWebView) — spike

**Status: spike, not the default.** The macOS app still ships on CEF. This feature only exists
to answer one question: can the macOS build run on Tauri's own runtime and Apple's system
WKWebView, with the UI fast enough and the bundle small enough?

The separate `system-webview-beta` feature builds experimental Windows/macOS test packages.
Actions > **System webview beta** > **Run workflow** builds a Windows x64 setup `.exe` and
an Apple Silicon `.dmg`. Download each platform's artifact at the bottom of the run and extract
it; it contains the package, `SHA256SUMS` and `BETA.txt`. These are Actions artifacts (14 days),
not GitHub Releases or website downloads.

The app is **GitNexia Beta**, identifier `com.gitnexia.beta`, with separate data, webview storage
and keyring entries. It never imports regular settings or tokens. The Windows per-user installer
uses `%LOCALAPPDATA%\Programs\GitNexia Beta`; on macOS drag `GitNexia Beta.app` into Applications.
Uninstalling keeps the beta's data. Updates are manual: download and install a later workflow
artifact; the regular updater stays disabled. Windows needs WebView2 already installed. macOS
needs 13.3 or later; newer CSS features still need compatibility/performance testing.

These are unsigned Windows and ad-hoc-signed, unnotarized macOS packages: expect OS security
warnings. They are for disposable test repositories, not production work. The app CSP stays
enabled, but the CEF build's verified runtime network/privacy and sandbox guarantees do not
apply; see [PRIVACY.md](../PRIVACY.md) and [SECURITY.md](../SECURITY.md). No system browser runtime
or offline dictionary is bundled. Notices are generated for the beta's actual dependency graph.
The packager bypasses the CEF-aware Tauri bundler entirely, so it cannot add CEF helpers.

Why CEF was chosen in the first place is in
[`cef-over-webkitgtk.md`](decisions/cef-over-webkitgtk.md). That decision is about Linux's
WebKitGTK; WKWebView is a different engine port and has not been measured yet.

## Building and running

`./build-macos-app.sh --wkwebview` builds the app on Tauri's own runtime (WKWebView) instead of
CEF. It passes `--no-default-features`, so the binary links `tauri-runtime-wry` and not CEF.
The CLI's bundler still detects CEF (it looks at the manifest's default features, not the runner
args), so the script sets `bundle.cef.embed=false` and removes the unused helper apps itself:
the bundle contains no CEF framework and no helper apps. `--install` works as usual; the rest of
the script (minimum macOS version, ad-hoc signing, `/Applications` swap) is unchanged:

```sh
./build-macos-app.sh --wkwebview --debug --install
```

The underlying Cargo feature is `macos-wkwebview` in `crates/gitnexia-app`:

```sh
cd crates/gitnexia-app
cargo tauri build --debug --no-bundle --features macos-wkwebview -- --no-default-features
```

With wry the app also runs as a bare binary (no `GitNexia.app`, no helper processes). A throwaway
profile keeps the real one alone:

```sh
GITNEXIA_DEV_DIRS=$(mktemp -d) ./target/debug/GitNexia /path/to/repo
```

`--no-default-features` is what drops CEF from the binary; a plain
`cargo tauri build --features macos-wkwebview` keeps CEF linked (the feature only selects the
runtime at startup) and produces a CEF-sized bundle. The script's `bundle.cef.embed=false` and
helper removal are a workaround for the CLI limitation above; a real switch should teach the
bundler to see `--no-default-features` (or use a CEF-free manifest) instead.

## First results (2026-11-05, this Mac)

- The `--wkwebview` release bundle links `/System/Library/Frameworks/WebKit.framework`, has no
  `Contents/Frameworks`, no CEF framework and no helper apps, and ad-hoc signing verifies.
  **46 MB** versus **369 MB** for the CEF bundle (same UI and release profile, unstripped).
- The debug wry binary opened the UI, read the repository and loaded its avatars through the
  normal IPC path. Performance is not measured yet.
- The build still compiles CEF's helper once: `cargo tauri build` decides the bundler runtime
  from the manifest's default features and ignores the `--no-default-features` runner argument.
  The script works around the result, not the extra compile. A CEF-free build needs the bundler
  to honour the feature set (or a CEF-free manifest).

## What the spike must show

1. **Diff editor.** Open a 5,000-line diff and scroll at 80 px/frame and 16 px/frame. Compare
   against CEF on the same Mac: open time, fps, % frames over 20 ms, p99. WebKitGTK failed this
   hard (37 fps, 99% janky); WKWebView needs to stay close to CEF's numbers.
2. **Graph.** A 2,000- and 10,000-row fling, same metrics.
3. **UI audit.** `field-sizing` (commit description, WIP input) needs Safari 26.2+; `lh` units
   (Open screen, commit box) need Safari 16.4+; `scrollbar-width` and Chromium-only scrollbar
   styling differ. Check the commit box, Open screen, tabs and scrollbars on macOS 12, 14 and 26.
4. **Storage.** The CEF profile's `localStorage` (theme mirror, WIP drafts, list preferences)
   is not shared with WKWebView. Decide between a one-time export/import and accepting the loss.
5. **Idle cost.** `GITNEXIA_PID=<browser pid> just bench` still measures the process tree; the
   budget stays idle CPU < 1.5% and < 650 MB with 12 tabs.
6. **Packaging.** Size of the `.app`/`.dmg`, signing (no JIT entitlements needed), and the
   absence of CEF's helper apps and `chrome-sandbox` story.

## Known gaps in this spike

- The macOS CI jobs drive the app over Chromium's DevTools protocol (`GITNEXIA_DEV_CDP_PORT`).
  WKWebView has no CDP; those checks need WebDriver or another driver.
- CEF-specific behaviour has no equivalent: `--host-resolver-rules`, component updates, the
  bundled offline en-US dictionary, `SandboxPolicy::Required`, and the vendored runtime patches.
  The security and privacy documents must be revisited before shipping this.
- `window_manager` (the title bar's window menu/lower request) returns an error, and the UI
  falls back to its own menu, as it already does on platforms without a window manager.
- The script still stamps `LSMinimumSystemVersion` 12.0. The UI's `lh` units need Safari 16.4
  (macOS 13.3) and `field-sizing` needs Safari 26.2; a real switch has to raise the minimum or
  add fallbacks.

## Decision

Do not flip the macOS default until the numbers above are in. If WKWebView passes, macOS can
use it and CEF stays on Linux; Windows' system webview (WebView2) is Chromium and could be
evaluated separately.
