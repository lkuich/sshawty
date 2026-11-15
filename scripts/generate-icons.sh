#!/bin/sh
# Requires ImageMagick and macOS iconutil; release builds use the checked-in assets.
set -eu
cd "$(dirname "$0")/.."
icon_tmp=$(mktemp -d)
trap 'rm -rf "$icon_tmp"' EXIT
mkdir "$icon_tmp/icon.iconset"
sips -s format png assets/icon.svg --out "$icon_tmp/master.png" >/dev/null
magick "$icon_tmp/master.png" -resize 256x256 PNG32:assets/icon.png
magick assets/icon.png -define icon:auto-resize=256,128,64,48,32,24,16 assets/icon.ico
for size in 16 32 128 256 512; do
    magick "$icon_tmp/master.png" -resize "${size}x${size}" "PNG32:$icon_tmp/icon.iconset/icon_${size}x${size}.png"
    double=$((size * 2))
    magick "$icon_tmp/master.png" -resize "${double}x${double}" "PNG32:$icon_tmp/icon.iconset/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$icon_tmp/icon.iconset" -o assets/icon.icns
