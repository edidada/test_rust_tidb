# 实验环境定义（docs/05 §9.3 / AC-BENCH-008）
# 与 CI 保持一致的固定工具链镜像；禁用浮动标签保证可复现。
FROM rust:1.99-bookworm

RUN rustup component add rustfmt clippy llvm-tools-preview

WORKDIR /workspace
COPY . .

CMD ["cargo", "test", "--workspace"]
