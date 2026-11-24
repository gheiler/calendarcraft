#!/bin/sh
# Builds the web app and packs it the way artcraftCloud's installer expects:
# dist-release/calendarcraft-web-<version>.zip (index.html at the root) and SHA256SUMS.txt.
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ "${SKIP_BUILD:-}" = 1 ] || trunk build --release
out=dist-release
rm -rf "$out" && mkdir -p "$out"
zip_name="calendarcraft-web-$version.zip"
(
  cd dist && rm -f .trunk-lock && rm -rf .stage
  if command -v zip >/dev/null; then zip -qr "../$out/$zip_name" .; else python3 -m zipfile -c "../$out/$zip_name" ./*; fi
)
(cd "$out" && sha256sum "$zip_name" > SHA256SUMS.txt)
# The host checks for the bridge before installing.
unzip -p "$out/$zip_name" index.html | grep -q "host=parent"
unzip -p "$out/$zip_name" '*.wasm' | grep -aq "calendarcraft:ready"
echo "$out/$zip_name"
