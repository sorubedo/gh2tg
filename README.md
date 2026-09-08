# GH2TG

[中文文档](README.zh-CN.md) | [日本語](README.ja.md)

GH2TG is a one-shot GitHub-to-Telegram poller. It checks commits, releases, and GitHub Actions for configured repositories, then sends updates and matching build artifacts to topics in a Telegram supergroup.

## Features

- Monitor new commits on configured branches
- Monitor the latest release, prereleases, or a specific tag
- Monitor GitHub Actions workflows and send matching artifacts
- Automatically create, rename, and close Telegram topics for repositories
- Store cursors in `state.json` to avoid duplicate notifications
- Support English, Chinese, and Japanese console output

## Requirements

- A Telegram supergroup with Topics enabled
- A Telegram Bot with administrator permission to manage topics
- A GitHub Token

## Quick Start

Set the following environment variables:

```bash
GH2TG_BOT_TOKEN=your_telegram_bot_token
GH2TG_GROUP_ID=-1001234567890
GH2TG_GITHUB_TOKEN=your_github_token
LANG=en
```

`LANG` supports `en` (English, default), `zh` (Chinese), and `ja` (Japanese). Locale formats such as `en_US.UTF-8`, `zh_CN.UTF-8`, and `ja_JP.UTF-8` are also supported.

Edit `config.json` and replace the example `owner/repo`, branches, workflows, and file-matching rules with your actual values. Then run:

```bash
gh2tg
```

By default, GH2TG reads `config.json` and `state.json` from the current directory. You can also specify custom paths:

```bash
gh2tg --config /path/to/config.json --state /path/to/state.json
```

The first run establishes the current state baseline and saves it to `state.json`; existing history is not sent. Later runs only process newly detected updates. Run GH2TG periodically with cron, a systemd timer, or another scheduler.

## GitHub Actions Deployment

The [`gh2tg-template`](https://github.com/sorubedo/gh2tg-template) repository contains a ready-to-use GitHub Actions deployment. It runs the public `ghcr.io/sorubedo/gh2tg:latest` image every two hours and commits `state.json` back to the template repository so updates are not sent repeatedly.

1. Copy the template into a new GitHub repository.
2. In **Settings → Actions → General**, set **Workflow permissions** to **Read and write permissions**.
3. Add these repository secrets under **Settings → Secrets and variables → Actions**: `GH2TG_BOT_TOKEN`, `GH2TG_GROUP_ID`, and `GH2TG_GITHUB_TOKEN`.
4. Edit `config.json`, then run **Actions → Run GH2TG → Run workflow** once.

`GH2TG_GITHUB_TOKEN` must be able to read the repositories being monitored. You do not need to download, create, or upload `state.json` manually: the first run creates it, and the workflow maintains it automatically.

## Docker

The project publishes two images to GHCR:

- `ghcr.io/sorubedo/gh2tg:latest`: one-shot execution
- `ghcr.io/sorubedo/gh2tg:cron`: periodic execution with cron

Put `.env` and `config.json` in the current directory.

Run the one-shot image:

```bash
docker run --rm \
  --env-file .env \
  -v "$PWD:/data" \
  ghcr.io/sorubedo/gh2tg:latest
```

Run the cron image:

```bash
docker run -d \
  --name gh2tg-cron \
  --restart unless-stopped \
  --env-file .env \
  -v "$PWD:/data" \
  ghcr.io/sorubedo/gh2tg:cron
```

Cron image environment variables:

- `GH2TG_CRON`: schedule, default `0 * * * *`
- `GH2TG_CONFIG`: config path, default `config.json`
- `GH2TG_STATE`: state path, default `state.json`

## Configuration

See [`config.example.json`](config.example.json) for a complete example. Each repository can enable the following modules as needed:

- `commits.branches`: branches to check
- `releases.include_prereleases`: whether to select prereleases
- `releases.tag`: a specific release tag to check
- `releases.asset_regex`: a regular expression for release asset file names to send
- `actions.workflows`: workflow files, branches, conclusions, and artifact file-name patterns

If `topic_name` is omitted, the repository name is used. The configuration file uses `schema_version: 1`.
