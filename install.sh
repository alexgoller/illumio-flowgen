#!/bin/sh
# flowgen installer: picks the right release tarball for this OS/arch,
# verifies its SHA256 and installs the binary.
#
#   curl -fsSL https://github.com/OWNER/REPO/releases/latest/download/install.sh | sh
#   sh install.sh --from ./dist --prefix ~/.local/bin
set -eu

# Replaced with the real "owner/repo" by the release workflow.
REPO="${FLOWGEN_REPO:-__GITHUB_REPOSITORY__}"
VERSION="latest"
FROM=""
PREFIX="/usr/local/bin"

usage() {
  cat <<USAGE
Usage: install.sh [--version vX.Y.Z] [--from DIR_OR_URL] [--prefix DIR]

  --version  release tag to install (default: latest)
  --from     directory or base URL that holds the release files
  --prefix   install directory (default: /usr/local/bin)
USAGE
}

die() {
  echo "install.sh: $*" >&2
  exit 1
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2:?--version needs a value}"; shift 2 ;;
    --from) FROM="${2:?--from needs a value}"; shift 2 ;;
    --prefix) PREFIX="${2:?--prefix needs a value}"; shift 2 ;;
    -h | --help) usage; exit 0 ;;
    *) usage >&2; die "unknown option: $1" ;;
  esac
done

os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux)
    case "$arch" in
      x86_64 | amd64) platform=linux-x86_64 ;;
      aarch64 | arm64) platform=linux-aarch64 ;;
      *) die "unsupported Linux architecture: $arch" ;;
    esac ;;
  Darwin) platform=macos-universal ;;
  *) die "unsupported OS: $os" ;;
esac

if [ -n "$FROM" ]; then
  base="$FROM"
else
  case "$REPO" in
    __*) die "no release repository configured; use --from DIR or set FLOWGEN_REPO=owner/repo" ;;
  esac
  if [ "$VERSION" = latest ]; then
    base="https://github.com/$REPO/releases/latest/download"
  else
    base="https://github.com/$REPO/releases/download/$VERSION"
  fi
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

fetch() {
  case "$base" in
    http://* | https://*) curl -fsSL "$base/$1" -o "$tmp/$1" ;;
    *) [ -f "$base/$1" ] && cp "$base/$1" "$tmp/$1" ;;
  esac
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

suffix="-$platform.tar.gz"
file=""
have_sums=no
if fetch SHA256SUMS 2>/dev/null; then
  have_sums=yes
  file=$(awk -v s="$suffix" '{ f = $2; sub(/^\*/, "", f); if (length(f) > length(s) && substr(f, length(f) - length(s) + 1) == s) { print f; exit } }' "$tmp/SHA256SUMS")
else
  case "$base" in
    http://* | https://*) die "cannot download $base/SHA256SUMS" ;;
  esac
  for candidate in "$base"/flowgen-*"$suffix"; do
    [ -f "$candidate" ] && file=$(basename "$candidate") && break
  done
fi
[ -n "$file" ] || die "no flowgen package for $platform in $base"

fetch "$file" || die "cannot get $base/$file"

if [ "$have_sums" = yes ]; then
  expected=$(awk -v f="$file" '{ g = $2; sub(/^\*/, "", g); if (g == f) { print $1; exit } }' "$tmp/SHA256SUMS")
  actual=$(sha256 "$tmp/$file")
  [ "$expected" = "$actual" ] || die "checksum mismatch for $file"
fi

tar -xzf "$tmp/$file" -C "$tmp"
dir="${file%.tar.gz}"
[ -f "$tmp/$dir/flowgen" ] || die "archive $file has no flowgen binary"

use_sudo=no
if ! mkdir -p "$PREFIX" 2>/dev/null || [ ! -w "$PREFIX" ]; then
  command -v sudo >/dev/null 2>&1 || die "$PREFIX is not writable and sudo is not available; use --prefix"
  use_sudo=yes
fi

as_root() {
  if [ "$use_sudo" = yes ]; then sudo "$@"; else "$@"; fi
}

as_root mkdir -p "$PREFIX"
as_root install -m 0755 "$tmp/$dir/flowgen" "$PREFIX/flowgen"
if [ "$os" = Darwin ]; then
  as_root xattr -d com.apple.quarantine "$PREFIX/flowgen" 2>/dev/null || true
fi

echo "installed $PREFIX/flowgen"

# Sample scenarios: <prefix>/../share/flowgen/examples when prefix ends in bin,
# otherwise <prefix>/examples. Failure here is not fatal.
if [ "$(basename "$PREFIX")" = bin ]; then
  examples_dir="$(dirname "$PREFIX")/share/flowgen/examples"
else
  examples_dir="$PREFIX/examples"
fi
install_examples() {
  as_root mkdir -p "$examples_dir" || return 1
  for f in "$tmp/$dir"/examples/*.toml; do
    as_root install -m 0644 "$f" "$examples_dir/$(basename "$f")" || return 1
  done
}
have_examples=no
for f in "$tmp/$dir"/examples/*.toml; do
  [ -f "$f" ] && have_examples=yes && break
done
if [ "$have_examples" = yes ]; then
  if install_examples; then
    echo "examples: $examples_dir"
  else
    echo "install.sh: warning: could not install examples to $examples_dir" >&2
  fi
fi
"$PREFIX/flowgen" --version
