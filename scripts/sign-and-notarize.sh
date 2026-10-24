#!/bin/bash
# macOS 代码签名 + 公证脚本
#
# 用法：
#   ./scripts/sign-and-notarize.sh <path-to.app> <developer-id> <apple-id> <team-id> <notarytool-password>
#
# 前提：
#   - 已安装 Xcode Command Line Tools
#   - 钥匙串里已导入 Developer ID Application 证书
#   - 已在 https://appleid.apple.com 为 notarytool 创建 App 专用密码
#
# 流程：签名（hardened runtime + 时间戳）→ ditto 打 zip 提交公证并等待 →
# staple 票据到 .app → Gatekeeper 校验。notarytool 不接受 .app 目录，
# 只收 zip/dmg/pkg，所以提交的是临时 zip，staple 的是 .app 本身。

set -euo pipefail

APP_PATH="${1:?用法: $0 <path-to.app> <developer-id> <apple-id> <team-id> <notarytool-password>}"
DEVELOPER_ID="${2:?需要 Developer ID 证书名}"
APPLE_ID="${3:?需要 Apple ID}"
TEAM_ID="${4:?需要 Team ID}"
NOTARY_PASSWORD="${5:?需要 App-Specific Password}"

echo "==> 1/5 代码签名: $APP_PATH"
codesign --force --options runtime --timestamp --sign "$DEVELOPER_ID" "$APP_PATH/Contents/MacOS/quick-cleaner"
codesign --force --options runtime --timestamp --sign "$DEVELOPER_ID" "$APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

SUBMIT_DIR="$(mktemp -d)"
trap 'rm -rf "$SUBMIT_DIR"' EXIT
SUBMIT_ZIP="$SUBMIT_DIR/$(basename "$APP_PATH" .app).zip"

echo "==> 2/5 打包公证提交用 zip"
ditto -c -k --keepParent "$APP_PATH" "$SUBMIT_ZIP"

echo "==> 3/5 提交公证并等待结果"
xcrun notarytool submit "$SUBMIT_ZIP" \
    --apple-id "$APPLE_ID" \
    --team-id "$TEAM_ID" \
    --password "$NOTARY_PASSWORD" \
    --wait

echo "==> 4/5 Staple 公证票据"
xcrun stapler staple "$APP_PATH"

echo "==> 5/5 验证"
xcrun stapler validate "$APP_PATH"
spctl --assess --type execute --verbose=2 "$APP_PATH"

echo "==> 完成: $APP_PATH 已签名并公证"
