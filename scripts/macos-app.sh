#!/bin/sh
# Builds rplan.app (Apple Silicon + Intel in one) and rplan-<version>-macos.dmg.
# Run on a Mac with Rust installed:  sh scripts/macos-app.sh
set -eu
cd "$(dirname "$0")/.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
OUT=target/macos
APP=$OUT/rplan.app
export MACOSX_DEPLOYMENT_TARGET=11.0

rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

rm -rf "$OUT" && mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/rplan" \
  target/aarch64-apple-darwin/release/rplan target/x86_64-apple-darwin/release/rplan
sed "s/@VERSION@/$VERSION/g" assets/macos/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# The icon, at every size macOS asks for.
SET=$OUT/rplan.iconset && mkdir -p "$SET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/macos/rplan-1024.png --out "$SET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d assets/macos/rplan-1024.png --out "$SET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$APP/Contents/Resources/rplan.icns"
rm -rf "$SET"

# Ad-hoc signature: enough to run on Apple Silicon. (Not notarized — see the README.)
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict "$APP"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

# A disk image with the app and a shortcut to Applications.
STAGE=$OUT/dmg && mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/" && ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "rplan $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$OUT/rplan-$VERSION-macos.dmg" >/dev/null
rm -rf "$STAGE"
lipo -info "$APP/Contents/MacOS/rplan"
echo "Built $APP and $OUT/rplan-$VERSION-macos.dmg"
