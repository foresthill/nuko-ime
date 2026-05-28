# ぬこIME アーキテクチャ

最終更新: 2026-05-28

このドキュメントは「現在のぬこIMEがどう構成されているか」を記述します。
将来計画は [`ROADMAP.md`](ROADMAP.md)、機能アイデアは
[`FUTURE_FEATURES.md`](FUTURE_FEATURES.md) を参照してください。

## 1. 設計原則

- **Rust ファースト**: コア・プラットフォーム統合・UI すべて Rust。
  Python は辞書/モデル生成スクリプト等のオフライン処理に限定。
- **ライセンス: MIT OR Apache-2.0 デュアル**: Rust エコシステムの慣習に準拠。
  これと両立しない依存 (GPL 系) は本体に含めない。
- **プライバシー優先**: 入力データはローカル処理。外部送信はオプトインのみ
  (AI 機能を有効化した場合)。詳細は
  [`FUTURE_FEATURES.md` §8.5 BYOK](FUTURE_FEATURES.md) を参照。
- **シンプルさ優先**: ロジックを増やす前に既存パターンの再利用と既存
  OSS の活用を検討する。

## 2. 層分離

```
┌─────────────────────────────────────────────────────────┐
│ プラットフォーム統合層                                    │
│   nuko-macos (InputMethodKit)                            │
│   nuko-platform (現状は薄い OS 抽象 / 将来 Linux/Win)    │
└──────────────────────┬──────────────────────────────────┘
                       │ Rust API
┌──────────────────────▼──────────────────────────────────┐
│ コア層 (nuko-core)                                       │
│                                                          │
│  ┌────────────────┐    ┌──────────────────────────────┐ │
│  │ 入力層          │    │ 変換層                        │ │
│  │ romaji.rs      │───▶│ engine.rs                    │ │
│  │ kana.rs        │    │ candidate.rs / context.rs    │ │
│  │ mozc_table.rs  │    │ (Phase 1 で libakaza 統合)   │ │
│  └────────────────┘    └──────────┬───────────────────┘ │
│                                    │                     │
│  ┌────────────────┐    ┌──────────▼───────────────────┐ │
│  │ 辞書層          │    │ 学習層                        │ │
│  │ system.rs      │◀──▶│ frequency.rs / manager.rs    │ │
│  │ user.rs        │    │ (Phase 3 で AI 連携拡張)     │ │
│  └────────────────┘    └──────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│ UI 層 / CLI 層                                            │
│   nuko-ui (候補ウィンドウ等、現状最小)                   │
│   nuko-cli (開発ツール)                                   │
└─────────────────────────────────────────────────────────┘
```

### 各層の責務

| 層 | クレート | 現状 | 役割 |
|---|---|---|---|
| 入力 | `nuko-core::input` | ✅ 実装済 (rstest + nn→ん の子音処理含む) | ローマ字→かな (Mozc テーブルベース) |
| 変換 | `nuko-core::conversion` | 🟡 placeholder (かな + カタカナ系候補のみ) | かな→漢字 (Phase 1 で libakaza 統合) |
| 辞書 | `nuko-core::dictionary` | 🟡 雛形のみ | システム辞書 / ユーザー辞書 |
| 学習 | `nuko-core::learning` | 🟡 雛形のみ | 頻度学習 (将来 context / AI 連携) |
| プラットフォーム | `nuko-macos` | 🟡 起動・モード切替まで | macOS InputMethodKit バインディング |
| プラットフォーム | `nuko-platform` | 🟡 雛形のみ | OS 抽象 (Linux/Windows は将来) |
| UI | `nuko-ui` | 🟡 雛形のみ | 候補ウィンドウ等 |
| CLI | `nuko-cli` | 🟡 雛形のみ | 辞書ビルダー、ベンチ等 |

凡例: ✅ 実装済 / 🟡 雛形・部分実装 / 🔴 未着手

## 3. かな漢字変換の方針 — Path B (libakaza ベース)

### 3.1 採用根拠

