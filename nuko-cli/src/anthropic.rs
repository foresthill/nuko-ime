//! Anthropic (Claude) 直 API 版 [`DreamProvider`]（Layer 3 dreaming の実プロバイダ）。
//!
//! `POST {base_url}/v1/messages`（Anthropic Messages API）で dreaming のプロンプトを送り、
//! 応答テキストを返す。リクエスト整形・応答解析は **純粋関数** でテストし、実通信だけ
//! ureq に委ねる（ネットワークは nuko-core に持ち込まない設計。openrouter.rs と同方針）。
//!
//! OpenRouter 経由でも Claude は使えるが、本モジュールは **Anthropic の API キーを直接**
//! 使いたい場合（OpenRouter を挟まない）用。API キーは環境変数 `ANTHROPIC_API_KEY`。
//! 本体には平文保存しない。
//!
//! Anthropic API の形（OpenAI 互換の OpenRouter と異なる点）:
//! - ヘッダ: `x-api-key: <key>` / `anthropic-version: 2023-06-01`
//! - body に `max_tokens` が **必須**
//! - 応答本文は `content[0].text`（`choices[0].message.content` ではない）
//! - エラーは `{"type":"error","error":{"message":...}}`

use nuko_core::error::{NukoError, Result};
use nuko_core::learning::dreaming::DreamProvider;
use std::borrow::Cow;

/// Anthropic Messages API の既定ベース URL。
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";

/// Anthropic が要求する API バージョンヘッダ。
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// dreaming 応答に十分な既定の最大トークン数（提案 JSON は小さい）。
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Anthropic Messages API のリクエスト body を組み立てる（純粋）。
#[must_use]
pub fn build_request_body(model: &str, prompt: &str, max_tokens: u32) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [{ "role": "user", "content": prompt }],
    })
}

/// Anthropic の応答 JSON から本文（`content[0].text`）を取り出す（純粋）。
///
/// エラー応答（`{"error":{"message":...}}`）はエラーとして返す。
///
/// # エラー
/// 本文が見つからない / エラー応答だった場合。
pub fn parse_completion(resp: &serde_json::Value) -> Result<String> {
    // 正常: content は配列。text ブロックを連結して返す（通常は 1 個）。
    if let Some(blocks) = resp.get("content").and_then(serde_json::Value::as_array) {
        let text: String = blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(serde_json::Value::as_str))
            .collect();
        if !text.is_empty() {
            return Ok(text);
        }
    }
    if let Some(msg) = resp
        .pointer("/error/message")
        .and_then(serde_json::Value::as_str)
    {
        return Err(NukoError::Conversion(format!("Anthropic エラー: {msg}")));
    }
    Err(NukoError::Conversion(format!(
        "Anthropic 応答を解釈できません: {resp}"
    )))
}

/// Anthropic を叩く [`DreamProvider`] 実装。
pub struct AnthropicProvider {
    api_key: String,
    model: String,
    base_url: String,
    max_tokens: u32,
}

impl AnthropicProvider {
    /// キー・モデルから構築する（ベース URL / max_tokens は既定値）。
    #[must_use]
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            max_tokens: DEFAULT_MAX_TOKENS,
        }
    }
}

impl DreamProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn complete(&self, prompt: &str) -> Result<String> {
        let started = std::time::Instant::now();
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = build_request_body(&self.model, prompt, self.max_tokens);
        let resp = ureq::post(&url)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", ANTHROPIC_VERSION)
            .set("Content-Type", "application/json")
            .send_json(body);
        let json: serde_json::Value = match resp {
            Ok(r) => r.into_json().map_err(|e| {
                NukoError::Conversion(format!("Anthropic 応答の JSON 解析失敗: {e}"))
            })?,
            // HTTP エラー (4xx/5xx) でも body に error.message が入るので読む。
            Err(ureq::Error::Status(code, r)) => {
                let j: serde_json::Value = r.into_json().unwrap_or_else(
                    |_| serde_json::json!({ "error": { "message": format!("HTTP {code}") } }),
                );
                return parse_completion(&j); // error.message をエラーとして返す
            }
            Err(e) => {
                return Err(NukoError::Conversion(format!("Anthropic 通信失敗: {e}")));
            }
        };
        let text = parse_completion(&json)?;
        // AI ログ記録 (透明性: 送信/応答/トークン/コスト)。
        let u = |p: &str| {
            json.pointer(p)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        };
        let (it, ot) = (u("/usage/input_tokens"), u("/usage/output_tokens"));
        nuko_core::ai_log::record(&nuko_core::ai_log::AiLogEntry {
            time: nuko_core::ai_log::now_iso(),
            provider: "anthropic".to_string(),
            model: self.model.clone(),
            resolved_model: None,
            prompt: prompt.to_string(),
            response: text.clone(),
            input_tokens: it,
            output_tokens: ot,
            cost_usd: nuko_core::ai_log::estimate_anthropic_cost(&self.model, it, ot),
            ms: started.elapsed().as_millis() as u64,
        });
        Ok(text)
    }
}

/// `--model` / `ai.toml` のモデル名が未指定のとき使う既定モデルの案内文（エラー用）。
#[must_use]
pub fn model_hint() -> Cow<'static, str> {
    Cow::Borrowed("例: claude-sonnet-5-5 / claude-haiku-5-5 / claude-opus-5-5")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_body_shape() {
        let body = build_request_body("claude-sonnet-5-5", "こんにちは", 2048);
        assert_eq!(body["model"], "claude-sonnet-5-5");
        assert_eq!(body["max_tokens"], 2048);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "こんにちは");
    }

    #[test]
    fn parse_completion_extracts_text() {
        let resp = serde_json::json!({
            "content": [{ "type": "text", "text": "{\"proposals\":[]}" }]
        });
        assert_eq!(parse_completion(&resp).unwrap(), "{\"proposals\":[]}");
    }

    #[test]
    fn parse_completion_joins_multiple_text_blocks() {
        let resp = serde_json::json!({
            "content": [
                { "type": "text", "text": "{\"proposals\":" },
                { "type": "text", "text": "[]}" }
            ]
        });
        assert_eq!(parse_completion(&resp).unwrap(), "{\"proposals\":[]}");
    }

    #[test]
    fn parse_completion_surfaces_error_message() {
        let resp = serde_json::json!({
            "type": "error",
            "error": { "type": "authentication_error", "message": "invalid x-api-key" }
        });
        let err = parse_completion(&resp).unwrap_err().to_string();
        assert!(err.contains("invalid x-api-key"), "エラー文を伝える: {err}");
    }

    #[test]
    fn parse_completion_errors_on_unknown_shape() {
        assert!(parse_completion(&serde_json::json!({ "foo": 1 })).is_err());
    }

    #[test]
    fn provider_name_is_anthropic() {
        let p = AnthropicProvider::new("sk-ant-xxx", "claude-sonnet-5-5");
        assert_eq!(p.name(), "anthropic");
    }
}
