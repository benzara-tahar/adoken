#!/usr/bin/env bash
# Package target/bundle/adoken.app into target/bundle/adoken-<version>.dmg with
# the classic "drag adoken onto Applications" layout. Uses hdiutil only.
#
# Usage: scripts/dmg.sh [--no-build]
#   (default)   scripts/bundle.sh --release first, then package
#   --no-build  package the existing target/bundle/adoken.app as is
#
# The app inside carries a local signature ("adoken dev" or ad hoc), which
# other Macs don't trust: Gatekeeper blocks the first launch until the
# recipient allows it (docs/release.md, "Sharing a test build"). A dmg anyone
# can open without that step needs Developer ID signing and notarization:
# docs/release.md, "Distribution".

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

BUILD=true
for arg in "$@"; do
  case "$arg" in
    --no-build) BUILD=false ;;
    -h | --help)
      sed -n '2,14p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "dmg.sh: unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

if $BUILD; then
  "$ROOT/scripts/bundle.sh" --release
fi

OUT_DIR="$ROOT/target/bundle"
APP="$OUT_DIR/adoken.app"
[[ -d "$APP" ]] || { echo "dmg.sh: $APP missing; run scripts/bundle.sh --release" >&2; exit 1; }
codesign --verify --strict "$APP"

VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist")"
DMG="$OUT_DIR/adoken-$VERSION.dmg"
STAGE="$OUT_DIR/dmg-stage"

rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
ditto "$APP" "$STAGE/adoken.app"
ln -s /Applications "$STAGE/Applications"

# ULFO (lzfse) is the smallest read-only format; it needs macOS 10.11+, below
# the app's LSMinimumSystemVersion of 12.0.
hdiutil create -quiet -volname "adoken $VERSION" -srcfolder "$STAGE" \
  -fs HFS+ -format ULFO -ov "$DMG"
rm -rf "$STAGE"
hdiutil verify -quiet "$DMG"

echo "dmg: $DMG ($(du -h "$DMG" | cut -f1 | tr -d ' '), $(lipo -archs "$APP/Contents/MacOS/adoken"))"
