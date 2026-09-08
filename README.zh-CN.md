# GH2TG

[English](README.md) | [日本語](README.ja.md)

GH2TG 是一个单次执行的 GitHub 到 Telegram 轮询器：它检查配置仓库的提交、Release 和 GitHub Actions，并把更新与匹配的构建产物发送到 Telegram 超级群组的话题中。

## 功能

- 监控指定分支的新提交
- 监控最新 Release、预发布版本或指定 Tag
- 监控 GitHub Actions 工作流，并发送匹配的 Artifacts
- 自动为每个仓库创建、重命名和关闭 Telegram 话题
- 使用 `state.json` 保存游标，避免重复发送
- 支持英文、中文和日文控制台输出

## 环境要求

- 开启了“话题”的 Telegram 超级群组
- Telegram Bot，并授予管理员的“管理话题”权限
- GitHub Token

## 快速开始

设置以下环境变量：

```bash
GH2TG_BOT_TOKEN=你的 Telegram Bot Token
GH2TG_GROUP_ID=-1001234567890
GH2TG_GITHUB_TOKEN=你的 GitHub Token
LANG=zh
```

`LANG` 可选值：`en`（英文，默认）、`zh`（中文）、`ja`（日文）。也支持 `en_US.UTF-8`、`zh_CN.UTF-8`、`ja_JP.UTF-8` 等区域格式。

编辑 `config.json`，把示例中的 `owner/repo`、分支、工作流和文件匹配规则替换成实际值。然后运行：

```bash
gh2tg
```

默认读取当前目录下的 `config.json` 和 `state.json`，也可以指定路径：

```bash
gh2tg --config /path/to/config.json --state /path/to/state.json
```

首次运行会建立当前状态基线并保存到 `state.json`，不会发送已有历史内容。之后每次运行只处理检测到的新内容。可通过 cron、systemd timer 或其他任务调度器定期执行。

## 使用 GitHub Actions 部署

[`gh2tg-template`](https://github.com/sorubedo/gh2tg-template) 是可直接使用的 GitHub Actions 模板。它每两小时运行公开的 `ghcr.io/sorubedo/gh2tg:latest` 镜像，并把 `state.json` 提交回模板仓库，避免重复发送。

1. 将模板复制到一个新的 GitHub 仓库。
2. 打开 **Settings → Actions → General**，将 **Workflow permissions** 设置为 **Read and write permissions**。
3. 在 **Settings → Secrets and variables → Actions** 添加以下 Repository secrets：`GH2TG_BOT_TOKEN`、`GH2TG_GROUP_ID`、`GH2TG_GITHUB_TOKEN`。
4. 编辑 `config.json`，然后打开 **Actions → Run GH2TG → Run workflow** 手动执行一次。

`GH2TG_GITHUB_TOKEN` 必须能读取要监控的仓库。无需手动下载、创建或上传 `state.json`：首次运行会自动生成，之后由工作流自动维护。

## Docker

GitHub Actions 会向 GHCR 发布两个镜像：

- `ghcr.io/sorubedo/gh2tg:latest`：单次运行
- `ghcr.io/sorubedo/gh2tg:cron`：使用 cron 定时运行

将 `.env` 和 `config.json` 放在当前目录中。

运行单次执行镜像：

```bash
docker run --rm \
  --env-file .env \
  -v "$PWD:/data" \
  ghcr.io/sorubedo/gh2tg:latest
```

运行 cron 镜像：

```bash
docker run -d \
  --name gh2tg-cron \
  --restart unless-stopped \
  --env-file .env \
  -v "$PWD:/data" \
  ghcr.io/sorubedo/gh2tg:cron
```

Cron 镜像环境变量：

- `GH2TG_CRON`：执行计划，默认 `0 * * * *`
- `GH2TG_CONFIG`：配置文件路径，默认 `config.json`
- `GH2TG_STATE`：状态文件路径，默认 `state.json`

## 配置说明

`config.json` 的完整示例见 [`config.example.json`](config.example.json)。每个仓库可以按需启用以下模块：

- `commits.branches`：要检查的分支
- `releases.include_prereleases`：是否选择预发布版本
- `releases.tag`：固定检查某个 Release Tag
- `releases.asset_regex`：需要发送的 Release 附件文件名正则
- `actions.workflows`：工作流文件、分支、运行结论和 Artifact 文件名正则

`topic_name` 不填时默认使用仓库名。配置文件使用 `schema_version: 1`。
