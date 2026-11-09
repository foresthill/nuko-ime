# ぬこIME v0.1.5 (macOS プレビュー)

v0.1.4 からの主眼は**配布まわりの整備**です。GitHub Actions による自動リリースに移行し、
**署名 + 公証（notarization）済み**の .app を配布します。

## 目玉

### 🔏 署名 + 公証済みビルド
Apple の **Developer ID** で署名し、Apple の公証（notarization）を通した .app を配布します。
- 従来の未署名版で出ていた Gatekeeper の強い警告（「開発元を検証できません」等）が軽減されます。
- ※ プレビュー（pre-release）の位置づけは変わりません。

### 🔧 配布版で「アップデート確認 / AI dreaming」が動くように（修正）
配布パッケージに **`nuko` CLI を同梱**するよう修正しました。
- 以前の配布版 .app は `nuko` を同梱しておらず、ダウンロードして使うと
  メニューの「🔄 アップデートを確認…」や「🤖 AIで学習を研ぎ直す」が
  `nuko CLI が見つかりません` で失敗していました。これを解消します。

### ⚙️ リリースの自動化
タグ push で署名・公証・Release 添付までを **GitHub Actions** で実行する仕組みに移行しました。

## 既知の制約
- **macOS のみ**／**arm64 単独**（Intel / universal は将来）。
- モデル（~166MB）は手動配置（初回自動 DL は将来）。内容は v0.1.3/v0.1.4 と同一です。
- 一部の固有名詞・長文はまだ誤変換あり（使うほど学習で寄ります）。

## インストール
手順は [`docs/MANUAL.md`](MANUAL.md) ／ [`docs/RELEASE.md`](RELEASE.md) 参照。

---
前バージョン: [v0.1.4](RELEASE_NOTES_v0.1.4.md) / [v0.1.3](RELEASE_NOTES_v0.1.3.md)
