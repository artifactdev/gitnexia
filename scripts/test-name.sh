#!/usr/bin/env bash
# The old names stay in the files listed in scripts/name-allowlist.txt: the rename's notes, the
# compatibility code that reads what older versions wrote, the tests that refuse their names, and
# the ids kept (the MSI's UpgradeCode, the metainfo's <replaces>).
#
#   GitNex / gitnex / GITNEX    the app's name until 1.6.0 (and the crates' prefix)
#   GitBolt / gitbolt           the app's name before 0.7.0 (another project's name too)
#
# `gitnex([^i]|$)` is the old GitNex name without the new GitNexia. Any other tracked file that has
# one of them fails, with its lines.
set -euo pipefail
cd "$(dirname "$0")/.."
allow=scripts/name-allowlist.txt
allowed=$(grep -v -e '^#' -e '^$' "$allow")
status=0
for old in 'git[ -]?bolt' 'git[ -]?nex([^i]|$)'; do
  bad=$(git grep -l -I -i -E "$old" -- . ":(exclude)$allow" | grep -vxF -f <(printf '%s\n' "$allowed") || true)
  if [ -n "$bad" ]; then
    echo "test-name: FAIL: an old name ($old) outside $allow:" >&2
    while read -r f; do git grep -n -I -i -E "$old" -- "$f" >&2; done <<<"$bad"
    status=1
  fi
done
[ "$status" -eq 0 ] || exit 1
echo "test-name: OK"
