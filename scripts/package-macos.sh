#!/usr/bin/env bash
# CI only: build x86_64 + arm64, merge into a universal binary, make tarball and .pkg.
set -euo pipefail

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
name="flowgen-${version}-macos-universal"

for t in x86_64-apple-darwin aarch64-apple-darwin; do
  cargo build --release --locked --target "$t"
done

mkdir -p dist "stage/$name/examples" stage/pkgroot/usr/local/bin
lipo -create -output "stage/$name/flowgen" \
  target/x86_64-apple-darwin/release/flowgen \
  target/aarch64-apple-darwin/release/flowgen
lipo -info "stage/$name/flowgen"
cp README.md "stage/$name/"
[ -f LICENSE ] && cp LICENSE "stage/$name/"
cp examples/*.toml "stage/$name/examples/"
tar -C stage -czf "dist/$name.tar.gz" "$name"

cp "stage/$name/flowgen" stage/pkgroot/usr/local/bin/flowgen
pkgbuild --root stage/pkgroot --identifier com.illumio.flowgen --version "$version" \
  --install-location / "dist/$name.pkg"

ls -l dist
