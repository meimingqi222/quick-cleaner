#!/usr/bin/env bash
# 验收脚本「可粘贴回填块」的格式测试。
#
# 回填块是交接链路的承重件：拿到 macOS 的人把它贴进状态记录即完成验收回填。
# 若块格式悄悄坏掉（列数、分隔行、环境行缺失），回填出来的表格就不成立——
# 本测试在任意主机（含 Windows/Linux CI）上跑 `--print-summary-format` 并断言
# 关键行存在，不需要 macOS、不执行任何检查。
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

output=$(bash scripts/macos-acceptance.sh --print-summary-format) || {
    echo "FAILED: --print-summary-format 退出非 0"
    exit 1
}

check() {
    if ! grep -qF "$1" <<<"$output"; then
        echo "FAILED: 回填块缺少「$1」"
        echo "----- 实际输出 -----"
        echo "$output"
        exit 1
    fi
}

check "| macOS 检查 | 结果 |"
check "| --- | --- |"
check "| 1/7 \`cargo fmt --check\` | OK |"
check "| 5/7 废纸篓往返（ignored） | OK |"
check "| 7/7 守卫三件套（strict-anchors / UI 本地化 / 状态引用） | OK |"
check "| 运行环境 |"
check "块结束"

# 行数断言：表头 2 行 + 样例行 5 行 + 环境 1 行 = 8 行表格
rows=$(grep -c '^|' <<<"$output")
if [ "$rows" -ne 8 ]; then
    echo "FAILED: 表格行数应为 8，实际 $rows"
    echo "$output"
    exit 1
fi

echo "OK: 回填块格式正确（8 行表格 + 环境行；未执行任何检查）"
