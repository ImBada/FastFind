#!/bin/sh
# Renders assets/AppIcon.iconset from scripts/render_icon.swift, drawing each size directly.
set -eu
cd "$(dirname "$0")/.."
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
swiftc -O scripts/render_icon.swift -o "$tmp/render_icon"
out=assets/AppIcon.iconset
mkdir -p "$out"
for base in 16 32 128 256 512; do
  "$tmp/render_icon" "$base" "$out/icon_${base}x${base}.png"
  "$tmp/render_icon" "$((base * 2))" "$out/icon_${base}x${base}@2x.png"
done
echo "Wrote $out"
