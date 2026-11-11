#!/bin/sh
# Builds the release program and the .deb, .rpm and Arch packages into dist/.
# Used by the GitHub workflow; also works on a Debian/Ubuntu machine with the build tools installed.
set -eu
cd "$(dirname "$0")/.."

NFPM_VERSION="${NFPM_VERSION:-2.43.0}"
VERSION="${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
export VERSION
export HOMEPAGE="${HOMEPAGE:-https://github.com/}"

cargo build --release --locked
# The Dolphin badge plugin must have been built in (needs g++, Qt 6 and KF6 KIO).
if [ "${REQUIRE_DOLPHIN_PLUGIN:-1}" = 1 ] && ! find target/release/build -name drivelinux_overlay.so -size +1k | grep -q .; then
  echo "The Dolphin plugin was not built: install g++, qt6-base-dev, qt6-base-dev-tools and libkf6kiocore6" >&2
  exit 1
fi

if ! command -v nfpm >/dev/null 2>&1; then
  tmp="$(mktemp -d)"
  curl -fsSL -o "$tmp/nfpm.tar.gz" "https://github.com/goreleaser/nfpm/releases/download/v${NFPM_VERSION}/nfpm_${NFPM_VERSION}_Linux_x86_64.tar.gz"
  tar -xzf "$tmp/nfpm.tar.gz" -C "$tmp" nfpm
  NFPM="$tmp/nfpm"
else
  NFPM=nfpm
fi

mkdir -p dist
for format in deb rpm archlinux; do
  "$NFPM" package -f packaging/nfpm.yaml -p "$format" -t dist/
done
ls -l dist/
