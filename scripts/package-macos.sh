#!/bin/bash
# macOS 打包脚本：编译 Go 产品并组装 QuickCleaner.app Bundle
#
# 用法：
#   ./scripts/package-macos.sh [output-dir] [binary-path | arm64 | amd64 | universal]
#
# 第二个参数是已编译的二进制时直接使用；是架构名时现场编译（默认 universal）。
# Bundle 结构、可执行文件名与 bundle id 与 Rust 版一致：旧版更新器按
# QuickCleaner.app/Contents/MacOS/quick-cleaner 认领更新包，不能改。
#
# 签名：设置 MACOS_SIGN_IDENTITY（Developer ID Application 证书名）时用
# hardened runtime 正式签名，否则 ad-hoc 签名（风扇助手要求至少有签名）。

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

OUTPUT_DIR="${1:-$ROOT_DIR/dist}"
INPUT="${2:-universal}"

APP_NAME="QuickCleaner.app"
APP_BUNDLE="$OUTPUT_DIR/$APP_NAME"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

# 版本号唯一来源：internal/app/root.go 的 const Version
VERSION=$(sed -nE 's/^const Version = "([0-9.]+)".*/\1/p' "$ROOT_DIR/internal/app/root.go" | head -1)
if [ -z "$VERSION" ]; then
    echo "无法从 internal/app/root.go 读取版本号" >&2
    exit 1
fi

build_arch() {
    # 全项目没有 cgo，CGO_ENABLED=0 可在任意主机上交叉编译
    (cd "$ROOT_DIR" && CGO_ENABLED=0 GOOS=darwin GOARCH="$1" \
        go build -trimpath -ldflags "-s -w" -o "$2" ./cmd/quick-cleaner)
}

if [ -f "$INPUT" ]; then
    SRC_BINARY="$INPUT"
    echo "==> 1/5 使用指定二进制: $SRC_BINARY"
else
    BUILD_DIR="$(mktemp -d)"
    trap 'rm -rf "$BUILD_DIR"' EXIT
    case "$INPUT" in
        arm64|amd64)
            echo "==> 1/5 编译 Go 二进制 ($INPUT)..."
            build_arch "$INPUT" "$BUILD_DIR/quick-cleaner"
            ;;
        universal)
            echo "==> 1/5 编译 Go 二进制 (arm64 + amd64 → universal)..."
            build_arch arm64 "$BUILD_DIR/quick-cleaner-arm64"
            build_arch amd64 "$BUILD_DIR/quick-cleaner-amd64"
            lipo -create -output "$BUILD_DIR/quick-cleaner" \
                "$BUILD_DIR/quick-cleaner-arm64" "$BUILD_DIR/quick-cleaner-amd64"
            ;;
        *)
            echo "未知的二进制或架构: $INPUT" >&2
            exit 2
            ;;
    esac
    SRC_BINARY="$BUILD_DIR/quick-cleaner"
fi

echo "==> 2/5 组装 App Bundle 目录结构 (版本: $VERSION)..."
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

cp "$SRC_BINARY" "$MACOS_DIR/quick-cleaner"
chmod +x "$MACOS_DIR/quick-cleaner"

# Finder / Dock 程序图标与 Rust 版同一份 assets/icon.icns
if [ ! -f "$ROOT_DIR/assets/icon.icns" ]; then
    echo "正在从 icon-512.png 生成 icon.icns..."
    ICONSET_DIR="$(mktemp -d)/icon.iconset"
    mkdir -p "$ICONSET_DIR"
    for spec in 16:16x16 32:16x16@2x 32:32x32 64:32x32@2x 128:128x128 256:128x128@2x \
                256:256x256 512:256x256@2x 512:512x512 1024:512x512@2x; do
        size="${spec%%:*}"
        name="${spec#*:}"
        sips -z "$size" "$size" "$ROOT_DIR/assets/icon-512.png" --out "$ICONSET_DIR/icon_$name.png" >/dev/null
    done
    iconutil -c icns "$ICONSET_DIR" -o "$ROOT_DIR/assets/icon.icns"
    rm -rf "$ICONSET_DIR"
fi
cp "$ROOT_DIR/assets/icon.icns" "$RESOURCES_DIR/icon.icns"

echo "==> 3/5 生成 Info.plist 与 PkgInfo..."
cat << EOF > "$CONTENTS_DIR/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>QuickCleaner</string>
    <key>CFBundleDisplayName</key>
    <string>QuickCleaner</string>
    <key>CFBundleIdentifier</key>
    <string>com.quickcleaner.app</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleExecutable</key>
    <string>quick-cleaner</string>
    <key>CFBundleIconFile</key>
    <string>icon.icns</string>
    <key>CFBundleIconName</key>
    <string>icon.icns</string>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleLocalizations</key>
    <array>
        <string>en</string>
        <string>zh-Hans</string>
    </array>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSHumanReadableCopyright</key>
    <string>Copyright © 2026 QuickCleaner</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
    <key>NSRequiresAquaSystemAppearance</key>
    <false/>
</dict>
</plist>
EOF
printf 'APPL????' > "$CONTENTS_DIR/PkgInfo"

if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
    echo "==> 4/5 Developer ID 签名 (hardened runtime)..."
    codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$MACOS_DIR/quick-cleaner"
    codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$APP_BUNDLE"
else
    echo "==> 4/5 Ad-hoc 代码签名..."
    codesign --force --deep --sign - "$APP_BUNDLE"
fi
codesign --verify --deep --strict --verbose=1 "$APP_BUNDLE"

echo "==> 5/5 打包完成: $APP_BUNDLE"
ls -la "$APP_BUNDLE"
