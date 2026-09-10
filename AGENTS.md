# 仓库指南

## 项目结构与模块组织

GH2TG 是一个使用 Rust 2024 edition 的单二进制 crate。`src/main.rs` 定义命令行入口，`src/app.rs` 负责整体流程编排。提交、Release 和 GitHub Actions 的业务逻辑分别位于 `commit.rs`、`release.rs` 与 `actions.rs`。GitHub API 代码集中在 `src/github/`；Telegram 传输与消息发布位于 `telegram.rs` 和 `telegram_report.rs`。配置、持久化游标、消息格式化及本地化分别由 `config.rs`、`state.rs`、`message.rs` 和 `i18n.rs` 负责。

测试与模块代码放在同一文件的 `#[cfg(test)] mod tests` 中。部署文件位于仓库根目录及 `docker/`。公共配置示例以 `config.example.json` 为准；本地的 `config.json`、`.env`、`state.json` 和 `target/` 均已忽略。

## 构建、测试与开发命令

- `cargo build`：编译调试版二进制文件。
- `cargo run -- --help`：在不访问 GitHub 或 Telegram 的情况下检查 CLI。
- `cargo run -- --config config.json --state state.json`：使用本地配置执行一次轮询。
- `cargo test`：运行全部单元测试和异步集成风格测试。
- `cargo fmt --all -- --check`：检查格式；使用 `cargo fmt --all` 自动格式化。
- `cargo clippy --all-targets --all-features -- -D warnings`：运行 Clippy，并将警告视为错误。
- `cargo build --release`：生成 Docker 镜像使用的优化版二进制文件。

## 编码风格与命名约定

遵循标准 `rustfmt` 输出，使用四空格缩进。模块、函数和变量使用 `snake_case`；结构体、枚举和 trait 使用 `PascalCase`；常量使用 `SCREAMING_SNAKE_CASE`。业务行为应留在对应领域模块，API 序列化细节应放在 `src/github/`。优先使用 `thiserror` 定义类型化错误、使用 `serde` 进行结构化解析；仅需跨模块访问时，优先采用较窄的 `pub(crate)` 可见性。

## 测试规范

在被修改代码附近添加聚焦行为的测试。测试函数名应描述可观察行为，例如 `selector_change_establishes_a_new_baseline`。覆盖成功路径、状态转换、非法输入及相关 API 边界情况。提交前运行格式检查、Clippy 和完整测试套件。

## 提交与 Pull Request 规范

近期历史采用简短、祈使式的提交主题，并常用 `docs:`、`chore:`、`release:` 等前缀。适用时沿用该格式，并确保每个提交只处理一个明确主题。Pull Request 应说明行为变化、列出验证命令、关联相关 Issue，并指出配置或状态 schema 的影响。用户可见行为发生变化时附上控制台输出示例；本项目是 CLI，通常无需截图。

## 安全与配置

严禁提交 Bot Token、GitHub Token、真实群组 ID 或生成的状态数据。更新 `.env.example` 或 `config.example.json` 时只使用占位值。修改持久化状态或配置 schema 时，应明确处理向后兼容性。

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues for `sorubedo/gh2tg`. See `docs/agents/issue-tracker.md`.

### Triage labels

The repo uses the default five-label triage vocabulary. See `docs/agents/triage-labels.md`.

### Domain docs

This repo uses a single-context domain documentation layout. See `docs/agents/domain.md`.
