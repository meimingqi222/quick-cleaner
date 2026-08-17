#!/bin/bash
# macOS 代码签名 + 公证脚本
#
# 用法：
#   ./scripts/sign-and-notarize.sh <path-to.app> <developer-id> <apple-id> <team-id> <notarytool-password>
#
# 前提：
#   - 已安装 Xcode Command Line Tools
#   - 已在 Keychain Access 中导入 Developer ID Application 证书
#   - 已在 https://developer.apple.com 中创建 App-Specific Password for notarytool
#
# 流程：
#   1. 用 Developer ID Application 证书签名
#   2. 提交公证
#   3. 等待公证完成
#   4. Staple 公证票据到 .app

set -euo pipefail

APP_PATH="${1:?用法: $0 <path-to.app> <developer-id> <apple-id> <team-id> <notarytool-password>}"
DEVELOPER_ID="${2:?需要 Developer ID 证书名}"
APPLE_ID="${3:?需要 Apple ID}"
TEAM_ID="${4:?需要 Team ID}"
NOTARY_PASSWORD="${5:?需要 App-Specific Password}"

echo "==> 1/4 代码签名: $APP_PATH"
codesign --force --deep --options runtime --sign "$DEVELOPER_ID" "$APP_PATH"

echo "==> 2/4 提交公证"
xcrun notarytool submit "$APP_PATH" \
    --apple-id "$APPLE_ID" \
    --team-id "$TEAM_ID" \
    --password "$NOTARY_PASSWORD" \
    --wait

echo "==> 3/4 Staple 公证票据"
xcrun stapler staple "$APP_PATH"

echo "==> 4/4 验证"
codesign --verify --strict --verbose=2 "$APP_PATH"
xcrun stapler validate "$APP_PATH"

echo "==> 完成: $APP_PATH 已签名并公证"
