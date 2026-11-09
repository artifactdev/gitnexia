#!/usr/bin/env bash
# Self-test for package-rpm.sh: the pure parts of scripts/rpm-pkg.py (versions, the dependency
# table and its unknown-name failure), then a synthetic .deb through the whole script (the file
# name, the header's fields, Requires, the payload's modes and links, the license flags, the
# system directories left out, chrome-sandbox root:root 4755), read back with rpm-pkg.py's own
# reader. With GITNEXIA_TEST_DOCKER=1, rpm itself checks it in fedora:latest (rpm -K, the query
# tags, and an install without dependencies plus rpm -V).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
helper="$here/rpm-pkg.py"
fail() { echo "test-package-rpm: FAIL: $*" >&2; exit 1; }
eq() { [ "$2" = "$3" ] || fail "$1: expected '$3', got '$2'"; }

eq version "$(python3 "$helper" version '0.1.0+202610051325.ab4dbf9e')" '0.1.0+202610051325.ab4dbf9e'
eq version-pre "$(python3 "$helper" version '0.1.0~alpha.1')" '0.1.0~alpha.1'
python3 "$helper" version '1:0.1.0' 2>/dev/null && fail "an epoch ':' was accepted"
python3 "$helper" version '0.1.0-1' 2>/dev/null && fail "a '-' was accepted"
eq filever "$(python3 "$helper" filever '0.1.0~alpha.1')" '0.1.0-alpha.1'
eq filever-stamp "$(python3 "$helper" filever '0.1.0~rc.2+202610051325.ab4dbf9e')" '0.1.0-rc.2+202610051325.ab4dbf9e'
eq filever-plain "$(python3 "$helper" filever '0.2.0')" '0.2.0'

deps=$(python3 "$helper" depends 'libasound2t64 (>= 1.0.17), libc6 (>= 2.39), libgtk-4-1 (>= 4.13.8), libnss3 (>= 2:3.30), git (>= 1:2.40)' | tr '\n' '|')
eq depends "$deps" 'git-core >= 2.40|libgtk-4.so.1()(64bit)|'
err=$(python3 "$helper" depends 'libgtk-4-1, foo9 (>= 1), git' 2>&1 >/dev/null) && fail "an unknown dependency was accepted"
grep -q 'unknown Debian dependencies.*foo9' <<<"$err" || fail "unclear unknown-dependency message: $err"

# The whole script on a synthetic .deb.
command -v dpkg-deb >/dev/null || { echo "test-package-rpm: pure parts OK; package test skipped (no dpkg-deb)"; exit 0; }
umask 022
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
pkg="$work/pkg"
mkdir -p "$pkg/DEBIAN" "$pkg/usr/share/GitNexia/locales" "$pkg/usr/bin" "$pkg/usr/share/applications" "$pkg/usr/share/icons/hicolor/32x32/apps"
cp /usr/bin/true "$pkg/usr/share/GitNexia/gitnexia"
printf '#!/bin/sh\n' > "$pkg/usr/share/GitNexia/chrome-sandbox"
chmod 4755 "$pkg/usr/share/GitNexia/chrome-sandbox"
printf 'pak' > "$pkg/usr/share/GitNexia/locales/en-US.pak"
printf '[Desktop Entry]\n' > "$pkg/usr/share/applications/GitNexia.desktop"
printf 'png' > "$pkg/usr/share/icons/hicolor/32x32/apps/gitnexia.png"
ln -s ../share/GitNexia/gitnexia "$pkg/usr/bin/gitnexia"
notices="LICENSE THIRD-PARTY-NOTICES-rust.txt THIRD-PARTY-NOTICES-ui.txt CEF-LICENSE.txt CHROMIUM-CREDITS.html.gz DICTIONARY-en-US-LICENSE.txt"
mkdir -p "$pkg/usr/share/doc/gitnexia"
for f in $notices; do printf 'notice %s\n' "$f" > "$pkg/usr/share/doc/gitnexia/$f"; done
printf "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n" > "$pkg/usr/share/doc/gitnexia/copyright"
printf 'Rust notices\n\nSummary\n-------\n  MIT          40\n  Apache-2.0    2\n\n\nMIT\n' \
  > "$pkg/usr/share/doc/gitnexia/THIRD-PARTY-NOTICES-rust.txt"
printf 'UI notices\n\nSummary\n-------\n  MIT     9\n  ISC     3\n\n\nMIT\n' \
  > "$pkg/usr/share/doc/gitnexia/THIRD-PARTY-NOTICES-ui.txt"
mkdir -p "$pkg/usr/share/metainfo"
printf '<component>\n  <id>com.gitnexia.desktop</id>\n  <pkgname>gitnexia</pkgname>\n</component>\n' \
  > "$pkg/usr/share/metainfo/com.gitnexia.desktop.metainfo.xml"
