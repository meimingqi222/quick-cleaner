#!/usr/bin/env python3
"""UI 边界守卫：components 不得依赖 Root / core / platform，views 不得自建
遮罩、键盘处理与滚动条拖拽（这些由 components / Root 统一提供）。

存量记在基线文件里（scripts/ui-boundaries-baseline.json，{规则: {文件: 次数}}）
——脚本只拦「新增」，不因存量失败；迁移一处就 `--update` 收窄基线。
注释行（以 // 或 /// 开头）不计入。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UI_DIR = ROOT / "src" / "ui"
BASELINE = ROOT / "scripts" / "ui-boundaries-baseline.json"

# (规则名, 作用目录, 模式, 说明)
RULES = [
    (
        "components-no-root",
        "components",
        re.compile(r"\bRoot\b|crate::core|crate::platform|crate::ui::Root"),
        "components 不得依赖 Root / core / platform",
    ),
    (
        "views-no-overlay-backdrop",
        "views",
        re.compile(r"\.inset_0\(\)"),
        "views 不得自建遮罩（.inset_0() 且附近有 on_mouse_down(），走统一 overlay 组件",
    ),
    (
        "views-no-key-handlers",
        "views",
        re.compile(r"on_key_down\(|track_focus\("),
        "views 不得自行处理键盘 / 焦点（on_key_down / track_focus）",
    ),
    (
        "views-no-scroll-drag",
        "views",
        re.compile(r"drag_capture\(|scroll_metrics\(|drag_to_offset\("),
        "views 不得自行拼滚动条拖拽，走统一滚动组件",
    ),
]


# 只有「.inset_0() 与 on_mouse_down( 同在 NEAR_LINES 行内」才算自建遮罩：
# 遮罩要拦点击。dashboard / status 里的 .inset_0() 是扫描环、环形图文字的
# 装饰叠层（不处理鼠标），单看 .inset_0() 会误报。
BACKDROP_RULE = "views-no-overlay-backdrop"
MOUSE_DOWN = re.compile(r"on_mouse_down\(")
NEAR_LINES = 15


def scan() -> tuple[dict[str, dict[str, int]], dict[str, dict[str, list[int]]]]:
    counts: dict[str, dict[str, int]] = {name: {} for name, *_ in RULES}
    lines_hit: dict[str, dict[str, list[int]]] = {name: {} for name, *_ in RULES}
    for path in sorted(UI_DIR.rglob("*.rs")):
        rel = path.relative_to(UI_DIR)
        top = rel.parts[0] if len(rel.parts) > 1 else ""
        key = path.relative_to(ROOT).as_posix()
        applicable = [r for r in RULES if r[1] == top]
        if not applicable:
            continue
        text_lines = path.read_text(encoding="utf-8").splitlines()
        mouse_down_at = [
            no
            for no, line in enumerate(text_lines, 1)
            if not line.lstrip().startswith("//") and MOUSE_DOWN.search(line)
        ]
        for no, line in enumerate(text_lines, 1):
            if line.lstrip().startswith("//"):
                continue
            for name, _, pattern, _ in applicable:
                n = len(pattern.findall(line))
                if name == BACKDROP_RULE and n:
                    if not any(abs(m - no) <= NEAR_LINES for m in mouse_down_at):
                        n = 0
                if n:
                    counts[name][key] = counts[name].get(key, 0) + n
                    lines_hit[name].setdefault(key, []).append(no)
    return counts, lines_hit


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--update",
        action="store_true",
        help="把当前存量写入基线（只允许收窄；迁移完成后使用）",
    )
    args = parser.parse_args()

    current, lines_hit = scan()
    if args.update:
        BASELINE.write_text(
            json.dumps(current, ensure_ascii=False, indent=1, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        summary = ", ".join(f"{n}={sum(v.values())}" for n, v in current.items())
        print(f"baseline updated: {summary}")
        return 0

    baseline = (
        json.loads(BASELINE.read_text(encoding="utf-8")) if BASELINE.exists() else {}
    )
    failed = False
    shrunk = False
    for name, _, _, desc in RULES:
        allowed_map = baseline.get(name, {})
        grew = {
            p: (c, allowed_map.get(p, 0))
            for p, c in current[name].items()
            if c > allowed_map.get(p, 0)
        }
        if grew:
            failed = True
            print(f"[{name}] 新增违规：{desc}")
            for p, (c, allowed) in sorted(grew.items()):
                at = ",".join(str(n) for n in lines_hit[name][p])
                print(f"  {p}: {c} 处（基线允许 {allowed}），行号 {at}")
        if any(current[name].get(p, 0) < a for p, a in allowed_map.items()):
            shrunk = True

    if failed:
        return 1
    total = sum(sum(v.values()) for v in current.values())
    print(f"OK: no new UI boundary violations (remaining baseline: {total})")
    if shrunk:
        print("提示：存量已减少，可 --update 收窄基线")
    return 0


if __name__ == "__main__":
    sys.exit(main())
