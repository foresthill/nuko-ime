# ぬこIME ロードマップ

最終更新: 2026-05-28

実装の **順序** を記述します。具体的な日付は意図的に書きません — 個人 OSS
として「いつまでに」を約束できないためです。Phase 完了の判定基準のみ明示します。

現状の構成・層分離は [`ARCHITECTURE.md`](ARCHITECTURE.md) を参照してください。

## Phase 0 — 入力層と最小起動 ✅ 完了

| 項目 | 状態 | 補足 |
|---|---|---|
| ローマ字→かな変換 | ✅ | Mozc テーブル準拠、rstest 蓄積中 |
| nn→ん の子音処理 | ✅ | PR #7 |
| 不変条件テスト (`proptest`) | 🟡 | Stage 1 (FUTURE_FEATURES 8.1) |
| macOS InputMethodKit 起動 | ✅ | モード切替・基本入力まで |
| 候補ウィンドウ | 🔴 | Phase 1 で実装 |

**完了判定**: macOS でローマ字入力 → かな確定 ができる (実機 dogfooding 可能)

## Phase 1 — libakaza 統合 (現在地)

### 1.1 spike (済)

`spikes/libakaza-build/` で libakaza v2026.404.0 の macOS ビルドを検証済。
詳細は [`ARCHITECTURE.md` §3.1](ARCHITECTURE.md) と
[`spike/libakaza-macos-build`](https://github.com/foresthill/nuko-ime/tree/spike/libakaza-macos-build)
ブランチを参照。

### 1.2 nuko-core への統合

- [ ] `nuko-core/Cargo.toml` に libakaza を追加
- [ ] libakaza の公開 API 表面調査 (Engine / 変換層を pub で叩けるか)
- [ ] ぬこIME 既存のローマ字層と libakaza の入力境界を明確化
  (libakaza に渡すのは **かな文字列** のみ、ローマ字層は本プロジェクト維持)
- [ ] `nuko-core::conversion::engine` に libakaza バックエンド実装
- [ ] フォールバック (libakaza モデルが無い場合の挙動) 設計
- [ ] 単体テスト

### 1.3 候補ウィンドウ最小実装

- [ ] `nuko-ui` で候補表示 (`iced` ベース or InputMethodKit native)
- [ ] 確定・キャンセル・次候補のキーバインド

**完了判定**: 「にほんご」入力 → 「日本語」など実用的な変換候補が出る

## Phase 2 — 自前モデル生成パイプライン

`akaza-default-model` (SKK-JISYO.L 含む GPL 汚染) を使わずモデルを生成。

- [ ] データ取得スクリプト (SudachiDict, UniDic, Wikipedia, 青空文庫)
- [ ] `jawiki-kana-kanji-dict` ベースの辞書生成スクリプト
- [ ] bigram コーパス生成 (Wikipedia → 統計データ)
- [ ] libakaza 互換のバイナリモデル出力
- [ ] GitHub Actions ワークフロー (6 時間制限内に収まるよう分割)
- [ ] モデルファイルのリリース添付 (`gh release upload`)
- [ ] モデル取得・配置の利用者向け手順

**完了判定**: ユーザーが GitHub Release からモデルをダウンロード → 配置で動作

## Phase 3 — 学習機構 (FUTURE_FEATURES §8 と一致)

詳細設計は [`FUTURE_FEATURES.md` §8](FUTURE_FEATURES.md) を参照。

- [ ] Stage 1: `proptest` 不変条件 (現 nuko-core にも適用可)
- [ ] Stage 2: ログ取得基盤 (成功/失敗、デフォルト OFF、ローカル保存)
- [ ] Stage 3: 頻度ベース学習 (libakaza 既存機構を活用)
- [ ] Stage 4: 文脈ベース学習 / AI 連携の足場

**完了判定**: ユーザー固有の語彙が変換順位に反映される

## Phase 4 — AI 連携 (BYOK)

詳細設計は [`FUTURE_FEATURES.md` §8.5](FUTURE_FEATURES.md) と
[`AI_AGENT_FOUNDATION.md`](AI_AGENT_FOUNDATION.md) を参照。

- [ ] `~/.config/nuko-ime/ai.toml` 設定スキーマ
- [ ] プロバイダー抽象 (Anthropic / Gemini / OpenAI / local LLM / none)
- [ ] API キーは環境変数経由 (本体に保存しない、BYOK)
- [ ] デフォルト OFF、明示的オプトイン
- [ ] ログのオプトイン送信 (プライバシー注意点を UI で明示)

**完了判定**: ユーザーが自分の API キーを設定すれば AI 連携で変換精度が向上する

## Phase 5+ — クロスプラットフォーム展開

Phase 1-4 が macOS で安定したあと:

- Linux: IBus / Fcitx5 統合 (`nuko-platform/linux`)
- Windows: TSF 統合 (`nuko-platform/windows`)
- 配布: AUR, Homebrew, winget 等パッケージマネージャ対応

**判断材料**: Phase 1-4 完了時点で個人運用に耐える品質に達しているか、
コントリビュータが現れて他 OS を担当できるか。

## 横断: 継続的に取り組む課題

- macOS 実機の品質バグ修正 (メニュー UI 重複、アイコンキャッシュ等)
- ライセンス境界の維持 (依存追加時の検証)
- README / Cargo.toml / リリース情報の現実化
- ドキュメントの陳腐化監視 (Phase 完了時に ARCHITECTURE / ROADMAP を更新)

## 関連

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — 現在の構成
- [`FUTURE_FEATURES.md`](FUTURE_FEATURES.md) — 将来機能の設計
- [`AI_AGENT_FOUNDATION.md`](AI_AGENT_FOUNDATION.md) — IME を AI エージェント基盤と捉える思想
- [`archive/`](archive/) — 過去の計画ドキュメント
