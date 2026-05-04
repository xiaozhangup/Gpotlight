#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RPMBUILD_DIR="$ROOT_DIR/target/rpmbuild"
NAME="gpotlight"
VERSION="$(awk '$1 == "Version:" { print $2; exit }' "$ROOT_DIR/packaging/gpotlight.spec")"
SOURCE_DIR="$NAME-$VERSION"
SOURCE_ARCHIVE="$SOURCE_DIR.tar.gz"
LOCAL_SPEC="$RPMBUILD_DIR/SPECS/$NAME.spec"

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
  "$RPMBUILD_DIR"/SRPMS/"$NAME"-*.src.rpm \
  "$RPMBUILD_DIR/SOURCES/$SOURCE_ARCHIVE" \
  "$LOCAL_SPEC"

cd "$ROOT_DIR"
git ls-files -z | tar --null --files-from=- --transform "s,^,$SOURCE_DIR/," -czf "$RPMBUILD_DIR/SOURCES/$SOURCE_ARCHIVE"
sed \
  -e "s#^VCS:.*#VCS: local#" \
  -e "s#^Source0:.*#Source0: $SOURCE_ARCHIVE#" \
  -e "s#^{{{ git_repo_setup_macro.*}}}#%setup -q -n $SOURCE_DIR#" \
  "$ROOT_DIR/packaging/gpotlight.spec" > "$LOCAL_SPEC"

rpmbuild -bb "$LOCAL_SPEC" \
  --define "_topdir $RPMBUILD_DIR" \
  --define "_tmppath $RPMBUILD_DIR/TMP"

find "$RPMBUILD_DIR/RPMS" -type f -name '*.rpm' -print
