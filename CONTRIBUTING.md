# ぬこIME への貢献

ぬこIMEへの貢献を検討いただきありがとうございます！

## 開発環境のセットアップ

### 必要なツール

- Rust 1.75.0以上
- Git

### ビルド手順

```bash
# リポジトリをクローン
git clone https://github.com/your-org/nuko-ime.git
cd nuko-ime

# ビルド
cargo build

# テスト
cargo test

# CLIツールを実行
cargo run -p nuko-cli -- info
```

## 貢献の方法

### バグ報告

1. 既存のIssueを確認してください
2. 新しいIssueを作成する場合は、以下を含めてください：
   - 再現手順
   - 期待する動作
   - 実際の動作
   - 環境情報（OS、Rustバージョン等）

### 機能提案

1. Discussionsで議論を開始してください
2. 合意が得られたらIssueを作成

### プルリクエスト

1. フォークしてブランチを作成
2. 変更を実装
3. テストを追加・実行
4. `cargo fmt` と `cargo clippy` を実行
5. プルリクエストを作成

### コーディング規約

- `cargo fmt` でフォーマット
- `cargo clippy` で警告ゼロを維持
- ドキュメントコメントを追加
- テストを書く

## 初めての貢献

`good first issue` タグが付いたIssueは、初めての貢献者向けです。

## コミュニケーション

- GitHub Issues: バグ報告、機能要望
- GitHub Discussions: 質問、議論

## ライセンス

貢献いただいたコードは Apache-2.0 OR MIT ライセンスで提供されます。