mkdir -p "$pkg/usr/share/GitNexia/dictionaries"
printf 'BDic' > "$pkg/usr/share/GitNexia/dictionaries/en-US-10-2.bdic"
printf 'deb\n' > "$pkg/usr/share/GitNexia/install-kind"
cat > "$pkg/DEBIAN/control" <<'EOT'
Package: gitnexia
Version: 0.1.0~alpha.1+202610051325.ab4dbf9e
Architecture: amd64
Maintainer: package-rpm self-test <test@example.invalid>
Homepage: https://gitnexia.example.invalid
Depends: libgtk-4-1 (>= 4.13.8), libc6 (>= 2.39), git (>= 1:2.40)
Description: A fast desktop Git client
 First paragraph.
 .
 Second paragraph.
EOT
dpkg-deb --root-owner-group -Zgzip -b "$pkg" "$work/t.deb" >/dev/null
SOURCE_DATE_EPOCH='' "$here/package-rpm.sh" "$work/t.deb" "$work/out" >/dev/null
out="$work/out/GitNexia-0.1.0-alpha.1+202610051325.ab4dbf9e-1.x86_64.rpm"
[ -f "$out" ] || fail "no package: $(ls "$work/out")"

# What the header says, through the reader package-rpm.sh's verify uses.
dump() {
  python3 -B - "$helper" "$out" <<'EOP'
import importlib.util, sys
spec = importlib.util.spec_from_file_location("rpmpkg", sys.argv[1]); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
sig, h, _, _ = m.read_rpm(sys.argv[2])
files = [h[1118][d] + b for d, b in zip(h[1116], h[1117])]
print("name", h[1000]); print("version", h[1001]); print("release", h[1002]); print("arch", h[1022])
print("summary", h[1004]); print("description", h[1005].replace("\n", "|")); print("url", h[1020]); print("license", h[1014])
print("buildtime", h[1006][0])
for n, f, v in zip(h[1049], h[1048], h[1050]):
    print("require", n, f, v)
for n, f, v in zip(h[1047], h[1112], h[1113]):
    print("provide", n, f, v)
for n, f, v in zip(h[1090], h[1114], h[1115]):
    print("obsolete", n, f, v)
for path, mode, link, flags, user in zip(files, h[1030], h[1036], h[1037], h[1039]):
    print("file", oct(mode), user, flags, path, link)
EOP
}
info=$(dump)
stamp=$(date -u -d '2026-10-05 13:25' +%s)
for kv in 'name gitnexia' 'version 0.1.0~alpha.1+202610051325.ab4dbf9e' 'release 1' 'arch x86_64' \
          'summary A fast desktop Git client' 'description First paragraph.||Second paragraph.' \
          'url https://gitnexia.example.invalid' 'license MIT AND Apache-2.0 AND BSD-3-Clause AND ISC AND LicenseRef-SCOWL' \
          "buildtime $stamp" 'require git-core 12 2.40' 'require libgtk-4.so.1()(64bit) 0 ' \
          'require libc.so.6()(64bit) 0 ' 'require rpmlib(TildeInVersions) 16777226 4.10.0-1' \
          'require rpmlib(PayloadIsZstd) 16777226 5.4.18-1' \
          'provide gitnexia 8 0.1.0~alpha.1+202610051325.ab4dbf9e-1' \
          'provide gitnexia(x86-64) 8 0.1.0~alpha.1+202610051325.ab4dbf9e-1' \
          'provide gitnex 8 0.1.0~alpha.1+202610051325.ab4dbf9e-1' 'obsolete gitnex 0 ' \
          'file 0o104755 root 0 /usr/share/GitNexia/chrome-sandbox ' \
          'file 0o120777 root 0 /usr/bin/gitnexia ../share/GitNexia/gitnexia' \
          'file 0o40755 root 0 /usr/share/GitNexia ' 'file 0o40755 root 0 /usr/share/GitNexia/locales ' \
          'file 0o40755 root 0 /usr/share/doc/gitnexia ' \
          'file 0o120777 root 0 /usr/share/licenses/gitnexia ../doc/gitnexia' \
          'file 0o100644 root 0 /usr/share/icons/hicolor/32x32/apps/gitnexia.png '; do
  grep -qxF "$kv" <<<"$info" || fail "the header lacks '$kv': $info"
done
for f in $notices copyright; do
  grep -qxF "file 0o100644 root 130 /usr/share/doc/gitnexia/$f " <<<"$info" || fail "$f isn't a %license file: $info"
done
# The system's directories stay the system's.
for d in /usr /usr/bin /usr/share /usr/share/applications /usr/share/icons/hicolor/32x32/apps /usr/share/licenses; do
  grep -q " $d \$" <<<"$info" && fail "the package owns $d"
