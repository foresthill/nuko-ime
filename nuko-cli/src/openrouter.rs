//! OpenRouter 版 [`DreamProvider`]（Layer 3 dreaming の実プロバイダ）。
//!
//! `POST {base_url}/chat/completions`（OpenAI 互換）で dreaming のプロンプトを送り、
//! 応答テキストを返す。リクエスト整形・応答解析は **純粋関数** でテストし、実通信だけ
//! ureq に委ねる（ネットワークは nuko-core に持ち込まない設計）。
//!
//! API キーは環境変数 `OPENROUTER_API_KEY`（設定画面が書き込む想定）。本体には平文保存しない。
//! モデル等は data ディレクトリの `ai.toml`（[`DreamConfig`]）。

use nuko_core::error::{NukoError, Result};
use nuko_core::learning::dreaming::DreamProvider;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// dreaming の BYOK 設定（`ai.toml`）。API キーは含めない（環境変数）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamConfig {
    /// dreaming を有効にするか（既定 OFF）。
    #[serde(default)]
    pub enabled: bool,
    /// 使用するモデル（例: `anthropic/claude-3.5-sonnet`）。未設定ならエラーで促す。
    #[serde(default)]
    pub model: Option<String>,
    /// OpenRouter API のベース URL（既定 `https://openrouter.ai/api/v1`）。
    #[serde(default = "default_base_url")]
    pub base_url: String,
}

fn default_base_url() -> String {
    "https://openrouter.ai/api/v1".to_string()
}

impl Default for DreamConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            base_url: default_base_url(),
        }
    }
}

impl DreamConfig {
    /// `ai.toml` から読む。ファイルが無ければ既定値（dreaming OFF）。
    ///
    /// # エラー
    /// ファイルはあるが TOML パースに失敗した場合。
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .map_err(|e| NukoError::Conversion(format!("ai.toml 読み込み失敗: {e}")))?;
        toml::from_str(&text).map_err(|e| NukoError::Conversion(format!("ai.toml パース失敗: {e}")))
    }
}

/// OpenRouter chat completions のリクエスト body を組み立てる（純粋）。
#[must_use]
pub fn build_request_body(model: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
    })
}

/// OpenRouter の応答 JSON から本文（`choices[0].message.content`）を取り出す（純粋）。
///
/// エラー応答（`{"error":{"message":...}}`）はエラーとして返す。
///
/// # エラー
/// 本文が見つからない / エラー応答だった場合。
pub fn parse_completion(resp: &serde_json::Value) -> Result<String> {
    if let Some(content) = resp
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
    {
        return Ok(content.to_string());
    }
    if let Some(msg) = resp
        .pointer("/error/message")
        .and_then(serde_json::Value::as_str)
    {
        return Err(NukoError::Conversion(format!("OpenRouter エラー: {msg}")));
    }
    Err(NukoError::Conversion(format!(
        "OpenRouter 応答を解釈できません: {resp}"
    )))
}

/// OpenRouter を叩く [`DreamProvider`] 実装。
pub struct OpenRouterProvider {
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenRouterProvider {
    /// キー・モデル・ベース URL から構築する。
    #[must_use]
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url.into(),
        }
    }
}

impl DreamProvider for OpenRouterProvider {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn complete(&self, prompt: &str) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = build_request_body(&self.model, prompt);
        let resp = ureq::post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .set("X-Title", "nuko-ime")
            .set("Content-Type", "application/json")
            .send_json(body);
        let json: serde_json::Value = match resp {
            Ok(r) => r.into_json().map_err(|e| {
                NukoError::Conversion(format!("OpenRouter 応答の JSON 解析失敗: {e}"))
            })?,
            // HTTP エラー (4xx/5xx) でも body に error.message が入るので読む。
            Err(ureq::Error::Status(code, r)) => {
                let j: serde_json::Value = r.into_json().unwrap_or_else(
                    |_| serde_json::json!({ "error": { "message": format!("HTTP {code}") } }),
                );
                return parse_completion(&j); // error.message をエラーとして返す
            }
            Err(e) => {
                return Err(NukoError::Conversion(format!("OpenRouter 通信失敗: {e}")));
            }
        };
        parse_completion(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_body_shape() {
        let body = build_request_body("test/model", "こんにちは");
        assert_eq!(body["model"], "test/model");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "こんにちは");
    }

    #[test]
    fn parse_completion_extracts_content() {
        let resp = serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": "{\"proposals\":[]}" } }]
        });
        assert_eq!(parse_completion(&resp).unwrap(), "{\"proposals\":[]}");
    }

    #[test]
    fn parse_completion_surfaces_error_message() {
        let resp = serde_json::json!({ "error": { "message": "invalid api key" } });
        let err = parse_completion(&resp).unwrap_err().to_string();
        assert!(err.contains("invalid api key"), "エラー文を伝える: {err}");
    }

    #[test]
    fn parse_completion_errors_on_unknown_shape() {
        assert!(parse_completion(&serde_json::json!({ "foo": 1 })).is_err());
    }

    #[test]
    fn config_default_is_disabled() {
        let c = DreamConfig::default();
        assert!(!c.enabled);
        assert!(c.model.is_none());
        assert_eq!(c.base_url, "https://openrouter.ai/api/v1");
    }

    #[test]
    fn config_missing_file_is_default() {
        let c = DreamConfig::load("/tmp/nuko-ime-no-such-ai-config.toml").unwrap();
        assert!(!c.enabled);
    }

    #[test]
    fn config_parses_toml() {
        let path = std::env::temp_dir().join(format!("nuko-ai-{}.toml", std::process::id()));
        std::fs::write(
            &path,
            "enabled = true\nmodel = \"anthropic/claude-3.5-sonnet\"\n",
        )
        .unwrap();
        let c = DreamConfig::load(&path).unwrap();
        assert!(c.enabled);
        assert_eq!(c.model.as_deref(), Some("anthropic/claude-3.5-sonnet"));
        let _ = std::fs::remove_file(&path);
    }
}
