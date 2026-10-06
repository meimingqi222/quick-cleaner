#!/usr/bin/env python3
"""界面文案本地化守卫：`Language::Zh =>` 出现在 i18n 之外的 UI 源文件里，
说明那里写死了中英串，而不是走 `ui/i18n` 的 `tr_*`（AGENTS.md 的约束）。

存量站点记在基线文件里（scripts/ui-localization-baseline.json）——脚本只拦
「新增」，不因存量失败；迁移一处就 `--update` 收窄基线。基线清空即守卫升级
为「完全禁止」。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
UI_DIR = ROOT / "src" / "ui"
I18N_DIR = UI_DIR / "i18n"
BASELINE = ROOT / "scripts" / "ui-localization-baseline.json"

# 只看 Zh 分支的字面量：En 分支与它成对出现，Zh 一处即一条内联文案。
PATTERN = re.compile(r'Language::Zh => (?:format!\(|")')


def scan() -> dict[str, int]:
    counts: dict[str, int] = {}
    for path in sorted(UI_DIR.rglob("*.rs")):
        if I18N_DIR in path.parents:
            continue
        text = path.read_text(encoding="utf-8")
        found = len(PATTERN.findall(text))
        if found:
            counts[path.relative_to(ROOT).as_posix()] = found
    return counts


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--update",
        action="store_true",
        help="把当前存量写入基线（只允许收窄；迁移完成后使用）",
    )
    args = parser.parse_args()

    current = scan()
    if args.update:
        BASELINE.write_text(
            json.dumps(current, ensure_ascii=False, indent=1, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        total = sum(current.values())
        print(f"baseline updated: {len(current)} file(s), {total} inline string(s)")
        return 0

    baseline = (
        json.loads(BASELINE.read_text(encoding="utf-8")) if BASELINE.exists() else {}
    )
    grew = {
        path: (count, baseline.get(path, 0))
        for path, count in current.items()
        if count > baseline.get(path, 0)
    }
    total = sum(current.values())
    if grew:
        print("新增的内联中英串必须走 ui/i18n 的 tr_*：")
        for path, (count, allowed) in sorted(grew.items()):
            print(f"  {path}: {count} 处（基线允许 {allowed}）")
        return 1
    print(f"OK: no new inline UI strings (remaining baseline: {total})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
