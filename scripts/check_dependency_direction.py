#!/usr/bin/env python3
"""L0 CI 静态检查（docs/02 §3、docs/04 F-TEST-002、docs/05 AC-TEST-002）。

用法：
    python3 scripts/check_dependency_direction.py                # 依赖方向
    python3 scripts/check_dependency_direction.py --traceability # 追溯完整性

依赖方向契约（docs/02 §3）：
    server → executor → optimizer → planner → expr → types → common
    storage/meta 经 trait 注入到 executor，禁止反向引用。
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CRATES = ROOT / "crates"

# crate 名 → 允许的依赖层级（数字越小越底层）。
LAYER: dict[str, int] = {
    "common": 0,
    "types": 1,
    "expr": 2,
    "parser": 2,
    "planner": 3,
    "catalog": 4,
    "optimizer": 4,
    "storage": 1,
    "meta": 1,
    "executor": 5,
    "server": 6,
}

# 每个 crate 允许直接依赖的 crate（含同层协作，禁止反向）。
ALLOWED: dict[str, set[str]] = {
    "common": set(),
    "types": {"common"},
    "expr": {"common", "types"},
    "parser": {"common"},
    "planner": {"common", "types", "expr"},
    "catalog": {"common", "types", "expr", "parser", "planner"},
    "optimizer": {"common", "types", "expr", "planner"},
    "storage": {"common", "types"},
    "meta": {"common"},
    "executor": {"common", "types", "expr", "planner", "optimizer", "storage", "meta"},
    "server": {
        "common", "types", "expr", "parser", "catalog", "planner",
        "optimizer", "executor", "storage", "meta",
    },
}

DEP_RE = re.compile(r"^\s*lattice-([a-z]+)\s*=", re.MULTILINE)


def check_dependency_direction() -> list[str]:
    errors: list[str] = []
    for cargo in sorted(CRATES.glob("*/Cargo.toml")):
        crate = cargo.parent.name
        if crate not in ALLOWED:
            errors.append(f"[依赖方向] 未知 crate: {crate}（未登记到检查表）")
            continue
        text = cargo.read_text(encoding="utf-8")
        for dep in DEP_RE.findall(text):
            if dep == crate:
                errors.append(f"[依赖方向] {crate} 不能依赖自身")
            elif dep not in ALLOWED[crate]:
                errors.append(
                    f"[依赖方向] {crate} → {dep} 违反单向依赖 "
                    f"(允许: {', '.join(sorted(ALLOWED[crate])) or '无'})"
                )
            elif LAYER.get(dep, 99) > LAYER[crate]:
                errors.append(f"[依赖方向] {crate}(层{LAYER[crate]}) → {dep}(层{LAYER[dep]}) 反向引用")
    return errors


FEATURE_RE = re.compile(r"\|\s*(F-[A-Z]+-\d+)\s*\|")
LAB_RE = re.compile(r"\|\s*(L\d+)\s*\|")
AC_RE = re.compile(r"(AC-[A-Z]+-\d+)")
# 范围写法：AC-TYP-001..008 / AC-OP-001..020（docs 大量使用）。
AC_RANGE_RE = re.compile(r"(AC-[A-Z]+-)(\d+)\.\.(\d+)")


def expand_ac_ranges(text: str) -> set[str]:
    """把 `AC-XXX-001..008` 展开为 8 个具体编号，与单点引用合并。"""
    found = set(AC_RE.findall(text))
    for prefix, start, end in AC_RANGE_RE.findall(text):
        width = len(start)
        for n in range(int(start), int(end) + 1):
            found.add(f"{prefix}{n:0{width}d}")
    return found


def check_traceability() -> list[str]:
    """docs/05 §10.1：每个 F-* 至少 1 条 AC-*；每个 AC-* 至少 1 个 F-*。"""
    errors: list[str] = []
    feature_doc = (ROOT / "docs" / "04-feature-list.md").read_text(encoding="utf-8")
    acceptance_doc = (ROOT / "docs" / "05-acceptance-plan.md").read_text(encoding="utf-8")
    roadmap_doc = (ROOT / "docs" / "03-roadmap.md").read_text(encoding="utf-8")

    features = set(FEATURE_RE.findall(feature_doc))
    ac_in_features = expand_ac_ranges(feature_doc)
    ac_in_acceptance = expand_ac_ranges(acceptance_doc)
    labs = set(LAB_RE.findall(roadmap_doc))

    if not features:
        errors.append("[追溯] 04 文档未解析到任何 F-* 编号")
    if not labs:
        errors.append("[追溯] 03 文档未解析到任何 L* 编号")
    missing_ac = sorted(ac_in_acceptance - ac_in_features)
    for ac in missing_ac:
        errors.append(f"[追溯] {ac} 在 05 中定义，但 04 功能点表未引用（AC 无 F 回溯）")
    dangling_ac = sorted(ac_in_features - ac_in_acceptance)
    for ac in dangling_ac:
        errors.append(f"[追溯] {ac} 在 04 中被引用，但 05 未定义（F 无 AC 覆盖）")
    return errors


def main() -> int:
    mode_traceability = "--traceability" in sys.argv[1:]
    errors = check_traceability() if mode_traceability else check_dependency_direction()
    label = "追溯完整性" if mode_traceability else "依赖方向"
    if errors:
        print(f"{label}检查失败：")
        for err in errors:
            print(f"  - {err}")
        return 1
    print(f"{label}检查通过。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
