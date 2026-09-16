# ぬこIME

```
       /\___/\
      ( =^・ω・^= )
       (")_ぬこ_(")
```

**日本人の、日本人による、日本人のためのIME**

> 音声入力が上位互換と思われるが、
> 手入力する必要性 (仕事などで) も少しはまだ残っているため。

[![License](https://img.shields.io/badge/license-Apache--2.0_OR_MIT-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange?logo=rust&logoColor=white)](https://www.rust-lang.org/)

## 概要

ぬこIMEは、日本語ユーザーの入力効率を最大化するために設計されたオープンソースのインプットメソッドです。シンプルで軽量、かつ高速な動作を目指しています。

## 特徴

- **高速**: Rust製による高速な変換処理を目指す
- **軽量**: 最小限のメモリ使用量を目指す
- **学習機能**: ユーザーの入力パターンに適応 (頻度ベースを実装済、AI 蒸留は将来)
- **プライバシー重視**: 完全オフライン動作、データは端末内のみ
- **候補ウィンドウ**: 自前 NSPanel ベースで複数候補を一覧表示、数字 1-9 で直接選択
- **マルチプラットフォーム指向**: 現在は **macOS** で開発中。Linux / Windows は将来対応予定

> [!NOTE]
> ぬこIMEは現在 **開発初期段階** です。実機で動作するのは macOS のみ。
> 変換エンジン (libakaza) は統合済で、日常入力レベルの実用域には到達しています。
> 詳細は [ロードマップ](docs/ROADMAP.md) を参照してください。

## インストール

**macOS 専用**です(Windows/Linux は設計上の想定のみ、未実装)。

### プレビュー版 (ビルド不要・推奨)

[GitHub Releases](https://github.com/foresthill/nuko-ime/releases) から未署名の
プレビュー版を入手できます。**未署名のため導入に手作業が要ります**:

1. `NukoIME-v0.1.0-macos.zip` を展開 → `NukoIME.app` を `~/Library/Input Methods/` に置く
2. Finder で `NukoIME.app` を**右クリック →「開く」**、または
   **システム設定 → プライバシーとセキュリティ**で許可(Gatekeeper 対策)
3. 変換モデルを配置(これが無いと変換品質が落ちる):
   ```bash
   mkdir -p "$HOME/Library/Application Support/nuko-ime"
   tar -xzf nuko-ime-model-v0.1.0.tar.gz -C "$HOME/Library/Application Support/nuko-ime/"
   ```
4. ログアウト→ログイン後、システム設定 → キーボード → 入力ソースで「ぬこIME」を追加

> 署名 + notarization(Gatekeeper 警告なしの導入)と、モデルの初回自動 DL は次版で対応予定。

### ソースからビルド (macOS)

```bash
git clone https://github.com/foresthill/nuko-ime.git
cd nuko-ime

# 静的辞書のみ (最小構成)
./nuko-macos/scripts/install.sh

# libakaza ベースの統計変換を有効化する場合 (推奨)
# モデルファイルを ~/Library/Application Support/nuko-ime/akaza-model/ に
# 配置する手順は model-pipeline/README.md を参照
FEATURES=akaza ./nuko-macos/scripts/install.sh
```

libakaza バックエンドの **モデル生成・配置手順** は
[`model-pipeline/README.md`](model-pipeline/README.md) を参照してください。

### モデル更新時の反映 (2 種類)

Rust コード変更時は `install.sh` で `.app` 再ビルド。libakaza モデル更新時は以下のファイル配置 + `killall NukoIME`:

```bash
MODEL_DIR="$HOME/Library/Application Support/nuko-ime/akaza-model"
cp -v model-pipeline/data/SKK-JISYO.akaza \
      model-pipeline/data/unigram.model \
      model-pipeline/data/bigram.model \
      model-pipeline/data/bigram.model.scores \
      model-pipeline/data/skip_bigram.model \
      model-pipeline/data/skip_bigram.model.scores \
      "$MODEL_DIR/"
killall NukoIME
```

**`*.scores` ファイル 2 つも必須** (合計 6 ファイル)。漏らすと libakaza が silent fail して静的辞書フォールバックになる。

## 使い方

インストール後、システムの入力ソース設定から「ぬこIME」を追加してください。

### キーバインド

| キー | 動作 |
|-----|------|
| Space | 変換 / 候補表示中は次候補へ巡回 |
| ↓ | 次候補 (候補表示中) |
| ↑ | 前候補 (候補表示中) |
| 数字 1-9 | 該当行の候補を直接確定 |
| Enter | 現在の選択を確定 |
| Escape | 変換をキャンセル |
| Tab | 次の候補 |
| Shift+Tab | 前の候補 |
| F7 | カタカナ変換 |
| F8 | 半角カタカナ変換 |
| F9 | 全角英数変換 |
| F10 | 半角英数変換 |

## 開発

### 必要環境

- Rust 1.75.0以上
- Python 3.11以上（ツール用）

### ビルド

```bash
# 開発ビルド
cargo build

# リリースビルド
cargo build --release

# テスト実行
cargo test

# ベンチマーク
cargo bench
```

### プロジェクト構造

```
nuko-ime/
├── nuko-core/       # コアエンジン (入力・変換・辞書・学習)
├── nuko-platform/   # OS統合の抽象層
├── nuko-macos/      # macOS InputMethodKit 統合
├── nuko-ui/         # UI コンポーネント
├── nuko-cli/        # CLIツール
├── spikes/          # 短命の検証用 crate (ワークスペース外)
└── docs/            # ドキュメント
```

## 貢献

プルリクエストを歓迎します！詳細は[CONTRIBUTING.md](CONTRIBUTING.md)をご覧ください。

### 開発に参加する

1. このリポジトリをフォーク
2. フィーチャーブランチを作成 (`git checkout -b feature/amazing-feature`)
3. 変更をコミット (`git commit -m 'Add amazing feature'`)
4. ブランチをプッシュ (`git push origin feature/amazing-feature`)
5. プルリクエストを作成

## ライセンス

Apache License 2.0 または MIT License のデュアルライセンスです。

## 関連リンク

- [開発ノート (CLAUDE.md)](CLAUDE.md) — ビルド/デプロイ手順・落とし穴
- [アーキテクチャ](docs/ARCHITECTURE.md) — 現在の構成
- [ロードマップ](docs/ROADMAP.md) — Phase 別の実装順序
- [将来機能](docs/FUTURE_FEATURES.md)
- [AI エージェント基盤としての思想](docs/AI_AGENT_FOUNDATION.md)
- [Issues](https://github.com/foresthill/nuko-ime/issues)
- [Discussions](https://github.com/foresthill/nuko-ime/discussions)

---

Made with :cat: in Japan
