#!/usr/bin/env bash
set -euo pipefail

# Script to build and install CipherClipboard as a native macOS .app bundle into /Applications
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${PROJECT_DIR}"

APP_NAME="CipherClipboard"
INSTALL_DIR="${1:-/Applications}"
APP_BUNDLE="${INSTALL_DIR}/${APP_NAME}.app"

echo "📦 Building ${APP_NAME} in release mode..."
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
cat << 'EOF' > "${APP_BUNDLE}/Contents/Info.plist"
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
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
EOF

echo "🔄 Refreshing LaunchServices..."
touch "${APP_BUNDLE}"

echo "✅ CipherClipboard is now installed in ${INSTALL_DIR}!"
echo "You can launch it from Spotlight (Cmd+Space) or Finder."
