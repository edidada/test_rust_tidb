# 实验记录模板（L0 · docs/05 §9 可复现规范）

> 复制本文件到 `artifacts/<M*>/<AC-*>/report.md` 并逐项填写；任何进入论文的
> 结论必须满足「同基线、同数据集、同环境、置信区间、消融对照」。

## 1. 元数据

| 字段 | 值 |
| --- | --- |
| 实验 ID | `EXP-YYYY-M*-NNN` |
| 关联验收项 | `AC-*` |
| 关联研究问题 | `RQ-*` |
| git commit | `<sha>` |
| 种子 | `20261109`（或派生子种子） |
| 重复次数 | ≥ 5（建议 7） |
| 数据集 | 名称 / SF / skew / generator_version |
| 环境 | `bench/env.toml@<sha>` |
| 配置 | cost_weights / broadcast_threshold / cbo_timeout / chunk_capacity |
| 基线 | 系统 / commit |

## 2. 统计结果

| 指标 | 均值 | 标准差 | 95% 置信区间 | 异常值剔除说明 |
| --- | --- | --- | --- | --- |
| 目标指标 |  |  |  |  |

- 异常值按预定规则（如 3σ）剔除并说明。
- 对比实验同环境、同数据集、同基线；跨系统只做机制讨论。
- 消融实验一次只变一个变量，附交互项检查。

## 3. 性能归因

- `perf` / flamegraph / roofline 产物路径：
- 逐算子归因结论：

## 4. 一键重跑

```bash
python3 scripts/bench_template.py --name <exp-name> --repeat 5 --seed 20261109
```

## 5. 复核记录

- 复核人独立重跑相对偏差：____%（阈值 ≤ 5%，AC-BENCH-009）
- 结论（证实 / 证伪 / 退回）：
