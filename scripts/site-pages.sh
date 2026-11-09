#!/usr/bin/env bash
# The GitHub Pages artefact, site/dist/ (`just site-pages`; .github/workflows/site.yml runs it
# before deploying): the landing page (`just site-build`, which runs the page's own checks), the
# demo under demo/ (`just demo-build`: the WebAssembly graph, then `vite build --mode demo` over the
# bundles in site/demo-data/) and the 404 page, then ui/scripts/check-site.mjs over the result.
# GITNEXIA_SITE_BASE is the path the site is served under: /gitnexia/ on GitHub Pages; GITNEXIA_SITE_ORIGIN its
# scheme and host, https://artifactdev.github.io by default (the link previews' URLs). See docs/website.md.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
export GITNEXIA_SITE_BASE=${GITNEXIA_SITE_BASE:-/gitnexia/}
export GITNEXIA_SITE_ORIGIN=${GITNEXIA_SITE_ORIGIN:-https://artifactdev.github.io}
base=$GITNEXIA_SITE_BASE

# The page (with its own checks, site/build/check.ts), then the demo under demo/; either order works.
just site-build demo-build
# The 404 page is served at any depth, so its links are absolute: under the base.
sed "s|href=\"/|href=\"$base|g" site/404.html > site/dist/404.html
node ui/scripts/check-site.mjs --base "$base" site/dist
