//! BYOK の AI 呼び出しログ (透明性: 何を送り・何が返り・いくらかかったか)。
//!
//! `~/Library/Application Support/nuko-ime/ai_log.jsonl` に **1 行 1 呼び出し**で追記する。
//! プロバイダ (nuko-cli の anthropic / openrouter) が `complete()` 内で [`record`] し、
//! `nuko ai-log` や IME メニュー「AI ログを見る」から [`read_all`] で閲覧する。
//! nuko-cli と nuko-macos 両方から使うため nuko-core に置く。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// AI 呼び出し 1 件の記録。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiLogEntry {
    /// ISO8601 の時刻。
    pub time: String,
    /// プロバイダ ("anthropic" / "openrouter")。
    pub provider: String,
    /// 指定したモデル (スラッグ)。
    pub model: String,
    /// 実際に使われたモデル (Jev Router 等でルーティングされた先)。
    #[serde(default)]
    pub resolved_model: Option<String>,
    /// 送信したプロンプト (全文)。
    pub prompt: String,
    /// 返ってきた本文 (全文)。
    pub response: String,
    /// 入力トークン数。
    #[serde(default)]
    pub input_tokens: u64,
    /// 出力トークン数。
    #[serde(default)]
    pub output_tokens: u64,
    /// コスト概算 (USD)。
    #[serde(default)]
    pub cost_usd: f64,
    /// 所要時間 (ミリ秒)。
    #[serde(default)]
    pub ms: u64,
}

fn data_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("nuko-ime"),
    )
}

/// ログファイルのパス (`ai_log.jsonl`)。
#[must_use]
pub fn log_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("ai_log.jsonl"))
}

/// 1 件を追記する (失敗は握りつぶす — ログで本処理を止めない)。
pub fn record(entry: &AiLogEntry) {
    let Some(path) = log_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(line) = serde_json::to_string(entry) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// 全ログを読む (壊れた行は飛ばす)。
#[must_use]
pub fn read_all() -> Vec<AiLogEntry> {
    let Some(path) = log_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<AiLogEntry>(l).ok())
        .collect()
}

/// Anthropic モデルのコスト概算 (USD)。価格は概算で将来変動する。
#[must_use]
#[allow(clippy::cast_precision_loss)] // トークン数は 2^52 未満で精度問題なし
pub fn estimate_anthropic_cost(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    // (入力 $/Mtok, 出力 $/Mtok)。モデル名の部分一致で判定。
    let (pin, pout) = if model.contains("haiku") {
        (0.80, 4.0)
    } else if model.contains("opus") {
        (15.0, 75.0)
    } else {
        (3.0, 15.0) // sonnet / 既定
    };
    (input_tokens as f64 * pin + output_tokens as f64 * pout) / 1_000_000.0
}

/// 現在時刻の ISO8601 文字列。
#[must_use]
pub fn now_iso() -> String {
    chrono::Local::now().to_rfc3339()
}
