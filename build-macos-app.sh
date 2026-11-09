#!/usr/bin/env bash
# Builds GitNexia.app locally, ad-hoc signs it, sets its minimum macOS version to 12.0, and copies
# it to target/<profile>/bundle/macos/GitNexia.app. The build runs in a fresh temporary target
# directory (compiling into a network volume can corrupt build artifacts); MACOSX_DEPLOYMENT_TARGET
# stays unset because this toolchain makes corrupt proc-macro dylibs with it.
#
# Needs Xcode's command line tools, CMake, Ninja, Node and the pinned Tauri CLI
# (`cargo install tauri-cli --version =3.0.0-alpha.5 --locked`). See docs/dev-setup.md.
set -euo pipefail
root=$(cd "$(dirname "$0")" && pwd)
fail() { echo "build-macos-app: $*" >&2; exit 1; }

usage() {
  cat <<'EOF'
Usage: ./build-macos-app.sh [--debug] [--keep] [--install] [--wkwebview]

  --debug    build the debug profile instead of release
  --keep     keep the temporary build directory (removed on success otherwise; kept on failure)
  --install  also replace the app in $GITNEXIA_APP_DIR (default: /Applications/GitNexia.app)
  --wkwebview
             build on Tauri's system webview (WKWebView) instead of the bundled CEF
             (spike; see docs/macos-wkwebview-spike.md)

The existing bundle is swapped by renames in the same folder, so a running GitNexia keeps
running the old build until it is quit and reopened.
EOF
}

debug=0
keep=0
install=0
wkwebview=0
for arg in "$@"; do
  case "$arg" in
    --debug) debug=1 ;;
    --keep) keep=1 ;;
    --install) install=1 ;;
    --wkwebview) wkwebview=1 ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unknown option $arg (try --help)" ;;
  esac
done

[ "$(uname -s)" = Darwin ] || fail "builds on macOS only"
command -v cargo >/dev/null 2>&1 || fail "cargo not found"
cargo tauri --version >/dev/null 2>&1 || fail "the Tauri CLI is missing: cargo install tauri-cli --version =3.0.0-alpha.5 --locked"
if [ "$wkwebview" = 0 ]; then
  # The cmake crate also accepts host- and target-specific executable overrides.
  if ! env | grep -E '^(HOST_CMAKE|TARGET_CMAKE|CMAKE_[a-z0-9]+[-_][a-z0-9_-]+)=' >/dev/null; then
    command -v "${CMAKE:-cmake}" >/dev/null 2>&1 || fail "CMake not found; install CMake and Ninja with: brew install cmake ninja"
  fi
  command -v ninja >/dev/null 2>&1 || fail "Ninja not found; install it with: brew install ninja"
fi

profile=release
tauri_args=(--bundles app)
if [ "$debug" = 1 ]; then
  profile=debug
  tauri_args=(--debug --bundles app)
fi
if [ "$wkwebview" = 1 ]; then
  # The `--no-default-features` goes to cargo through `cargo tauri build`'s runner args, which
  # must come after every Tauri option. Dropping the default `cef` feature makes the app link
  # wry (WKWebView) instead of CEF.
  tauri_args+=(--features macos-wkwebview)
  runner_args=(-- --no-default-features)
  # The CLI selects the bundler runtime from the manifest's enabled features and only sees
  # cargo's default ones, so it still treats this as a CEF app and builds CEF's helper apps.
  # `embed=false` at least keeps the 322 MB framework out; the helpers are removed below.
  bundle_config='{"bundle":{"macOS":{"minimumSystemVersion":null},"cef":{"embed":false}}}'
else
  runner_args=()
  bundle_config='{"bundle":{"macOS":{"minimumSystemVersion":null}}}'
fi
build_dir=$(mktemp -d "${TMPDIR:-/tmp}/gitnexia-app.XXXXXX")
install_new=
cleanup() {
  local status=$?
  [ -n "$install_new" ] && rm -rf "$install_new"
  if [ "$keep" = 1 ] || [ "$status" -ne 0 ]; then
    echo "build-macos-app: build directory kept: $build_dir" >&2
  else
    rm -rf "$build_dir"
  fi
}
trap cleanup EXIT

echo "build-macos-app: building the $profile bundle (build directory: $build_dir)"
(cd "$root/crates/gitnexia-app" && \
  env -u CARGO_INCREMENTAL -u MACOSX_DEPLOYMENT_TARGET \
    CARGO_TARGET_DIR="$build_dir" CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}" \
    cargo tauri build "${tauri_args[@]}" \
      --config "$bundle_config" \
      ${runner_args[@]+"${runner_args[@]}"})

built=$build_dir/$profile/bundle/macos/GitNexia.app
[ -d "$built" ] || fail "the build made no $built"
if [ "$wkwebview" = 1 ]; then
  # This app runs on WKWebView and ships no CEF. The CLI still creates CEF's helper apps (it
  # reads the manifest's default features, not the `--no-default-features` runner arg); the
  # framework is already left out by `bundle.cef.embed=false`. Remove the helpers before the
  # final ad-hoc signature below.
  rm -rf "$built/Contents/Frameworks"/*Helper*.app
  rmdir "$built/Contents/Frameworks" 2>/dev/null || true
fi
plist=$built/Contents/Info.plist
if ! /usr/libexec/PlistBuddy -c "Set :LSMinimumSystemVersion 12.0" "$plist" 2>/dev/null; then
  /usr/libexec/PlistBuddy -c "Add :LSMinimumSystemVersion string 12.0" "$plist"
fi
codesign --force --deep --sign - --timestamp=none "$built" >/dev/null 2>&1
codesign --verify --deep --strict "$built"

dest=$root/target/$profile/bundle/macos/GitNexia.app
mkdir -p "$(dirname "$dest")"
rm -rf "$dest"
cp -R "$built" "$dest"
codesign --verify --deep --strict "$dest"
echo "build-macos-app: $dest"

if [ "$install" = 1 ]; then
  app_dir=${GITNEXIA_APP_DIR:-/Applications}
  target=$app_dir/GitNexia.app
  [ -w "$app_dir" ] || fail "no write access to $app_dir (install by hand, or grant it and retry)"
  install_new=$app_dir/.GitNexia.app.new.$$
  install_old=$app_dir/.GitNexia.app.old.$$
  rm -rf "$install_new" "$install_old"
  ditto "$dest" "$install_new"
  if [ -d "$target" ]; then
    mv "$target" "$install_old"
    if ! mv "$install_new" "$target"; then
      mv "$install_old" "$target"
      fail "couldn't put the new bundle in place; the old one is back"
    fi
    install_new=
    rm -rf "$install_old"
  else
    mv "$install_new" "$target"
    install_new=
  fi
  codesign --verify --deep --strict "$target"
  echo "build-macos-app: installed $target"
  if pgrep -x GitNexia >/dev/null 2>&1; then
    echo "build-macos-app: GitNexia is running; quit and reopen it to use this build" >&2
  fi
fi
