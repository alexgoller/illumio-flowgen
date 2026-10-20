#!/usr/bin/env bash
# CI only: package an already-built musl binary as tarball, .deb and .rpm.
# Usage: scripts/package-linux.sh x86_64|aarch64
set -euo pipefail

arch="${1:?usage: package-linux.sh x86_64|aarch64}"
target="${arch}-unknown-linux-musl"
case "$arch" in
  x86_64) debarch=amd64 ;;
  aarch64) debarch=arm64 ;;
  *) echo "unknown arch $arch" >&2; exit 1 ;;
esac
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
name="flowgen-${version}-linux-${arch}"

mkdir -p dist "stage/$name/examples"
cp "target/$target/release/flowgen" README.md "stage/$name/"
[ -f LICENSE ] && cp LICENSE "stage/$name/"
cp examples/*.toml "stage/$name/examples/"
tar -C stage -czf "dist/$name.tar.gz" "$name"

cargo deb --no-build --no-strip --target "$target" --output "dist/flowgen_${version}_${debarch}.deb"
cargo generate-rpm --target "$target" --output "dist/flowgen-${version}-1.${arch}.rpm"

ls -l dist