done
# The ELF scan found /usr/bin/true's libc, and no library the payload brings.
grep -q '^require libc\.so\.6(GLIBC_2\.[0-9.]*)(64bit) 0 $' <<<"$info" || fail "no versioned libc requirement: $info"
# The edited files.
zstd_payload() { python3 -B - "$helper" "$out" <<'EOP'
import importlib.util, sys
spec = importlib.util.spec_from_file_location("rpmpkg", sys.argv[1]); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
import subprocess
_, _, _, payload = m.read_rpm(sys.argv[2])
raw = subprocess.run(["zstd", "-dcq"], input=payload, capture_output=True, check=True).stdout
for name, mode, uid, gid, size, body in m.cpio_entries(raw):
    if name.endswith(("install-kind", ".metainfo.xml")):
        print(name, body.decode().replace("\n", "|"))
EOP
}
payload=$(zstd_payload)
grep -qxF './usr/share/GitNexia/install-kind rpm|' <<<"$payload" || fail "install-kind isn't rpm: $payload"
grep -qF '<pkgname>gitnexia</pkgname>' <<<"$payload" || fail "the metainfo doesn't name gitnexia: $payload"

# rpm's own reading of it, when Docker is there to run it.
if [ "${GITNEXIA_TEST_DOCKER:-}" = 1 ]; then
  docker run --rm -v "$out:/t.rpm:ro" fedora:latest bash -c '
    set -euo pipefail
    rpm -Kv /t.rpm
    rpm -qp --qf "%{NAME} %{VERSION} %{RELEASE} %{ARCH}\n" /t.rpm | grep -qx "gitnexia 0.1.0~alpha.1+202610051325.ab4dbf9e 1 x86_64"
    rpm -qplv /t.rpm
    rpm -qpL /t.rpm | grep -qx /usr/share/doc/gitnexia/LICENSE
    rpm -i --nodeps /t.rpm
    rpm -V --nodeps gitnexia
    [ "$(stat -c "%U:%G %a" /usr/share/GitNexia/chrome-sandbox)" = "root:root 4755" ]
    [ -s /usr/share/licenses/gitnexia/LICENSE ]
    echo "rpm read it: OK"' || fail "rpm rejected the package"
fi

# A .deb without a license notice, the dictionary or install-kind fails, as does an unknown
# dependency or a directory that is neither GitNexia's nor the system's.
: > "$pkg/usr/share/doc/gitnexia/CEF-LICENSE.txt"
dpkg-deb --root-owner-group -Zgzip -b "$pkg" "$work/n.deb" >/dev/null
"$here/package-rpm.sh" "$work/n.deb" "$work/out3" 2>/dev/null && fail "package-rpm accepted an empty CEF-LICENSE.txt"
printf 'notice\n' > "$pkg/usr/share/doc/gitnexia/CEF-LICENSE.txt"
mv "$pkg/usr/share/GitNexia/dictionaries/en-US-10-2.bdic" "$work/dict"
dpkg-deb --root-owner-group -Zgzip -b "$pkg" "$work/n.deb" >/dev/null
"$here/package-rpm.sh" "$work/n.deb" "$work/out3" 2>/dev/null && fail "package-rpm accepted a .deb without the dictionary"
mv "$work/dict" "$pkg/usr/share/GitNexia/dictionaries/en-US-10-2.bdic"
rm "$pkg/usr/share/GitNexia/install-kind"
dpkg-deb --root-owner-group -Zgzip -b "$pkg" "$work/n.deb" >/dev/null
"$here/package-rpm.sh" "$work/n.deb" "$work/out3" 2>/dev/null && fail "package-rpm accepted a .deb without install-kind"
printf 'deb\n' > "$pkg/usr/share/GitNexia/install-kind"
mkdir -p "$pkg/usr/lib/gitnexia"
printf 'x' > "$pkg/usr/lib/gitnexia/x"
dpkg-deb --root-owner-group -Zgzip -b "$pkg" "$work/n.deb" >/dev/null
err=$("$here/package-rpm.sh" "$work/n.deb" "$work/out3" 2>&1 >/dev/null) && fail "package-rpm accepted an unknown directory"
grep -q 'usr/lib.*OWNED_DIRS or SYSTEM_DIRS' <<<"$err" || fail "unclear unknown-directory message: $err"
rm -r "$pkg/usr/lib"
sed -i 's/^Depends: .*/Depends: libgtk-4-1, foo9/' "$pkg/DEBIAN/control"
dpkg-deb --root-owner-group -Zgzip -b "$pkg" "$work/u.deb" >/dev/null
"$here/package-rpm.sh" "$work/u.deb" "$work/out2" 2>/dev/null && fail "package-rpm accepted an unknown dependency"
echo "test-package-rpm: OK"
