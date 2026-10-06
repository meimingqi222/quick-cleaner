#!/usr/bin/env python3
"""文档引用守卫：状态记录与维护文档（RULES / CAPABILITIES）里反引号
引用的**标识符**与**仓库路径**必须真实存在。

标识符：要么是现行测试名，要么在源码/规则/工具/依赖清单里找得到。
路径：以仓库顶层目录开头的相对路径必须命中真实文件/目录；`{a,b}` 花括号
展开后每个变体都要存在，`*` 通配至少命中一个，带 `<...>` 占位符的模板跳过。

背景：矩阵映射表引用测试名与夹具路径作为证据，但没有任何检查拦得住
「引用了改名/被删的测试」。上一轮穷举终审手工发现过一例失实引用
（`deep_preserved_paths_recurse_and_keep_siblings` 当时并不存在）。本脚本把
那次穷举固化成可重复检查：

- 测试名：与 `cargo test --lib -- --list` 的产物比对（CI 里取 Windows +
  macOS 两个 job 的 testlist 并集，见 ci.yml 的 verify-anchors job）；
- 非测试标识符：在 src/ / rules/ / scripts/ / tools/ / Cargo.toml / vendor/
  里 grep 得到即算存在（函数名、配置键、依赖名等）；
- 历史恢复点里对**已删除实现**的记述会自然解析失败，这些用基线文件
  `scripts/status-reference-baseline.json` 登记（带原因），只拦「新增」的
  失实引用；基线只应因「记录被删除的历史名」而增，不应为掩盖拼写错误而增。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
STATUS = ROOT / "docs" / "RULES_REFACTOR_STATUS.md"
BASELINE = ROOT / "scripts" / "status-reference-baseline.json"
SEARCH_ROOTS = ["src", "rules", "scripts", "tools", "vendor", "Cargo.toml", "build.rs"]

IDENTIFIER = re.compile(r"`([a-z][a-z0-9_]{7,})`")
REPO_ROOTS = ("src", "rules", "docs", "scripts", "tools", "vendor", "examples", "assets")
PATH_SPAN = re.compile(r"`((?:%s)/[A-Za-z0-9._/{},<>*-]+)`" % "|".join(REPO_ROOTS))
BRACES = re.compile(r"\{([^{}]+)\}")


def expand_braces(path: str) -> list[str]:
    match = BRACES.search(path)
    if not match:
        return [path]
    return [
        variant
        for option in match.group(1).split(",")
        for variant in expand_braces(path[: match.start()] + option + path[match.end() :])
    ]


# 这些标记出现在同一行时，路径引用是「隔离根内的相对路径」或「已明确
# 记述为已删除的文件」，不存在是文档在如实陈述，不是失实引用。
CONTEXT_SKIP = (
    "隔离根",
    "夹具",
    "fixture",
    "<root>",
    "已删",
    "不存在",
    "删除",
    "移除",
)


def unresolved_paths(text: str, baseline: dict[str, str] | None = None) -> list[str]:
    problems: list[str] = []
    baseline = baseline or {}
    for line in text.splitlines():
        # 语境只看路径引用**之外**的文字：`rules/fixtures/...` 自身包含
        # "fixture"，拿整行做子串判断会把所有夹具引用误跳过。
        context = PATH_SPAN.sub("", line)
        if any(marker in context for marker in CONTEXT_SKIP):
            continue
        for raw in PATH_SPAN.findall(line):
            if any(token in raw for token in ("<", ">", "..")):
                continue  # 模板/占位符，不是具体引用
            if raw in baseline:
                continue  # 已登记的历史/已删除引用
            for path in expand_braces(raw):
                if "*" in path:
                    # 通配：至少命中一个
                    base = Path(path)
                    parent = ROOT / base.parent
                    if not parent.exists() or not list(parent.glob(base.name)):
                        problems.append(raw)
                    continue
                target = ROOT / path.rstrip("/")
                if not target.exists():
                    problems.append(raw)
                    break
    return sorted(set(problems))


def collect_identifiers(text: str) -> set[str]:
    return set(IDENTIFIER.findall(text))


def collect_tests(paths: list[Path]) -> set[str]:
    names: set[str] = set()
    for path in paths:
        for line in path.read_text(encoding="utf-8").splitlines():
            # `cargo test --list` 行格式：`module::path::test_name: test`
            # （注意路径里是 `::`，不能按第一个 `:` 切分）
            for suffix in (": test", ": benchmark"):
                if line.endswith(suffix):
                    line = line[: -len(suffix)]
                    break
            else:
                continue
            names.add(line.strip().rsplit("::", 1)[-1])
    return names


def exists_in_repo(identifier: str) -> bool:
    needle = identifier.encode()
    for rel in SEARCH_ROOTS:
        target = ROOT / rel
        if not target.exists():
            continue
        files = [target] if target.is_file() else target.rglob("*")
        for path in files:
            if not path.is_file():
                continue
            try:
                if needle in path.read_bytes():
                    return True
            except OSError:
                continue
    return False


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--tests",
        nargs="+",
        type=Path,
        required=True,
        help="cargo test --list 产物（可多个，取并集）",
    )
    parser.add_argument(
        "--docs",
        nargs="+",
        type=Path,
        default=[STATUS, ROOT / "docs" / "RULES.md", ROOT / "docs" / "CAPABILITIES.md"],
        help="要核对的文档（默认：状态记录 + RULES + CAPABILITIES）",
    )
    parser.add_argument("--status", type=Path, default=STATUS, help=argparse.SUPPRESS)
    parser.add_argument("--baseline", type=Path, default=BASELINE)
    parser.add_argument("--update-baseline", action="store_true")
    args = parser.parse_args()

    texts = {path: path.read_text(encoding="utf-8") for path in args.docs}
    identifiers = set()
    for text in texts.values():
        identifiers |= collect_identifiers(text)
    tests = collect_tests(args.tests)
    baseline: dict[str, str] = {}
    if args.baseline.exists():
        baseline = json.loads(args.baseline.read_text(encoding="utf-8"))

    unresolved: dict[str, list[str]] = {"test_missing": [], "unknown": []}
    for name in sorted(identifiers):
        if name in tests:
            continue
        if name in baseline:
            continue
        if exists_in_repo(name):
            continue
        # 命名像测试（长且带下划线的句子式）却解析不到 → 更可能是失实测试引用
        key = "test_missing" if name.count("_") >= 2 else "unknown"
        unresolved[key].append(name)

    if args.update_baseline:
        for key in unresolved:
            for name in unresolved[key]:
                baseline[name] = "历史恢复点引用（已删除实现）或既有语境，见对应恢复点"
        args.baseline.write_text(
            json.dumps(baseline, ensure_ascii=False, indent=1, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        print(f"baseline updated: {len(baseline)} entry(ies)")
        return 0

    bad_paths = {path: unresolved_paths(text, baseline) for path, text in texts.items()}

    problems = (
        unresolved["test_missing"]
        + unresolved["unknown"]
        + [f"{p}: {ref}" for p, refs in bad_paths.items() for ref in refs]
    )
    if problems:
        print("状态记录里出现无法解析的引用（测试名不存在 / 仓库里找不到）：")
        for name in unresolved["test_missing"]:
            print(f"  TEST? {name}")
        for name in unresolved["unknown"]:
            print(f"  ?     {name}")
        for path, refs in bad_paths.items():
            for ref in refs:
                print(f"  PATH  {ref}  ({path})")
        print("修引用或补真实测试；确属历史记述时用 --update-baseline 登记原因。")
        return 1
    print(
        f"OK: {len(identifiers)} identifier(s) resolved ({len(tests)} test names); "
        f"{len(texts)} doc(s) path-clean"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
