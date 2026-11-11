# ぬこIME v0.1.10 (macOS プレビュー)

v0.1.9 からの変更は **AI dreaming (Layer 3) を Ollama で安定動作させる修正** 1 点です。
引き続き署名 + 公証済み（Developer ID）の pre-release です。

## 目玉

### 🤖 Ollama での dreaming が JSON エラーで落ちなくなった

自社サーバーの Ollama（ローカルモデル）で AI dreaming を回すと、モデルが
「JSON だけ出力」という指示を無視して**感想文**を返し、
`dreaming 応答に JSON オブジェクトが見つかりません` で失敗していた問題を修正しました。

- 原因: リクエストに構造化出力の指定が無く、応答フォーマットがモデル任せだった。
- 直し方: Ollama 経路に **OpenAI 互換 JSON モード**（`response_format: {"type":"json_object"}`）を
  付与し、サーバー側に有効な JSON だけを返させる。Ollama 公式が `/v1/chat/completions` で
  `response_format` / JSON mode をサポートしていることを確認済み
  （docs/api/openai-compatibility.mdx）。
- 影響範囲: **Ollama 経路のみ**。OpenRouter / Anthropic は従来どおり（回帰なし）。

> 補足: この修正は「dreaming が落ちない」ことを保証しますが、**提案の中身の質は別**です。
> ローカルの中型モデル（qwen2.5:14b 等）は提案 0 件になりやすく、日本語の選好提案の質なら
> Anthropic (BYOK) のほうが上です。dreaming に「かなのまま/揺れている読み」を優先的に
> 渡す改善は今後の課題。

## 既知の制約
- **macOS のみ / arm64 単独**。モデル（~166MB）は手動配置（内容は v0.1.3〜と同一）。
- 一部の固有名詞・長文はまだ誤変換あり（使うほど学習で寄ります）。

## インストール
手順は [`docs/MANUAL.md`](MANUAL.md) / [`docs/RELEASE.md`](RELEASE.md)。既存ユーザーは
アプリ内「🔄 アップデートを確認…」→「今すぐ更新」でも更新できます。
更新後は入力ソースを切り替える（ことえり等 → ぬこIME）と確実に反映されます。

---
前バージョン: [v0.1.9](RELEASE_NOTES_v0.1.9.md) / [v0.1.7](RELEASE_NOTES_v0.1.7.md)
