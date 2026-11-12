#!/usr/bin/env bash
# Publishes a GitHub release, which installed copies pick up as an automatic update: tag
# v<version in Cargo.toml> on HEAD, with FastFind.zip (what the app installs) and the DMG.
#   1. bump `version` in Cargo.toml, commit and push
#   2. scripts/release.sh                 (more arguments go to `gh release create`, e.g. --notes "…")
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
tag="v$version"
if [ -n "$(git status --porcelain)" ]; then
  echo "Commit your changes first: the release is built from HEAD." >&2
  exit 1
fi
if git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
  echo "$tag is already released: bump the version in Cargo.toml first." >&2
  exit 1
fi
if [ "$(git rev-parse HEAD)" != "$(git rev-parse '@{upstream}' 2>/dev/null || true)" ]; then
  echo "Push HEAD first: the tag is made on it." >&2
  exit 1
fi

scripts/bundle.sh --zip --dmg
gh release create "$tag" dist/FastFind.zip dist/FastFindLatest.dmg \
  --title "FastFind $version" --target "$(git rev-parse HEAD)" --generate-notes "$@"
