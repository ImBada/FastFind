#!/usr/bin/env bash
# Builds dist/FastFind.app (a bare cargo binary never becomes the key window).
#   scripts/bundle.sh            release build
#   scripts/bundle.sh --debug    debug build
#   scripts/bundle.sh --dmg      release build + dist/FastFindLatest.dmg
#   scripts/bundle.sh --zip      release build + dist/FastFind.zip (the asset auto-update installs)
set -euo pipefail
cd "$(dirname "$0")/.."

product=FastFind
bundle_id=BadaGames.FastFind
profile=release
make_dmg=0
make_zip=0
for arg in "$@"; do
  case "$arg" in
    --debug) profile=debug ;;
    --dmg) make_dmg=1 ;;
    --zip) make_zip=1 ;;
  esac
done

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
build_number=$(git rev-list --count HEAD 2>/dev/null || echo 0)

if [ "$profile" = release ]; then cargo build --release; else cargo build; fi
bin="${CARGO_TARGET_DIR:-target}/$profile/fastfind"

app="dist/$product.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/en.lproj" "$app/Contents/Resources/ko.lproj"
cp "$bin" "$app/Contents/MacOS/$product"
# Empty .lproj folders tell AppKit which localizations exist (system panels, alerts, menus).
touch "$app/Contents/Resources/en.lproj/InfoPlist.strings" "$app/Contents/Resources/ko.lproj/InfoPlist.strings"
iconutil -c icns assets/AppIcon.iconset -o "$app/Contents/Resources/AppIcon.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key>	<string>en</string>
	<key>CFBundleLocalizations</key>	<array><string>en</string><string>ko</string></array>
	<key>CFBundleExecutable</key>		<string>$product</string>
	<key>CFBundleIdentifier</key>		<string>$bundle_id</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleName</key>			<string>$product</string>
	<key>CFBundleDisplayName</key>		<string>$product</string>
	<key>CFBundleIconFile</key>		<string>AppIcon</string>
	<key>CFBundlePackageType</key>		<string>APPL</string>
	<key>CFBundleShortVersionString</key>	<string>$version</string>
	<key>CFBundleVersion</key>		<string>$build_number</string>
	<key>LSApplicationCategoryType</key>	<string>public.app-category.utilities</string>
	<key>LSMinimumSystemVersion</key>	<string>26.0</string>
	<key>NSHighResolutionCapable</key>	<true/>
	<key>NSPrincipalClass</key>		<string>NSApplication</string>
	<key>NSHumanReadableCopyright</key>	<string>© 2024 BADA IM</string>
	<key>NSAppleEventsUsageDescription</key>	<string>$product asks Finder to open the Get Info window for the selected files.</string>
	<key>NSRemovableVolumesUsageDescription</key>	<string>$product indexes the folder you choose, including ones on external disks.</string>
	<key>NSNetworkVolumesUsageDescription</key>	<string>$product indexes the folder you choose, including ones on network shares.</string>
	<key>NSDesktopFolderUsageDescription</key>	<string>$product indexes the folder you choose.</string>
	<key>NSDocumentsFolderUsageDescription</key>	<string>$product indexes the folder you choose.</string>
	<key>NSDownloadsFolderUsageDescription</key>	<string>$product indexes the folder you choose.</string>
</dict>
</plist>
PLIST

codesign --force --deep --sign "${FASTFIND_CODESIGN_IDENTITY:--}" "$app" >/dev/null
echo "Built $app ($version, $profile)"

if [ "$make_zip" = 1 ]; then
  rm -f "dist/$product.zip"
  ditto -c -k --keepParent "$app" "dist/$product.zip"
  echo "Built dist/$product.zip"
fi

if [ "$make_dmg" = 1 ]; then
  staging=dist/dmg
  rm -rf "$staging" && mkdir -p "$staging"
  cp -R "$app" "$staging/"
  ln -s /Applications "$staging/Applications"
  hdiutil create -volname "$product" -srcfolder "$staging" -ov -format UDZO "dist/${product}Latest.dmg" >/dev/null
  rm -rf "$staging"
  echo "Built dist/${product}Latest.dmg"
fi
