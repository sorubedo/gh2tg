# GH2TG

[English](README.md) | [中文](README.zh-CN.md)

[![Telegram](https://img.shields.io/badge/Telegram-@gh2tg-26A5E4?logo=telegram&logoColor=white)](https://t.me/gh2tg)

GH2TG は、GitHub から Telegram へ通知を送る単発実行型のポーラーです。設定したリポジトリのコミット、Release、GitHub Actions を確認し、更新内容と条件に一致するビルド成果物を Telegram スーパーグループのトピックに送信します。

## GH2TG の利点

- **セルフホストサーバー不要：** GH2TG は単発実行型のため、常駐サービスを維持せず GitHub Actions で定期実行できます。
- **監視対象リポジトリの権限不要：** GitHub API で公開情報をポーリングするため、GitHub App のインストール、Webhook の設定、各リポジトリの変更は不要です。
- **複数リポジトリを一元管理：** 複数リポジトリの Commit、Release、GitHub Actions の更新を 1 つの Telegram スーパーグループに集約し、リポジトリごとに専用トピックへ送信します。
- **状態とトピックを自動管理：** `state.json` に進捗を記録して重複通知を防ぎ、リポジトリのトピックを自動的に作成、名前変更、閉鎖します。

## 機能

- 指定したブランチの新しいコミットを監視
- 最新 Release、プレリリース、または指定した Tag を監視
- GitHub Actions のワークフローを監視し、一致する Artifact を送信
- リポジトリごとの Telegram トピックを自動作成、名前変更、閉鎖
- `state.json` にカーソルを保存し、重複通知を防止
- コンソール出力は英語、中国語、日本語に対応

## 必要条件

- トピックを有効にした Telegram スーパーグループ
- トピック管理権限を持つ Telegram Bot

## クイックスタート

以下の環境変数を設定します。

```bash
GH2TG_BOT_TOKEN=your_telegram_bot_token
GH2TG_GROUP_ID=-1001234567890
LANG=ja
```

公開リポジトリを監視する場合、`GH2TG_GITHUB_TOKEN` は任意です。GitHub API のレート制限を引き上げる場合、プライベートリポジトリへアクセスする場合、または GitHub Actions Artifact をダウンロードする場合は設定してください。Actions Workflow に `artifact_regex` を設定した場合は Token が必要です。

`LANG` は `en`（英語、デフォルト）、`zh`（中国語）、`ja`（日本語）に対応しています。`en_US.UTF-8`、`zh_CN.UTF-8`、`ja_JP.UTF-8` などのロケール形式も使用できます。

`config.json` を編集し、例にある `owner/repo`、ブランチ、ワークフロー、ファイルのマッチング条件を実際の値に置き換えます。その後、次のコマンドを実行します。

```bash
gh2tg
```

デフォルトでは、カレントディレクトリの `config.json` と `state.json` を読み込みます。パスを指定することもできます。

```bash
gh2tg --config /path/to/config.json --state /path/to/state.json
```

初回実行時に現在の状態をベースラインとして `state.json` に保存し、既存の履歴は送信しません。2 回目以降は新しく検出された更新だけを処理します。cron、systemd timer、または別のスケジューラーで定期的に実行できます。

## GitHub Actions でのデプロイ

すぐにデプロイできる [GH2TG GitHub Actions テンプレート](https://github.com/sorubedo/gh2tg-template) を使用してください。テンプレートは GH2TG を定期実行し、更新された `state.json` を自動的にコミットするため、セルフホストサーバーや手動での状態管理は不要です。

## Docker

GitHub Actions は GHCR に次の 2 つのイメージを公開します。

- `ghcr.io/sorubedo/gh2tg:latest`：単発実行
- `ghcr.io/sorubedo/gh2tg:cron`：cron による定期実行

`.env` と `config.json` をカレントディレクトリに置きます。

単発実行イメージを起動します。

```bash
docker run --rm \
  --env-file .env \
  -v "$PWD:/data" \
  ghcr.io/sorubedo/gh2tg:latest
```

cron イメージを起動します。

```bash
docker run -d \
  --name gh2tg-cron \
  --restart unless-stopped \
  --env-file .env \
  -v "$PWD:/data" \
  ghcr.io/sorubedo/gh2tg:cron
```

Cron イメージの環境変数：

- `GH2TG_CRON`：実行スケジュール、デフォルトは `0 * * * *`
- `GH2TG_CONFIG`：設定ファイルのパス、デフォルトは `config.json`
- `GH2TG_STATE`：状態ファイルのパス、デフォルトは `state.json`

## 設定

完全な例は [`config.example.json`](config.example.json) を参照してください。各リポジトリでは、必要なモジュールだけを有効にできます。

- `commits.branches`：確認するブランチ
- `releases.include_prereleases`：プレリリースを選択するかどうか
- `releases.tag`：確認する特定の Release Tag
- `releases.asset_regex`：送信する Release 添付ファイル名の正規表現
- `actions.workflows`：ワークフローファイル、ブランチ、結論、Artifact ファイル名のパターン

`topic_name` を省略すると、リポジトリ名が使用されます。設定ファイルの `schema_version` は `1` です。
