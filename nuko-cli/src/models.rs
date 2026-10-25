//! プロバイダのモデル一覧を実 API から取得する（設定画面のモデル選択用）。
//!
//! モデル ID を推測でハードコードせず、常に各社の公式 API から取得して最新を反映する。
//! - OpenRouter: `GET {base}/models`（公開・認証不要）。
//! - Anthropic: `GET https://api.anthropic.com/v1/models`（`x-api-key` が必要）。
//!
//! ネットワークは nuko-cli に閉じる（nuko-core には持ち込まない設計）。取得した id を
//! `nuko models <provider>` が 1 行ずつ標準出力に書き、設定画面がそれを読んで一覧化する。

use anyhow::{anyhow, Result};

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// OpenRouter の公開モデル一覧（id のみ、ソート済み・重複排除）。認証不要。
pub fn fetch_openrouter(base_url: &str) -> Result<Vec<String>> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let json: serde_json::Value = ureq::get(&url)
        .set("X-Title", "nuko-ime")
        .call()
        .map_err(|e| anyhow!("OpenRouter models 取得失敗: {e}"))?
        .into_json()
        .map_err(|e| anyhow!("OpenRouter models JSON 解析失敗: {e}"))?;
    Ok(extract_ids(&json))
}

/// Anthropic のモデル一覧（id のみ、ソート済み・重複排除）。`x-api-key` が必要。
pub fn fetch_anthropic(api_key: &str) -> Result<Vec<String>> {
    let resp = ureq::get("https://api.anthropic.com/v1/models")
        .set("x-api-key", api_key)
        .set("anthropic-version", ANTHROPIC_VERSION)
        .call();
    let json: serde_json::Value = match resp {
        Ok(r) => r
            .into_json()
            .map_err(|e| anyhow!("Anthropic models JSON 解析失敗: {e}"))?,
        // 4xx/5xx でも body に error.message が入るので読んで伝える。
        Err(ureq::Error::Status(code, r)) => {
            let j: serde_json::Value = r.into_json().unwrap_or_else(
                |_| serde_json::json!({ "error": { "message": format!("HTTP {code}") } }),
            );
            let msg = j
                .pointer("/error/message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("不明なエラー");
            return Err(anyhow!("Anthropic models 取得失敗: {msg}"));
        }
        Err(e) => return Err(anyhow!("Anthropic models 通信失敗: {e}")),
    };
    Ok(extract_ids(&json))
}

/// モデル id を「おすすめ順」に並べ替える（純粋・安定ソート）。
///
/// 400 件以上あると選べないので、日本語 dreaming に向く Claude 系を上位に出す。
/// 層の順は Claude Sonnet（コスト/品質の推奨）→ その他 Claude → 残り。
/// 各層内の相対順は保つ（呼び出し側がアルファベット順で渡す前提）。
#[must_use]
pub fn recommended_first(ids: Vec<String>) -> Vec<String> {
    fn tier(id: &str) -> u8 {
        let l = id.to_lowercase();
        if l.contains("claude") && l.contains("sonnet") {
            0
        } else if l.contains("claude") {
            1
        } else {
            2
        }
    }
    let mut v = ids;
    v.sort_by_key(|id| tier(id)); // sort_by_key は安定ソート → 層内の順は保たれる
    v
}

/// `{ "data": [ {"id": "..."}, ... ] }` から id を抜き出してソート・重複排除（純粋）。
fn extract_ids(json: &serde_json::Value) -> Vec<String> {
    let mut ids: Vec<String> = json
        .get("data")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    m.get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_ids_sorts_and_dedups() {
        let j = serde_json::json!({
            "data": [
                { "id": "zeta" },
                { "id": "alpha" },
                { "id": "alpha" },
                { "name": "no-id" },
            ]
        });
        assert_eq!(
            extract_ids(&j),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn extract_ids_handles_missing_data() {
        assert!(extract_ids(&serde_json::json!({})).is_empty());
        assert!(extract_ids(&serde_json::json!({ "data": "not-array" })).is_empty());
    }

    #[test]
    fn recommended_first_floats_claude_sonnet_to_top() {
        let input = vec![
            "amazon/nova-pro".to_string(),
            "anthropic/claude-opus-4".to_string(),
            "openai/gpt-5".to_string(),
            "anthropic/claude-sonnet-4.5".to_string(),
        ];
        let out = recommended_first(input);
        // 1) Sonnet → 2) 他 Claude → 3) 残り (層内は元の相対順)
        assert_eq!(
            out,
            vec![
                "anthropic/claude-sonnet-4.5".to_string(),
                "anthropic/claude-opus-4".to_string(),
                "amazon/nova-pro".to_string(),
                "openai/gpt-5".to_string(),
            ]
        );
    }

    #[test]
    fn recommended_first_is_stable_within_tier() {
        // 非 Claude 同士は元の順を保つ
        let input = vec!["z/model".to_string(), "a/model".to_string()];
        assert_eq!(
            recommended_first(input),
            vec!["z/model".to_string(), "a/model".to_string()]
        );
    }
}
