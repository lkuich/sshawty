#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --release --locked
bundle=dist/sshawty.app
version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
cp assets/icon.icns "$bundle/Contents/Resources/icon.icns"
# Replace the inode so rebuilding cannot invalidate executable pages in a running app.
cp target/release/sshawty "$bundle/Contents/MacOS/sshawty.new"
mv -f "$bundle/Contents/MacOS/sshawty.new" "$bundle/Contents/MacOS/sshawty"
cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>sshawty</string>
<key>CFBundleIdentifier</key><string>app.sshawty.desktop</string>
<key>CFBundleName</key><string>sshawty</string>
<key>CFBundleIconFile</key><string>icon.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
plutil -lint "$bundle/Contents/Info.plist"
codesign --force --deep --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
printf '%s\n' "$bundle"
