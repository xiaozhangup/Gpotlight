#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RPMBUILD_DIR="$ROOT_DIR/target/rpmbuild"

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

rpmbuild -bb "$ROOT_DIR/packaging/gpotlight.spec" \
  --define "_topdir $RPMBUILD_DIR" \
  --define "_tmppath $RPMBUILD_DIR/TMP" \
  --define "gpotlight_project_dir $ROOT_DIR"

find "$RPMBUILD_DIR/RPMS" -type f -name '*.rpm' -print
