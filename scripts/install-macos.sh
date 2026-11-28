#!/usr/bin/env bash
set -euo pipefail

# Script to build and install CipherClipboard as a native macOS .app bundle into /Applications
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${PROJECT_DIR}"

APP_NAME="CipherClipboard"
APP_VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')"
APP_VERSION="${APP_VERSION:-0.1.0}"
INSTALL_DIR="${1:-/Applications}"
APP_BUNDLE="${INSTALL_DIR}/${APP_NAME}.app"
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-CipherClipboard Local Dev}"

echo "📦 Building ${APP_NAME} v${APP_VERSION} in release mode..."
cargo build --release

echo "📂 Creating bundle directory at ${APP_BUNDLE}..."
rm -rf "${APP_BUNDLE}"
mkdir -p "${APP_BUNDLE}/Contents/MacOS"
mkdir -p "${APP_BUNDLE}/Contents/Resources"

echo "📋 Copying executable..."
cp "target/release/${APP_NAME}" "${APP_BUNDLE}/Contents/MacOS/${APP_NAME}"
chmod +x "${APP_BUNDLE}/Contents/MacOS/${APP_NAME}"

if [ -f "assets/icon.icns" ]; then
    echo "🎨 Installing application icon..."
    cp "assets/icon.icns" "${APP_BUNDLE}/Contents/Resources/icon.icns"
fi

echo "⚙️ Writing Info.plist metadata..."
cat << EOF > "${APP_BUNDLE}/Contents/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>CipherClipboard</string>
    <key>CFBundleIdentifier</key>
    <string>com.cipherclipboard.app</string>
    <key>CFBundleName</key>
    <string>CipherClipboard</string>
    <key>CFBundleDisplayName</key>
    <string>CipherClipboard</string>
    <key>CFBundleIconFile</key>
    <string>icon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${APP_VERSION}</string>
    <key>CFBundleVersion</key>
    <string>${APP_VERSION}</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSKeychainUsageDescription</key>
    <string>CipherClipboard stores your database encryption key securely in macOS Keychain so your clipboard history remains encrypted and accessible only to you.</string>
</dict>
</plist>
EOF

if security find-identity -p codesigning 2>/dev/null | grep -Fq "\"${CODESIGN_IDENTITY}\""; then
    echo "🔏 Signing release binary and app bundle with \"${CODESIGN_IDENTITY}\"..."
    codesign --force --deep --sign "${CODESIGN_IDENTITY}" "target/release/${APP_NAME}"
    codesign --force --deep --sign "${CODESIGN_IDENTITY}" "${APP_BUNDLE}"
else
    echo "⚠️  Signing identity \"${CODESIGN_IDENTITY}\" not found in keychain; falling back to ad-hoc signature (Accessibility permission may need re-granting after rebuilds)."
    codesign --force --deep --sign - "${APP_BUNDLE}"
fi

echo "🔄 Refreshing LaunchServices..."
touch "${APP_BUNDLE}"

echo "✅ CipherClipboard is now installed in ${INSTALL_DIR}!"
echo "You can launch it from Spotlight (Cmd+Space) or Finder."
