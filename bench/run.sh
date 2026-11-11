#!/usr/bin/env bash
# 一键重跑实验（docs/05 §9.3 artifact 规范）
set -euo pipefail
EXP="${1:-scan-bench}"
REPEAT="${REPEAT:-5}"
SEED="${SEED:-20261109}"
python3 "$(dirname "$0")/../scripts/bench_template.py" \
  --name "$EXP" --repeat "$REPEAT" --seed "$SEED" --config bench/env.toml
