# sapphire-ledger（日本語）

> Language: [English](README.md) | **日本語**

sapphire-framework上に構築された、ローカルファーストの複式簿記家計簿。ファイルベース・ローカルファースト・人間とAIエージェントの協働のために作られました。

## コンセプト

- **TOMLをソース・オブ・トゥルースに** — すべてのデータはプレーンな`.toml`ファイルに保存され、どんなツールでも読み書きできます
- **キャッシュ不要** — `load_workspace`は毎回TOMLファイルを直接読み込みます。家庭規模ならそれで十分速いので、SQLiteもデータベースも不要です
- **複式簿記** — すべての取引は貸借がバランスしたポスティング（通貨ごとに借方＝貸方）の集合です
- **最初から多通貨対応** — ポスティングは通貨をを持ち、通貨をまたぐ取引にはインラインの為替レートを扱います
- **1レコード1ファイル** — 取引・アカウント・残高アサーション・為替レートログの各エントリはそれぞれ独立したファイルに置かれ、人間とAIによる共同編集でもgitのマージコンフリクトが起きにくいようにしています
- **人間とAIの共同編集** — AIエージェント（Claudeなど）がgitやSyncthing同期を介して同じレジャーの読み書き・作成・編集をすることを想定して設計されています

## プロジェクト構成

```
sapphire-ledger/
├── cli/                     # 単一バイナリ sapphire-ledger：CLI、アプリサーバー（同期、stdio と /mcp の MCP）
├── desktop/                 # デスクトップGUI（egui）。台帳と同期。サーバーのクライアント
└── crates/
    ├── sapphire-ledger-core/  # データモデル、TOMLパーサ/シリアライザ、検証、書き込みパス
    └── sapphire-ledger-mcp/   # ledger の MCP ツールとサーバー側メソッド（ライブラリ）
```

## ステータス

データモデル、検証、書き込みパス、MCPサーバーは動作します。配布物は単一バイナリ`sapphire-ledger`で、ledger独自のコマンドと、[sapphire-framework](https://github.com/sapphire-turtle/sapphire-framework)のアプリサーバー用コマンド（`serve`、`status`、`service`、`workspace`、`workgroup`、`device`、`external-device`、`mcp`など）が同じ階層に並びます。サーバー（`sapphire-ledger serve`、または引数なしの`sapphire-ledger`）を起動した状態で、MCPクライアントを`sapphire-ledger mcp`に向けてstdio経由でエントリの読み書き・記録をしたり、`server.toml`で`/mcp`を有効にしてHTTP経由・外部デバイスのトークンで同じことができます。どちらもサーバーが選択中のワークスペースを対象にします（セットアップと認証モデルは[`docs/design.md`](docs/design.md#mcp-server)参照）。デスクトップアプリ（`sapphire-ledger-desktop`）はそのサーバーのクライアントで、勘定と残高、勘定ごとの仕訳の閲覧、取引・勘定の記録、フレームワークの同期パネルを備えます。CLIも書き込みに対応しています：

```
sapphire-ledger account add Expenses:Food --type Expense
sapphire-ledger tx add --narration "groceries" \
  --posting "Expenses:Food 1200 JPY" --posting "Assets:Cash -1200 JPY"
sapphire-ledger tx list --account Expenses:Food
```

`assertion add`と`price add`も利用可能です。すべてMCPツールと同じ`core::ops`の薄いラッパーなので、誰が入力してもルールは同じです。

## ライセンス

このリポジトリは異なるライセンスのコンポーネントを含みます：

| コンポーネント | ライセンス |
|-----------|---------|
| `sapphire-ledger-core` | MIT OR Apache-2.0 |
| `sapphire-ledger-mcp` | MIT OR Apache-2.0 |
| `sapphire-ledger`（`cli/`） | MIT OR Apache-2.0 |
| `sapphire-ledger-desktop` | MIT OR Apache-2.0 |

各コンポーネントのディレクトリにある`LICENSE-MIT` / `LICENSE-APACHE`ファイルに完全なライセンス文書があります。
