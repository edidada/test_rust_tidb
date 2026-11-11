#!/usr/bin/env python3
"""L0 可复现实验脚本模板（docs/03 L0 DoD、docs/05 §7.2 / §9.2 / F-TEST-003）。

约束：
  - 固定种子（common::SeedPolicy 的 SplitMix64 派生，与 Rust 侧一致）
  - 每个基准重复 ≥5 次，输出均值/标准差/95% 置信区间（docs/05 §9.2）
  - 实验元数据（commit/seed/repeat/config/dataset/baseline）自动归档

用法：
    python3 scripts/bench_template.py --name scan-bench --repeat 5 --seed 20261109
    python3 scripts/bench_template.py --name scan-bench --self-test
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ARTIFACTS = ROOT / "artifacts" / "M0" / "AC-TEST-001" / "raw"

MASK64 = 0xFFFFFFFFFFFFFFFF


class SeedPolicy:
    """与 crates/common/src/seed.rs 中 SeedPolicy::derive 完全一致的 Python 实现。"""

    def __init__(self, master: int):
        self.master = master

    def derive(self, component: str, index: int) -> int:
        h = 0x9E3779B97F4A7C15
        for ch in component.encode("utf-8"):
            h = ((h ^ ch) * 0x100000001B3) & MASK64
        x = (self.master ^ h ^ ((index * 0x9E3779B97F4A7C15) & MASK64)) & MASK64
        x = ((x ^ (x >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
        x = ((x ^ (x >> 27)) * 0x94D049BB133111EB) & MASK64
        return (x ^ (x >> 31)) & MASK64

    @staticmethod
    def next_random(state: int) -> int:
        state = (state * 6364136223846793005 + 1442695040888963407) & MASK64
        return state >> 33


def workload(name: str, seed: int) -> float:
    """确定性伪工作负载：用固定种子生成 10000 个样本并返回均值（毫秒）。

    L0 模板不依赖数据库；各 Lab 替换此函数为真实基准即可。
    """
    policy = SeedPolicy(seed)
    state = policy.derive(name, 0)
    total = 0
    for _ in range(10_000):
        total += SeedPolicy.next_random(state) % 1000
        state = (state * 6364136223846793005 + 1442695040888963407) & MASK64
    return total / 10_000 / 1000.0


def mean(values: list[float]) -> float:
    return sum(values) / len(values)


def stddev(values: list[float]) -> float:
    if len(values) < 2:
        return 0.0
    m = mean(values)
    return math.sqrt(sum((v - m) ** 2 for v in values) / (len(values) - 1))


def confidence_interval_95(values: list[float]) -> tuple[float, float]:
    """t 分布近似：n<30 用 t 值表，否则 1.96。"""
    if len(values) < 2:
        m = mean(values)
        return m, m
    t_table = {2: 12.706, 3: 4.303, 4: 3.182, 5: 2.776, 6: 2.571,
               7: 2.447, 8: 2.365, 9: 2.306, 10: 2.262}
    t = t_table.get(len(values), 1.96)
    half = t * stddev(values) / math.sqrt(len(values))
    m = mean(values)
    return m - half, m + half


def git_commit() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()
    except (subprocess.SubprocessError, FileNotFoundError):
        return "unknown"


def run(name: str, seed: int, repeat: int, config_path: str) -> dict:
    if repeat < 5:
        raise ValueError("docs/05 §9.2 要求每个实验 ≥5 次重复")
    samples = [workload(name, SeedPolicy(seed).derive(name, i)) for i in range(repeat)]
    ci_low, ci_high = confidence_interval_95(samples)
    return {
        "experiment": name,
        "seed": seed,
        "repeat": repeat,
        "commit": git_commit(),
        "config": config_path,
        "dataset": "L0-synthetic",
        "baseline": "self",
        "samples_ms": [round(v, 6) for v in samples],
        "mean_ms": round(mean(samples), 6),
        "stddev_ms": round(stddev(samples), 6),
        "ci95_ms": [round(ci_low, 6), round(ci_high, 6)],
    }


def self_test() -> int:
    """CI 自检：确定性 + 统计口径。"""
    a = run("scan-bench", 20261109, 5, "bench/env.toml")
    b = run("scan-bench", 20261109, 5, "bench/env.toml")
    assert a["samples_ms"] == b["samples_ms"], "同种子必须可复现"
    assert a["repeat"] == 5
    assert a["ci95_ms"][0] <= a["mean_ms"] <= a["ci95_ms"][1]
    assert a["stddev_ms"] >= 0.0
    print("bench_template self-test: 通过（确定性 / ≥5 次 / 均值-方差-CI）")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="L0 可复现实验模板")
    parser.add_argument("--name", default="scan-bench")
    parser.add_argument("--repeat", type=int, default=5)
    parser.add_argument("--seed", type=int, default=20261109)
    parser.add_argument("--config", default="bench/env.toml")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    result = run(args.name, args.seed, args.repeat, args.config)
    ARTIFACTS.mkdir(parents=True, exist_ok=True)
    out = ARTIFACTS / f"{args.name}.json"
    out.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(result, ensure_ascii=False, indent=2))
    print(f"已归档: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
