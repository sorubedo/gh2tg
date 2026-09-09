# GH2TG

[English](README.md) | [中文](README.zh-CN.md)

GH2TG は、GitHub から Telegram へ通知を送る単発実行型のポーラーです。設定したリポジトリのコミット、Release、GitHub Actions を確認し、更新内容と条件に一致するビルド成果物を Telegram スーパーグループのトピックに送信します。

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
- GitHub Token

## クイックスタート

以下の環境変数を設定します。

```bash
GH2TG_BOT_TOKEN=your_telegram_bot_token
GH2TG_GROUP_ID=-1001234567890
GH2TG_GITHUB_TOKEN=your_github_token
LANG=ja
```

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

すぐにデプロイできる [GH2TG GitHub Actions テンプレート](https://github.com/sorubedo/gh2tg-template) を使用してください。

## リリース

`Cargo.toml` のバージョンと一致するタグ（例：`v0.2.1`）を push すると、cargo-dist が各プラットフォーム向けアーカイブを自動でビルドし、GitHub Release を作成します。

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
