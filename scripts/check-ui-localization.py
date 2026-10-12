#!/usr/bin/env python3
"""界面文案本地化守卫：`Language::Zh =>` 出现在 i18n 之外的 UI 源文件里，
说明那里写死了中英串，而不是走 `ui/i18n` 的 `tr_*`（AGENTS.md 的约束）。

存量站点记在基线文件里（scripts/ui-localization-baseline.json）——脚本只拦
「新增」，不因存量失败；迁移一处就 `--update` 收窄基线。基线清空即守卫升级
为「完全禁止」。

另一条规则：i18n 之外的 UI 代码里不许出现含中文（含全角标点）的字符串字面量。
`if lang == Language::Zh {..}`、元组 `match`、只有中文的硬编码都会被它拦下。
注释、测试模块、日志 / 断言宏和 `.expect(..)` 的内部信息不算界面文案，先剔除。
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


CJK = re.compile(r"[\u3000-\u303f\u4e00-\u9fff\uff00-\uffef]")
# 只给开发者看的输出：日志、断言、panic、Option/Result 的 expect。
INTERNAL_CALL = re.compile(
    r"(?:\b(?:crate::)?(?:log|eprintln|println|assert|assert_eq|assert_ne|debug_assert|"
    r"debug_assert_eq|panic|unreachable|todo|unimplemented)!|\.expect)\s*\("
)


def _skip_string(text: str, i: int) -> int:
    """i 指向字符串起点（`"` 或 `r#"`），返回字符串结束后的位置。"""
    raw = re.match(r'b?r(#*)"', text[i:])
    if raw:
        end = text.find('"' + raw[1], i + raw.end())
        return len(text) if end < 0 else end + 1 + len(raw[1])
    i = text.index('"', i) + 1
    while i < len(text) and text[i] != '"':
        i += 2 if text[i] == "\\" else 1
    return i + 1


def _blank(chars: list[str], start: int, end: int) -> None:
    for k in range(start, min(end, len(chars))):
        if chars[k] != "\n":
            chars[k] = " "


def ui_copy_only(text: str) -> str:
    """去掉注释、`#[cfg(test)] mod` 块和内部诊断调用，保留其余代码与字符串。"""
    chars = list(text)
    i = 0
    while i < len(text):
        if text.startswith("//", i):
            end = text.find("\n", i)
            end = len(text) if end < 0 else end
            _blank(chars, i, end)
            i = end
        elif text.startswith("/*", i):
            end = text.find("*/", i + 2)
            end = len(text) if end < 0 else end + 2
            _blank(chars, i, end)
            i = end
        elif re.match(r'b?r#*"|"', text[i:]) and not re.match(r"\w", text[i - 1 : i] or " "):
            i = _skip_string(text, i)
        elif text[i] == "'" and re.match(r"'(?:\\.|[^'\\])'", text[i:]):
            i += len(re.match(r"'(?:\\.|[^'\\])'", text[i:])[0])
        else:
            i += 1
    text = "".join(chars)
    chars = list(text)
    for pattern in (INTERNAL_CALL, re.compile(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*")):
        for m in pattern.finditer(text):
            opener = "(" if pattern is INTERNAL_CALL else "{"
            j = text.find(opener, m.end() - 1)
            depth, k = 0, j
            while 0 <= k < len(text):
                c = text[k]
                if c == '"' or text.startswith('r#"', k):
                    k = _skip_string(text, k)
                    continue
                if c == opener:
                    depth += 1
                elif c == (")" if opener == "(" else "}"):
                    depth -= 1
                    if depth == 0:
                        break
                k += 1
            _blank(chars, m.start(), k + 1)
    return "".join(chars)


def inline_count(text: str) -> int:
    code = ui_copy_only(text)
    literals = re.findall(r'b?r(#*)"(.*?)"\1|"((?:[^"\\]|\\.)*)"', code, flags=re.S)
    cjk = sum(1 for _, raw, plain in literals if CJK.search(raw or plain))
    return len(PATTERN.findall(code)) + cjk


def scan() -> dict[str, int]:
    counts: dict[str, int] = {}
    for path in sorted(UI_DIR.rglob("*.rs")):
        if I18N_DIR in path.parents:
            continue
        found = inline_count(path.read_text(encoding="utf-8"))
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
