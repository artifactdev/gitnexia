#!/usr/bin/env bash
# Self-test for .github/workflows/: every action pinned by commit with its version as a comment,
# every workflow starting from no permissions (each job asks for what it needs, never write-all),
# every checkout without persisted credentials, no pull_request_target, no secret but the
# workflow's own token and, in the jobs that build the .dmg, the macOS signing certificate's two
# and the notarization credentials' three (nor all of them, through secrets[...] or
# toJSON(secrets)) and no forced
# refspec (+HEAD:, +refs/); the site's deploy only from main and its downloads never force-pushed;
# then actionlint (with shellcheck on the run: scripts) over the site's workflows, when it's
# installed.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
wf=$(cd "$here/../.github/workflows" && pwd)
fail() { echo "test-workflows: FAIL: $*" >&2; exit 1; }

for f in "$wf"/*.yml; do
  name=$(basename "$f")
  grep -qx 'permissions: {}' "$f" || fail "$name: no top-level 'permissions: {}'"
  if grep -qE '^[[:space:]]*permissions:[[:space:]]*write-all' "$f"; then fail "$name: a job asks for write-all"; fi
  unpinned=$(grep -nE '^[[:space:]]*(- )?uses:' "$f" | grep -vE 'uses: [A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40} # [^ ]+$' || true)
  [ -z "$unpinned" ] || fail "$name: actions not pinned by commit with a version comment: $unpinned"
  checkouts=$(grep -cE 'uses: actions/checkout@' "$f" || true)
  unpersisted=$(grep -cE '^[[:space:]]*persist-credentials: false$' "$f" || true)
  [ "$checkouts" = "$unpersisted" ] || fail "$name: $checkouts checkouts, $unpersisted with persist-credentials: false"
  if grep -q 'pull_request_target' "$f"; then fail "$name: pull_request_target runs fork code with a write token"; fi
  allowed='secrets\.GITHUB_TOKEN'
  case $name in
    release.yml) allowed+='|secrets\.MACOS_CERTIFICATE_(P12|PASSWORD)|secrets\.APPLE_(ID|PASSWORD|TEAM_ID)' ;;
    mac-port.yml) allowed+='|secrets\.MACOS_CERTIFICATE_(P12|PASSWORD)' ;;
  esac
  others=$(grep -oE 'secrets\.[A-Za-z0-9_]+' "$f" | grep -vxE "$allowed" || true)
  [ -z "$others" ] || fail "$name: secrets other than GITHUB_TOKEN: $others"
  if grep -qiE 'secrets[[:space:]]*\[|tojson\([[:space:]]*secrets' "$f"; then fail "$name: reads secrets by index or all at once"; fi
  if grep -qE '[[:space:]]["'"'"']?\+(HEAD|refs/)' "$f"; then fail "$name: pushes a forced refspec (+HEAD: or +refs/)"; fi
done

beta=$wf/system-webview-beta.yml
grep -qE '^  workflow_dispatch:$' "$beta" || fail "system-webview-beta.yml: not manually dispatched"
grep -qF 'os: windows-latest' "$beta" && grep -qF 'os: macos-15' "$beta" ||
  fail "system-webview-beta.yml: missing beta platforms"
grep -qF -- '--no-default-features --features system-webview-beta,tauri/custom-protocol' "$beta" ||
  fail "system-webview-beta.yml: binary build doesn't select the system runtime without CEF"
if grep -qE 'contents: write|gh release' "$beta"; then
  fail "system-webview-beta.yml: beta artifacts must not modify regular releases"
fi
grep -q 'actions/upload-artifact@' "$beta" || fail "system-webview-beta.yml: no downloadable packages"
grep -q 'package-system-webview-beta.py' "$beta" || fail "system-webview-beta.yml: no beta packaging"
grep -qF -- '--no-default-features --features system-webview-beta \' "$beta" ||
  fail "system-webview-beta.yml: notices aren't for the beta dependency graph"

macos=$(sed -n '/^  macos:$/,/^  release:$/p' "$wf/release.yml")
grep -qF "runs-on: \${{ inputs.macos_runner || 'macos-15' }}" <<<"$macos" ||
  fail "release.yml: macOS doesn't use the selected hosted runner with macos-15 as default"
grep -qF "key: release-\${{ inputs.macos_runner || 'macos-15' }}" <<<"$macos" ||
  fail "release.yml: Rust cache isn't separated by macOS image"
grep -qF -- "-\${{ inputs.macos_runner || 'macos-15' }}-arm64" <<<"$macos" ||
  fail "release.yml: cargo tools cache isn't separated by macOS image"
choices=$(sed -n '/^      macos_runner:$/,/^[^ ]/p' "$wf/release.yml")
grep -qx '        type: choice' <<<"$choices" ||
  fail "release.yml: macos_runner isn't a choice input"
grep -qx '        default: macos-15' <<<"$choices" ||
  fail "release.yml: macos_runner doesn't default to macos-15"
grep -qx '          - macos-15' <<<"$choices" &&
  grep -qx '          - macos-26' <<<"$choices" ||
  fail "release.yml: missing hosted macOS runner choices"
if grep -qE 'self-hosted|RUNNERS_READ_TOKEN' "$wf/release.yml"; then
  fail "release.yml: release runner selection isn't hosted-only"
fi

site=$wf/site.yml
release=$wf/site-release.yml
[ -f "$site" ] && [ -f "$release" ] || fail "site.yml or site-release.yml is missing"
grep -qE '^  release:$' "$release" || fail "site-release.yml: doesn't run on published releases"
grep -qE '^    types: \[published\]$' "$release" || fail "site-release.yml: release trigger isn't published"
grep -qF 'GH_REPO: ${{ github.repository }}' "$release" || fail "site-release.yml: doesn't use the current repository for release URLs"
grep -q 'gh release edit "\$TAG" --draft=false' "$wf/release.yml" || fail "release.yml: doesn't publish the release after uploading assets"
grep -q "if: github.ref == 'refs/heads/main'" "$site" || fail "site.yml: the deploy isn't limited to main"
grep -q 'name: github-pages' "$site" || fail "site.yml: the deploy isn't in the github-pages environment"
if grep -qE 'pull_request' "$site" "$release"; then fail "the site's workflows run on pull requests"; fi
grep -q 'gh workflow run site.yml --ref main' "$release" || fail "site-release.yml: doesn't start the deploy on main"
if grep -qE 'push .*(-f|--force)' "$release"; then fail "site-release.yml: force-pushes"; fi

if command -v actionlint >/dev/null 2>&1; then
  actionlint "$site" "$release"
else
  echo "test-workflows: actionlint isn't installed; skipped linting the site's workflows"
fi
echo "test-workflows: OK"
