# AGENTS.md

本仓库的协作约定，供 AI 助手与贡献者参考。

## 提交前必跑的三条验证

任何代码改动在提交前，必须先本地跑通以下三条；**任一条失败都先修复，不得提交**：

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`

## 提交信息

采用约定式提交：`type(scope): 中文描述`，scope 取 crate 简称（`app` / `core` / `platform`）。
每行正文说明「问题 → 改动」，必要时附回归用例名。
