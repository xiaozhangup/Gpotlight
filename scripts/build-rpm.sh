#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RPMBUILD_DIR="$ROOT_DIR/target/rpmbuild"
NAME="gpotlight"

if ! command -v rpmbuild >/dev/null 2>&1; then
  echo "rpmbuild is required. Install it on Fedora with: sudo dnf install -y rpm-build" >&2
  exit 1
fi

mkdir -p \
  "$RPMBUILD_DIR/BUILD" \
  "$RPMBUILD_DIR/BUILDROOT" \
  "$RPMBUILD_DIR/RPMS" \
  "$RPMBUILD_DIR/SOURCES" \
  "$RPMBUILD_DIR/SPECS" \
  "$RPMBUILD_DIR/SRPMS" \
  "$RPMBUILD_DIR/TMP"

rm -f \
  "$RPMBUILD_DIR"/RPMS/*/"$NAME"-*.rpm \
  "$RPMBUILD_DIR"/SRPMS/"$NAME"-*.src.rpm

cd "$ROOT_DIR"
rpmbuild -bb --build-in-place "$ROOT_DIR/packaging/gpotlight.spec" \
  --define "_topdir $RPMBUILD_DIR" \
  --define "_tmppath $RPMBUILD_DIR/TMP"

find "$RPMBUILD_DIR/RPMS" -type f -name '*.rpm' -print
