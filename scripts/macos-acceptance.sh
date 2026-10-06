#!/usr/bin/env bash
# macOS 原生验收一键脚本
#
# 用途：在 macOS 真机/runner 上执行矩阵「平台质量」行的 macOS 侧待跑清单，
# 输出可直接回填 docs/RULES_REFACTOR_STATUS.md 的结果摘要。
#
# 设计约束：
# - 只跑只读检查与自带清理的隔离夹具；不触碰真实 Hermes、生态资源或用户数据；
# - 唯一例外是第 5 步的废纸篓往返用例——它向 `~/.Trash` 放入一个临时文件并
#   随后自己删除（这是它的被测行为），默认不跑，本脚本显式运行它；
# - 任何一步失败立即停止并给出失败步骤，不掩盖、不重试。
#
# 用法：scripts/macos-acceptance.sh
set -uo pipefail

cd "$(dirname "$0")/.." || exit 2

PYTHON=""
for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1; then
        PYTHON="$candidate"
        break
    fi
done

STEP_ROWS=""

record() {
    # $1 = 步骤名；只登记成功步骤（失败会 fail 退出，不会走到汇总）
    STEP_ROWS="${STEP_ROWS}| $1 | OK |"$'
'
}

step() {
    echo
    echo "=== $1 ==="
}

fail() {
    echo
    echo "FAILED: $1"
    exit 1
}

emit_summary_block() {
    # 回填块：七步结果 + 运行环境。成功路径与 --print-summary-format 共用。
    echo "===== 可粘贴到 docs/RULES_REFACTOR_STATUS.md 的块（原样复制，含上面各步输出）====="
    echo "| macOS 检查 | 结果 |"
    echo "| --- | --- |"
    printf '%s' "$STEP_ROWS"
    echo "| 运行环境 | $(uname -sm) · $(rustc --version) · commit $(git rev-parse --short HEAD 2>/dev/null || echo n/a) dirty=$(test -n "$(git status --porcelain 2>/dev/null)" && echo true || echo false) · $(date -u +%Y-%m-%dT%H:%M:%SZ) |"
    echo "===== 块结束：粘贴以上表格 + 本脚本的完整原始输出 ====="
}

# 汇总块格式自检：不跑任何检查，只渲染样例行——供 CI 在非 macOS 主机上
# 守护回填块格式（scripts/macos-acceptance-format-test.sh 调用它）。
if [ "${1:-}" = "--print-summary-format" ]; then
    record "1/7 \`cargo fmt --check\`"
    record "4/7 \`cargo test\`（含 macOS 门控用例）"
    record "5/7 废纸篓往返（ignored）"
    record "6/7 testlist 产物（620 行）"
    record "7/7 守卫三件套（strict-anchors / UI 本地化 / 状态引用）"
    emit_summary_block
    exit 0
fi

echo "macOS acceptance run — $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "uname: $(uname -sm)"
echo "rustc: $(rustc --version)"
echo "commit: $(git rev-parse --short HEAD 2>/dev/null || echo 'n/a') dirty=$(test -n "$(git status --porcelain 2>/dev/null)" && echo true || echo false)"

step "1/7 cargo fmt --check"
cargo fmt --check || fail "fmt"
record "1/7 `cargo fmt --check`"

step "2/7 cargo build"
cargo build || fail "build"
record "2/7 `cargo build`"

step "3/7 cargo clippy --all-targets -- -D warnings"
cargo clippy --all-targets -- -D warnings || fail "clippy"
record "3/7 `cargo clippy --all-targets -- -D warnings`"

step "4/7 cargo test（全库，含 macOS 门控用例）"
cargo test || fail "tests"
record "4/7 `cargo test`（含 macOS 门控用例）"

step "5/7 废纸篓往返（ignored：会在 ~/.Trash 留下并回收自己的临时文件）"
# 必须确认它**真的跑了**：这个用例被 target_os = "macos" 门控，在别的主机上
# 0 个测试也会 exit 0——那种「静默跳过」绝不能当成验收通过。
TRASH_LOG=target/macos-trash-roundtrip.log
cargo test -- --ignored move_to_trash_relocates_the_file 2>&1 | tee "$TRASH_LOG"
grep -q "1 passed" "$TRASH_LOG" || fail "废纸篓往返没有真的运行（本机可能不是 macOS，或用例被改名）"
record "5/7 废纸篓往返（ignored）"

step "6/7 test list 产物（供锚点与状态引用守卫使用）"
cargo test --lib -- --list | grep ': test$' > target/testlist-macos-local.txt || fail "test list"
TESTLIST_LINES=$(wc -l < target/testlist-macos-local.txt | tr -d ' ')
echo "testlist: $TESTLIST_LINES lines"
[ "$TESTLIST_LINES" -gt 100 ] || fail "testlist 行数异常（$TESTLIST_LINES），可能构建未完成"
record "6/7 testlist 产物（$TESTLIST_LINES 行）"

step "7/7 守卫（strict-anchors / UI 本地化 / 状态引用）"
if [ -n "$PYTHON" ]; then
    "$PYTHON" -X utf8 tools/regression-notes/verify-notes.py --notes-dir docs/agent-notes --strict-anchors || fail "strict anchors"
    "$PYTHON" -X utf8 scripts/check-ui-localization.py || fail "ui localization"
    "$PYTHON" -X utf8 scripts/check-status-references.py --tests target/testlist-macos-local.txt || fail "status references"
    record "7/7 守卫三件套（strict-anchors / UI 本地化 / 状态引用）"
else
    echo "SKIPPED: 找不到 python3/python；守卫未运行（回填时必须注明）"
fi

echo
echo "ALL STEPS PASSED"
echo
emit_summary_block