- **コード**: [`libakaza`](https://github.com/akaza-im/akaza) (MIT, Tokuhiro Matsuno)
  を git 依存で取り込む。Viterbi + 2-gram + MARISA Trie + Cedarwood Trie +
  学習機構を備えた完成度の高い Rust 実装。
- **macOS ビルド検証済**: 2026-05 に
  [`spike/libakaza-macos-build`](https://github.com/foresthill/nuko-ime/tree/spike/libakaza-macos-build)
  ブランチで `cargo build` 通過を確認。依存ツリーに IBus/GTK/glib 系は登場せず。
- **ライセンス両立**: MIT のため本プロジェクト (MIT OR Apache-2.0) と矛盾なし。

### 3.2 モデルは自前生成 (akaza-default-model 不採用)

`akaza-default-model` リポジトリは **SKK-JISYO.L (GPL-2.0) 由来データ** を
含むため、ぬこIME に同梱すると本体ライセンスが汚染されます。
したがってモデル生成は別パイプラインで行います:

| データソース | ライセンス | 用途 |
|---|---|---|
| **SudachiDict** | Apache-2.0 | 一般語彙・固有名詞 |
| **UniDic** | BSD-3 (選択可) | 形態素解析用品詞 |
| **Wikipedia 日本語コーパス** | CC BY-SA 3.0 (transformative use 解釈) | bigram 統計 |
| **青空文庫** | パブリックドメイン | 追加コーパス |
| **jawiki-kana-kanji-dict** スクリプト | MIT | Wikipedia 由来辞書生成 |

生成パイプラインは GitHub Actions で実行し、生成物 (バイナリモデル) のみを
リリースに添付する方針です (詳細は Phase 2、[`ROADMAP.md`](ROADMAP.md))。

### 3.3 既知の検証残課題

Path B の技術的成立はビルド検証で確認済みですが、以下は次フェーズで検証します:

- libakaza の公開 API (Engine / kana_kanji モジュール) が外部から叩けるか
- `xdg::BaseDirectories` の macOS 上での挙動 (設定パスを inject する余地)
- ぬこIME 既存のローマ字層との接続点 (libakaza の `romkan` モジュールは使わず、
  かな文字列を直接 libakaza に渡す方針)

## 4. プラットフォーム境界

**現状: macOS のみが実機動作可能。**

| OS | 状態 | 備考 |
|---|---|---|
| macOS | 🟡 起動・モード切替まで | InputMethodKit binding (`nuko-macos`)、Apple Silicon (Darwin 24.x) で開発中 |
| Linux | 🔴 未着手 | IBus / Fcitx5 統合は Phase 5+ |
| Windows | 🔴 未着手 | TSF 統合は Phase 5+ |

README の「クロスプラットフォーム」記述は **将来の目標** であり、現時点では
事実と一致していません。修正予定 (別 PR で対応)。

## 5. ライセンス境界 (依存と本体の関係)

```
┌─────────────────────────────────────────────────────────┐
│ ぬこIME 本体 (MIT OR Apache-2.0)                          │
│                                                          │
│   依存可能:                                              │
│   ✅ MIT / Apache-2.0 / BSD-3 / BSD-2 / ISC / Zlib       │
│   ✅ CC BY-SA (transformative use 解釈、生成物が         │
│       独立であれば本体に伝播しない)                       │
│                                                          │
│   依存不可:                                              │
│   ❌ GPL-2.0 / GPL-3.0 (本体ライセンスを汚染する)        │
│   ❌ SKK-JISYO.L (GPL-2.0) — akaza-default-model も同様 │
└─────────────────────────────────────────────────────────┘
```

ライセンス互換性の検証は依存追加時の必須チェック項目です。
[`CONTRIBUTING.md`](../CONTRIBUTING.md) も参照してください。

## 6. 関連ドキュメント

- [`ROADMAP.md`](ROADMAP.md) — Phase 別の実装順序
- [`FUTURE_FEATURES.md`](FUTURE_FEATURES.md) — 将来機能の設計案
- [`AI_AGENT_FOUNDATION.md`](AI_AGENT_FOUNDATION.md) — IME を AI エージェント基盤と捉える思想
- [`archive/`](archive/) — 過去の設計ドキュメント
