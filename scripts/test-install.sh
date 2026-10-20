#!/bin/sh
# Tests install.sh against a stub tarball (no real build needed).
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64 | Linux-amd64) platform=linux-x86_64 ;;
  Linux-aarch64 | Linux-arm64) platform=linux-aarch64 ;;
  Darwin-*) platform=macos-universal ;;
  *) echo "unsupported test platform" >&2; exit 1 ;;
esac

name="flowgen-9.9.9-$platform"
mkdir -p "$work/$name/examples" "$work/dist"
printf '#!/bin/sh\necho "flowgen 9.9.9 (stub)"\n' > "$work/$name/flowgen"
chmod +x "$work/$name/flowgen"
echo "# readme" > "$work/$name/README.md"
tar -C "$work" -czf "$work/dist/$name.tar.gz" "$name"

if command -v sha256sum >/dev/null 2>&1; then
  (cd "$work/dist" && sha256sum "$name.tar.gz" > SHA256SUMS)
else
  (cd "$work/dist" && shasum -a 256 "$name.tar.gz" > SHA256SUMS)
fi

# 1. Happy path with checksum verification.
out=$(sh "$here/install.sh" --from "$work/dist" --prefix "$work/bin")
echo "$out" | grep -q "flowgen 9.9.9 (stub)" || { echo "FAIL: version not printed: $out" >&2; exit 1; }
[ -x "$work/bin/flowgen" ] || { echo "FAIL: binary not installed" >&2; exit 1; }

# 2. Tampered checksum must abort.
awk '{ print "0000000000000000000000000000000000000000000000000000000000000000  " $2 }' "$work/dist/SHA256SUMS" > "$work/dist/SHA256SUMS.bad"
mv "$work/dist/SHA256SUMS.bad" "$work/dist/SHA256SUMS"
if sh "$here/install.sh" --from "$work/dist" --prefix "$work/bin2" >/dev/null 2>&1; then
  echo "FAIL: bad checksum accepted" >&2; exit 1
fi

# 3. Without SHA256SUMS a local dir still works (CI verify job uses this).
rm "$work/dist/SHA256SUMS"
sh "$here/install.sh" --from "$work/dist" --prefix "$work/bin3" >/dev/null
[ -x "$work/bin3/flowgen" ] || { echo "FAIL: install without SHA256SUMS" >&2; exit 1; }

echo "install.sh tests passed"
