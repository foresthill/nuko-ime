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

- **高速**: Rust製による高速な変換処理
- **軽量**: 最小限のメモリ使用量
- **学習機能**: ユーザーの入力パターンに適応
- **プライバシー重視**: 完全オフライン動作、データは端末内のみ
- **クロスプラットフォーム**: Windows / macOS / Linux対応

## インストール

### Windows

```powershell
# winget (準備中)
winget install nuko-ime
```

### macOS

```bash
# Homebrew (準備中)
brew install nuko-ime
```

### Linux

```bash
# AUR (準備中)
yay -S nuko-ime
```

### ソースからビルド

```bash
git clone https://github.com/your-org/nuko-ime.git
cd nuko-ime
cargo build --release
```

## 使い方

インストール後、システムの入力ソース設定から「ぬこIME」を追加してください。

### キーバインド

| キー | 動作 |
|-----|------|
| Space | 変換 |
| Enter | 確定 |
| Tab | 次の候補 |
| Shift+Tab | 前の候補 |
| Escape | キャンセル |
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
├── nuko-core/       # コアエンジン
├── nuko-platform/   # OS統合層
├── nuko-ui/         # UI コンポーネント
├── nuko-cli/        # CLIツール
├── tools/           # 開発ツール
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

- [開発仕様書](docs/SPECIFICATION.md)
- [Issues](https://github.com/your-org/nuko-ime/issues)
- [Discussions](https://github.com/your-org/nuko-ime/discussions)

---

Made with :cat: in Japan
