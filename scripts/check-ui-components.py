#!/usr/bin/env python3
"""Enforce UI ownership boundaries. Baselines may shrink, never grow.

Usage: python scripts/check-ui-components.py [--update] [--previous-baseline FILE]
Rust comments and literals are masked before matching; line numbers are diagnostic only.
"""
from __future__ import annotations
import argparse
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / 'scripts/ui-components-baseline.json'
RULES = ('components-no-business', 'views-no-key-handlers', 'views-no-overlay', 'views-no-scroll-drag', 'ui-no-hex-colors')


def code_only(source: str) -> str:
    out = list(source)
    i = 0
    def mask(a: int, b: int) -> None:
        for k in range(a, b):
            if out[k] != '\n':
                out[k] = ' '
    while i < len(source):
        start = i
        if source.startswith('//', i):
            end = source.find('\n', i)
            i = len(source) if end < 0 else end
        elif source.startswith('/*', i):
            depth = 1
            i += 2
            while i < len(source) and depth:
                if source.startswith('/*', i): depth += 1; i += 2
                elif source.startswith('*/', i): depth -= 1; i += 2
                else: i += 1
        else:
            raw = re.match(r'(?:br|r)(#*)"', source[i:])
            if raw:
                endmark = '"' + raw[1]
                end = source.find(endmark, i + raw.end())
                i = len(source) if end < 0 else end + len(endmark)
            elif source[i] == '"':
                i += 1
                while i < len(source):
                    if source[i] == '\\': i += 2
                    elif source[i] == '"': i += 1; break
                    else: i += 1
            elif source[i] == "'" and (char := re.match(r"'(?:\\(?:u\{[^}]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'", source[i:])):
                i += char.end()
            else:
                i += 1
                continue
        mask(start, min(i, len(source)))
    return ''.join(out)


def violations(source: str, layer: str) -> dict[str, int]:
    code = code_only(source)
    result = {}
    if layer == 'components':
        aliases = re.findall(r'\buse\s+crate\s+as\s+(\w+)\s*;', code)
        for alias in aliases:
            code = re.sub(rf'\b{alias}\s*::', 'crate::', code)
        pattern = r'\bRoot\b|\bcrate\s*::\s*(?:core|platform)\b|\buse\s+crate\s*::\s*\{[^;]*\b(?:core|platform)\b'
        result['components-no-business'] = len(re.findall(pattern, code))
    elif layer == 'views':
        result['views-no-key-handlers'] = len(re.findall(r'\.\s*(?:on_key_down|track_focus|handle_input)\s*\(', code))
        result['views-no-overlay'] = len(re.findall(r'\.\s*(?:occlude|on_mouse_down_out)\s*\(', code))
        # Restrict inset_0 to an interactive absolute backdrop in the same chain.
        for chain in code.split(';'):
            if re.search(r'\.inset_0\s*\(', chain) and re.search(r'\.(?:on_any_mouse_down|on_mouse_down)\s*\(', chain):
                result['views-no-overlay'] += 1
        result['views-no-scroll-drag'] = len(re.findall(r'\b(?:drag_capture|drag_to_offset|scroll_metrics|scrollbar)\s*\(', code))
        result['views-no-scroll-drag'] += len(re.findall(r'\bgpui\s*::\s*uniform_list\s*\(', code))
    if layer in ('views', 'adapters', 'shell'):
        # Colors come from theme tokens; components keep their own palettes (icons, donut).
        result['ui-no-hex-colors'] = len(re.findall(r'\b0x[0-9a-fA-F]{6}\b', code))
    return {rule: count for rule, count in result.items() if count}


def scan(root: Path = ROOT) -> dict[str, dict[str, int]]:
    found = {rule: {} for rule in RULES}
    for layer in ('components', 'views', 'adapters', 'shell'):
        for path in sorted((root / 'src/ui' / layer).rglob('*.rs')):
            for rule, count in violations(path.read_text(encoding='utf-8'), layer).items():
                found[rule][path.relative_to(root).as_posix()] = count
    return found


def growth(current: dict, allowed: dict) -> list[str]:
    errors = []
    for rule, files in current.items():
        for path, count in files.items():
            if not isinstance(count, int) or count < 0:
                errors.append(f'{rule}: invalid count for {path}')
            elif count > allowed.get(rule, {}).get(path, 0):
                errors.append(f'{rule}: {path}: {count} exceeds {allowed.get(rule, {}).get(path, 0)}')
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--update', action='store_true')
    parser.add_argument('--previous-baseline', type=Path)
    args = parser.parse_args()
    baseline = json.loads(BASELINE.read_text(encoding='utf-8'))
    # Migration is complete: the checked-in exception budget is permanently zero.
    if any(files for files in baseline.values()):
        print('UI migration baseline must remain empty'); return 1
    if args.previous_baseline:
        errors = growth(baseline, json.loads(args.previous_baseline.read_text(encoding='utf-8')))
        if errors:
            print('\n'.join(errors)); return 1
    current = scan()
    errors = growth(current, baseline)
    if errors:
        print('\n'.join(errors)); return 1
    if args.update:
        BASELINE.write_text(json.dumps(current, indent=2, sort_keys=True) + '\n', encoding='utf-8')
    print(f'OK: UI component boundaries; remaining exceptions: {sum(sum(files.values()) for files in current.values())}')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
