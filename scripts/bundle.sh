#!/usr/bin/env bash
# Assemble target/bundle/adoken.app from the cargo build:
#   Contents/MacOS/adoken        the binary (debug by default)
#   Contents/Info.plist          crates/app/bundle/Info.plist, version filled in
#   Contents/Resources/adoken.icns  from crates/app/bundle/icon-1024.png
# then sign it with the local "adoken dev" identity (scripts/dev-signing.sh,
# set up automatically on first use: one macOS password prompt, ever). A
# stable signature means the Keychain's "Always Allow" survives rebuilds; an
# ad hoc one changes with every build, so macOS would ask again each time.
#
# Usage: scripts/bundle.sh [--release] [--no-build] [--run] [--adhoc]
#   --release   cargo build --release (thin LTO, stripped) and bundle that
#   --no-build  bundle the existing binary as is
#   --run       quit a running adoken, then open the new bundle
#   --adhoc     sign ad hoc instead (CI, or no signing setup wanted)
#
# Run it: open target/bundle/adoken.app            (Finder-style launch)
#         open --env ADOKEN_DEMO=1 target/bundle/adoken.app   (demo org)
#         target/bundle/adoken.app/Contents/MacOS/adoken      (logs in the terminal)
#
# Real Developer ID signing, notarization and stapling: docs/release.md.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
command -v cargo >/dev/null 2>&1 || PATH="$HOME/.cargo/bin:$PATH"

PROFILE=debug
BUILD=true
RUN=false
ADHOC=false
for arg in "$@"; do
  case "$arg" in
    --release) PROFILE=release ;;
    --no-build) BUILD=false ;;
    --run) RUN=true ;;
    --adhoc) ADHOC=true ;;
    -h | --help)
      sed -n '2,22p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "bundle.sh: unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
# CFBundleVersion must grow monotonically for LaunchServices; the commit count
# does, and a dirty dev tree doesn't matter for an ad hoc build.
BUILD_NUMBER="$(git -C "$ROOT" rev-list --count HEAD 2>/dev/null || echo 0)"
BUNDLE_SRC="$ROOT/crates/app/bundle"
OUT_DIR="$ROOT/target/bundle"
APP="$OUT_DIR/adoken.app"
CONTENTS="$APP/Contents"

cd "$ROOT"
if $BUILD; then
  if [[ "$PROFILE" == release ]]; then
    cargo build --release -p adoken --bin adoken
  else
    cargo build -p adoken --bin adoken
  fi
fi
BINARY="$ROOT/target/$PROFILE/adoken"
[[ -x "$BINARY" ]] || {
  echo "bundle.sh: missing $BINARY (drop --no-build)" >&2
  exit 1
}

rm -rf "$APP"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"
install -m 755 "$BINARY" "$CONTENTS/MacOS/adoken"
sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD_NUMBER/" \
  "$BUNDLE_SRC/Info.plist" >"$CONTENTS/Info.plist"
plutil -lint -s "$CONTENTS/Info.plist"
printf 'APPL????' >"$CONTENTS/PkgInfo"

# Icon: sips scales the pre-masked master (tile, margins and shadow are baked
# in by crates/app/bundle/render-icon.ts), iconutil packs the iconset.
ICONSET="$OUT_DIR/adoken.iconset"
rm -rf "$ICONSET" && mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$BUNDLE_SRC/icon-1024.png" \
    --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  retina=$((size * 2))
  sips -z "$retina" "$retina" "$BUNDLE_SRC/icon-1024.png" \
    --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$CONTENTS/Resources/adoken.icns"
rm -rf "$ICONSET"

# Licenses that must ship with any binary (THIRD_PARTY_NOTICES.md).
mkdir -p "$CONTENTS/Resources/licenses"
cp "$ROOT/LICENSE" "$CONTENTS/Resources/licenses/adoken-LICENSE"
cp "$ROOT/THIRD_PARTY_NOTICES.md" "$CONTENTS/Resources/licenses/"
cp "$ROOT/assets/icons/lucide/LICENSE" "$CONTENTS/Resources/licenses/lucide-LICENSE"
cp "$ROOT/assets/fonts/jetbrains-mono/OFL.txt" "$CONTENTS/Resources/licenses/JetBrainsMono-OFL.txt"

# Stable local signature (see the header); ad hoc with --adhoc or in CI.
if [[ "$ADHOC" == false && -z "${CI:-}" ]] && "$ROOT/scripts/dev-signing.sh"; then
  codesign --force --keychain "$("$ROOT/scripts/dev-signing.sh" --keychain)" \
    --sign "adoken dev" --identifier dev.adoken.app "$APP"
else
  echo "signing ad hoc: macOS will ask for Keychain access after every rebuild" >&2
  codesign --force --sign - --identifier dev.adoken.app "$APP"
fi
codesign --verify --strict "$APP"

# Refresh LaunchServices so the new icon and bundle id register at once.
LSREGISTER=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
[[ -x "$LSREGISTER" ]] && "$LSREGISTER" -f "$APP" >/dev/null 2>&1 || true

echo "bundled: $APP ($PROFILE, $VERSION build $BUILD_NUMBER)"

if [[ "$RUN" == true ]]; then
  osascript -e 'tell application id "dev.adoken.app" to quit' >/dev/null 2>&1 || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    pgrep -f "$APP/Contents/MacOS/adoken" >/dev/null || break
    sleep 0.3
  done
  open "$APP"
fi
