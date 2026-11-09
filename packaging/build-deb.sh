#!/usr/bin/env bash
# Builds ferrisfetch_<version>_amd64.deb.
#
# Assembles the DEBIAN tree by hand and calls dpkg-deb. Kept as a script rather
# than a Cargo helper so the CI container needs nothing but dpkg-dev.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="${1:?usage: build-deb.sh <version> <outdir>}"
OUTDIR="${2:-dist}"
ARCH="${ARCH:-amd64}"
MAINTAINER="${MAINTAINER:-babichgh <john@babich.pw>}"

STAGE="$(mktemp -d)"

# On failure leave the staged tree behind and say where it is: a CI job that
# just says "exit 1" tells you nothing.
cleanup() {
  local status=$?
  if [[ $status -ne 0 ]]; then
    echo "==> build-deb.sh failed (exit $status); staged tree kept at $STAGE" >&2
    echo "--- staged contents ---" >&2
    find "$STAGE" -maxdepth 4 >&2 || true
    [[ -f "$STAGE/DEBIAN/control" ]] && { echo "--- DEBIAN/control ---" >&2; cat "$STAGE/DEBIAN/control" >&2; }
  else
    rm -rf "$STAGE"
  fi
  return $status
}
trap cleanup EXIT

echo "==> building release binary"
# --locked pins dependencies to Cargo.lock. --offline is deliberately not
# used: a clean container has no registry cache, so it would fail.
cargo fetch --locked
cargo build --release --locked

install -Dm 0755 target/release/ferrisfetch "$STAGE/usr/bin/ferrisfetch"
install -Dm 0644 LICENSE          "$STAGE/usr/share/doc/ferrisfetch/LICENSE"
install -Dm 0644 LICENSE-fastfetch "$STAGE/usr/share/doc/ferrisfetch/LICENSE-fastfetch"
install -Dm 0644 README.md        "$STAGE/usr/share/doc/ferrisfetch/README.md"

# Keep the metadata in sync with the spec so both packages describe the same
# release.
install -d "$STAGE/DEBIAN"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@ARCH@/$ARCH/g" \
  packaging/debian/control.in > "$STAGE/DEBIAN/control"
# dpkg-deb rejects a control file whose last field lacks a terminating
# newline, and that is easy to lose to an editor or .gitattributes.
printf '\n' >> "$STAGE/DEBIAN/control"
chmod 0644 "$STAGE/DEBIAN/control"

# gzipped man page, if one has been generated; optional.
if [[ -f man/ferrisfetch.1 ]]; then
  install -Dm 0644 man/ferrisfetch.1 "$STAGE/usr/share/man/man1/ferrisfetch.1"
  gzip -9n "$STAGE/usr/share/man/man1/ferrisfetch.1"
fi

mkdir -p "$OUTDIR"
DEB="$OUTDIR/ferrisfetch_${VERSION}_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$STAGE" "$DEB" >/dev/null
echo "==> $DEB"
