# ぬこIME ロードマップ

最終更新: 2026-06-03

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

### 1.2 nuko-core への統合 ✅ 完了

- [x] libakaza 公開 API 調査 (PR #13)
- [x] モデル不在エラー挙動の実機検証 (spike-2, PR #14)
- [x] `nuko-core/Cargo.toml` に `akaza` feature + optional libakaza (PR #15)
- [x] `LibakazaBackend` 実装 (try_new / convert / cost_to_score, PR #15)
- [x] `ConversionEngine::with_libakaza()` と静的辞書フォールバック契約 (PR #16)
- [x] Send 制約発見と対処方針 (spike-3, PR #17)
- [x] nuko-macos: thread_local 化 + `akaza` feature pass-through (PR #18)

### 1.3 候補ウィンドウ最小実装 / 残作業

- [ ] `nuko-ui` で候補表示 (`iced` ベース or InputMethodKit native)
- [ ] 確定・キャンセル・次候補のキーバインド
- [ ] libakaza 変換の文節別 API 化 (現在は連結 1 候補 = 案 C)
- [ ] `LIBAKAZA_PRIORITY_BOOST` キャリブレーション

**完了判定**: 「にほんご」入力 → 「日本語」など実用的な変換候補が出る (Phase 2 のモデル生成完了が前提)

## Phase 2 — 自前モデル生成パイプライン

libakaza 本体は MIT、上流 corpus-stats は CC BY-SA + PD + IPADIC BSD-3 で
**GPL を経由しない**ことを一次調査で確認済 (PR #19)。生成パイプラインは
[`model-pipeline/`](../model-pipeline/) ディレクトリに配置 (モノレポ運用)。

### 2-A spike-4: akaza-data CLI 検証 ✅ 完了 (PR #20)
- [x] cargo install で macOS ビルド確認、16 subcommand を `--help` で確定

### 2-B model-pipeline/ scaffold ✅ 進行中 (本 PR)
- [x] README / NOTICE / Makefile 骨格 / seed (上流流用) / `.github/workflows/model-build.yml`

### 2-C 最小モデル試作 (案 III)
- [ ] Makefile の download target 実装 (corpus-stats / UniDic / SudachiDict)
- [ ] `make-dict` / `learn-corpus` 呼び出し実装 (PROFILE=min)
- [ ] `akaza-data check` で疎通確認

### 2-D ぬこIME 本体で実機検証
- [ ] `NUKO_AKAZA_MODEL_DIR` でモデルを指定 → 変換が走る

### 2-E 本番モデル (案 I)
- [ ] PROFILE=full でビルド
- [ ] seed を自前再構築 (上流から置換)
- [ ] `model-v0.1.0` リリース、tarball 添付

### 2-F 手動配置のドキュメント
- [ ] README に展開手順を記載

### 2-G 自動ダウンローダ (将来)
- [ ] nuko-cli or nuko-macos に CLI 追加

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
