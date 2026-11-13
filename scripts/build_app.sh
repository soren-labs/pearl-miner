#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-0.1.1}"
SIGN_IDENTITY="${PEARL_CODESIGN_IDENTITY:--}"
NOTARY_PROFILE="${PEARL_NOTARY_PROFILE:-}"
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Version must have the form 1.2.3" >&2
  exit 1
fi
if [[ -n "$NOTARY_PROFILE" && "$SIGN_IDENTITY" == "-" ]]; then
  echo "Notarization requires a Developer ID Application signing identity." >&2
  exit 1
fi
DIST_DIR="$ROOT_DIR/dist"
APP_NAME="PearlMiner.app"
APP_DIR="$DIST_DIR/$APP_NAME"
BUNDLE_ID="io.pearlminer.mac"
if [[ "$(uname -m)" != "arm64" ]]; then
  echo "Build the arm64 release on an Apple Silicon Mac." >&2
  exit 1
fi

echo "==> Building Rust core (pearl-miner-core)..."
cargo build --release --locked --manifest-path "$ROOT_DIR/Cargo.toml" -p pearl-miner-core

SWIFT_FLAGS=(-c release)
if [[ ! -d "/Applications/Xcode.app" && -d "/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk" ]]; then
  SWIFT_FLAGS+=(--sdk /Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk --triple arm64-apple-macos14.0)
fi

echo "==> Building SwiftUI macOS app (PearlMiner)..."
swift build --package-path "$ROOT_DIR/macos" "${SWIFT_FLAGS[@]}"

CORE_BIN="$ROOT_DIR/target/release/pearl-miner-core"
SWIFT_BIN="$(swift build --package-path "$ROOT_DIR/macos" "${SWIFT_FLAGS[@]}" --show-bin-path)/PearlMiner"

if [[ ! -x "$CORE_BIN" ]]; then
  echo "Missing Rust binary: $CORE_BIN" >&2
  exit 1
fi
if [[ ! -x "$SWIFT_BIN" ]]; then
  echo "Missing Swift binary: $SWIFT_BIN" >&2
  exit 1
fi

echo "==> Generating AppIcon.icns..."
WORK_TMP="$(mktemp -d)"
trap 'rm -rf "$WORK_TMP"' EXIT
ICONSET_DIR="$WORK_TMP/PearlMiner.iconset"
swift "$ROOT_DIR/scripts/generate_icon.swift" "$ICONSET_DIR"
iconutil -c icns "$ICONSET_DIR" -o "$WORK_TMP/AppIcon.icns"

echo "==> Assembling $APP_DIR..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS" "$APP_DIR/Contents/Resources"

cp "$SWIFT_BIN" "$APP_DIR/Contents/MacOS/PearlMiner"
cp "$CORE_BIN" "$APP_DIR/Contents/MacOS/pearl-miner-core"
cp "$WORK_TMP/AppIcon.icns" "$APP_DIR/Contents/Resources/AppIcon.icns"
chmod +x "$APP_DIR/Contents/MacOS/PearlMiner" "$APP_DIR/Contents/MacOS/pearl-miner-core"

cat > "$APP_DIR/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleDisplayName</key>
    <string>PearlMiner</string>
    <key>CFBundleExecutable</key>
    <string>PearlMiner</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>PearlMiner</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>LSArchitecturePriority</key>
    <array>
        <string>arm64</string>
    </array>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <false/>
</dict>
</plist>
EOF

echo "==> Signing $APP_DIR..."
SIGN_FLAGS=(--force --sign "$SIGN_IDENTITY")
if [[ "$SIGN_IDENTITY" != "-" ]]; then
  SIGN_FLAGS+=(--options runtime --timestamp)
else
  echo "Development build: ad-hoc signed, not notarized."
fi
# Sign nested executables first, then the bundle; --deep is for verification.
codesign "${SIGN_FLAGS[@]}" "$APP_DIR/Contents/MacOS/pearl-miner-core"
codesign "${SIGN_FLAGS[@]}" "$APP_DIR"
codesign --verify --deep --strict "$APP_DIR"

ZIP_PATH="$DIST_DIR/PearlMiner-macOS-arm64.zip"
DMG_PATH="$DIST_DIR/PearlMiner-macOS-arm64.dmg"
rm -f "$ZIP_PATH" "$DMG_PATH"

echo "==> Packaging $ZIP_PATH..."
ditto -c -k --sequesterRsrc --keepParent "$APP_DIR" "$ZIP_PATH"
if [[ -n "$NOTARY_PROFILE" ]]; then
  xcrun notarytool submit "$ZIP_PATH" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$APP_DIR"
  xcrun stapler validate "$APP_DIR"
  rm -f "$ZIP_PATH"
  ditto -c -k --sequesterRsrc --keepParent "$APP_DIR" "$ZIP_PATH"
fi

echo "==> Packaging $DMG_PATH..."
DMG_STAGE="$WORK_TMP/dmg"
mkdir -p "$DMG_STAGE"
cp -R "$APP_DIR" "$DMG_STAGE/"
ln -s /Applications "$DMG_STAGE/Applications"
hdiutil create -volname "PearlMiner" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DMG_PATH" >/dev/null
if [[ "$SIGN_IDENTITY" != "-" ]]; then
  codesign --force --sign "$SIGN_IDENTITY" --timestamp "$DMG_PATH"
fi
if [[ -n "$NOTARY_PROFILE" ]]; then
  xcrun notarytool submit "$DMG_PATH" --keychain-profile "$NOTARY_PROFILE" --wait
  xcrun stapler staple "$DMG_PATH"
  xcrun stapler validate "$DMG_PATH"
  spctl --assess --type execute --verbose "$APP_DIR"
fi

(
  cd "$DIST_DIR"
  shasum -a 256 "PearlMiner-macOS-arm64.dmg" "PearlMiner-macOS-arm64.zip" > SHA256SUMS.txt
)

echo "==> Done!"
ls -lh "$DIST_DIR"
