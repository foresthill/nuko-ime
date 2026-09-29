//! Layer 1: 観察ログ (Observation Log)
//!
//! 「入力 → emit → (ユーザーが訂正)」のイベントを **ローカルの JSONL** に追記する。
//! Layer 2(訂正学習)/ Layer 3(AI dreaming)の唯一の入力源。
//!
//! ## プライバシー既定 (最重要)
//!
//! **`enabled = false`(既定)なら 1 バイトも書かない。** オプトインで初めて記録が始まる。
//! 保存先は完全にローカル。外部送信は一切しない(それは Layer 3 の別オプトイン)。
//!
//! 詳細設計: [`docs/LEARNING_ARCHITECTURE.md`](../../../docs/LEARNING_ARCHITECTURE.md)。

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{NukoError, Result};

/// 現在時刻を unix エポック秒で返す(既存 controller の debug_log と同じ方式、依存追加なし)。
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 観察イベント(JSONL の 1 行 = 1 イベント)。
///
/// `kind` タグで種別を区別する(人間可読・機械可読の両立)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObservationEvent {
    /// 変換を確定した。
    Commit {
        /// unix エポック秒
        ts: u64,
        /// 読み(かな)
        reading: String,
        /// 確定した表層
        surface: String,
        /// 提示されていた候補(任意・空なら省略)
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        candidates: Vec<String>,
        /// 確定した候補の index(任意)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        picked: Option<usize>,
    },
    /// 確定直後に訂正した(emit → 直した)。Layer 2 の高価値シグナル。
    Correction {
        /// unix エポック秒
        ts: u64,
        /// 読み(かな)
        reading: String,
        /// 最初に emit されていた表層
        emitted: String,
        /// 訂正後の表層
        corrected: String,
    },
}

impl ObservationEvent {
    /// commit イベントを現在時刻で作る。
    #[must_use]
    pub fn commit(reading: impl Into<String>, surface: impl Into<String>) -> Self {
        Self::Commit {
            ts: now_unix(),
            reading: reading.into(),
            surface: surface.into(),
            candidates: Vec::new(),
            picked: None,
        }
    }

    /// commit イベント(候補リスト付き)を現在時刻で作る。
    #[must_use]
    pub fn commit_with_candidates(
        reading: impl Into<String>,
        surface: impl Into<String>,
        candidates: Vec<String>,
        picked: Option<usize>,
    ) -> Self {
        Self::Commit {
            ts: now_unix(),
            reading: reading.into(),
            surface: surface.into(),
            candidates,
            picked,
        }
    }

    /// correction イベントを現在時刻で作る。
    #[must_use]
    pub fn correction(
        reading: impl Into<String>,
        emitted: impl Into<String>,
        corrected: impl Into<String>,
    ) -> Self {
        Self::Correction {
            ts: now_unix(),
            reading: reading.into(),
            emitted: emitted.into(),
            corrected: corrected.into(),
        }
    }

    /// このイベントの読み(かな)。`Commit` / `Correction` いずれも読みを持つ。
    #[must_use]
    pub fn reading(&self) -> &str {
        match self {
            ObservationEvent::Commit { reading, .. }
            | ObservationEvent::Correction { reading, .. } => reading,
        }
    }
}

/// 観察ログ本体。追記のみ・ローカルのみ。
///
/// `enabled = false` なら [`record`](Self::record) は完全な no-op(ファイルも作らない)。
pub struct ObservationLog {
    enabled: bool,
    path: Option<PathBuf>,
}

impl ObservationLog {
    /// オプトイン状態と保存先を指定して作る。
    ///
    /// `enabled = false` なら記録しない(既定・プライバシー保護)。
    #[must_use]
    pub fn new(enabled: bool, path: impl Into<PathBuf>) -> Self {
        Self {
            enabled,
            path: Some(path.into()),
        }
    }

    /// 常に無効な観察ログ(記録先なし)。テストや opt-out 時に使う。
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            path: None,
        }
    }

    /// 記録が有効か。
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.path.is_some()
    }

    /// イベントを 1 行追記する。
    ///
    /// **`enabled = false` なら何もしない(ファイルも作らない)。** これが既定。
    pub fn record(&self, event: &ObservationEvent) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let Some(path) = &self.path else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let line = serde_json::to_string(event)
            .map_err(|e| NukoError::Learning(format!("観察イベントの直列化に失敗: {e}")))?;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{line}")?;
        Ok(())
    }

    /// 記録済みイベントを全件読む(壊れた行はスキップ)。
    ///
    /// ファイルが無ければ空を返す。
    pub fn read_all(&self) -> Result<Vec<ObservationEvent>> {
        let Some(path) = &self.path else {
            return Ok(Vec::new());
        };
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            // 壊れた行は無視して読み進める(ログの頑健性)
            if let Ok(ev) = serde_json::from_str::<ObservationEvent>(&line) {
                out.push(ev);
            }
        }
        Ok(out)
    }

    /// 記録件数(透明性 UI 用)。
    pub fn count(&self) -> Result<usize> {
        Ok(self.read_all()?.len())
    }

    /// 記録を全消去する(透明性: ユーザーがいつでも消せる)。
    pub fn clear(&self) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// 与えたイベント列でログを **丸ごと書き直す** (透明性: 特定の学習を忘れる用)。
    ///
    /// `enabled` に関わらず実行する (ユーザー明示操作)。空なら記録は残さない
    /// (追記実装と揃えて、全消去時はファイル自体を消す)。
    pub fn rewrite(&self, events: &[ObservationEvent]) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if events.is_empty() {
            if path.exists() {
                std::fs::remove_file(path)?;
            }
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut buf = String::new();
        for ev in events {
            let line = serde_json::to_string(ev)
                .map_err(|e| NukoError::Learning(format!("観察イベントの直列化に失敗: {e}")))?;
            buf.push_str(&line);
            buf.push('\n');
        }
        std::fs::write(path, buf)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "nuko-obs-test-{}-{}.jsonl",
            std::process::id(),
            name
        ));
        p
    }

    #[test]
    fn reading_returns_kana_for_both_variants() {
        assert_eq!(
            ObservationEvent::commit("いって", "言って").reading(),
            "いって"
        );
        assert_eq!(
            ObservationEvent::correction("いって", "言って", "行って").reading(),
            "いって"
        );
    }

    /// ★ forget の土台: 特定の読みを除いて書き直せる (再学習で戻らない durable 忘却)。
    #[test]
    fn rewrite_replaces_log_contents() {
        let path = tmp_path("rewrite");
        let _ = std::fs::remove_file(&path);
        let log = ObservationLog::new(true, &path);
        log.record(&ObservationEvent::commit("いって", "言って"))
            .unwrap();
        log.record(&ObservationEvent::commit("あす", "明日"))
            .unwrap();
        log.record(&ObservationEvent::commit("いって", "言って"))
            .unwrap();

        // いって を除去して書き直す。
        let kept: Vec<_> = log
            .read_all()
            .unwrap()
            .into_iter()
            .filter(|ev| ev.reading() != "いって")
            .collect();
        log.rewrite(&kept).unwrap();

        let after = log.read_all().unwrap();
        assert_eq!(after.len(), 1, "★ いって が消えて あす だけ残る");
        assert_eq!(after[0].reading(), "あす");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rewrite_empty_removes_file() {
        let path = tmp_path("rewrite-empty");
        let log = ObservationLog::new(true, &path);
        log.record(&ObservationEvent::commit("あ", "亜")).unwrap();
        assert!(path.exists());
        log.rewrite(&[]).unwrap();
        assert!(!path.exists(), "★ 空で書き直すとファイルごと消える");
    }

    /// ★ 既定(disabled)では 1 バイトも書かない = プライバシー既定の担保。
    #[test]
    fn disabled_writes_nothing() {
        let path = tmp_path("disabled");
        let _ = std::fs::remove_file(&path);
        let log = ObservationLog::new(/*enabled=*/ false, &path);
        assert!(!log.is_enabled());
        log.record(&ObservationEvent::commit("はんい", "範囲"))
            .unwrap();
        // ファイルすら作られない
        assert!(!path.exists(), "★ disabled なのにファイルが作られた");
        assert_eq!(log.read_all().unwrap().len(), 0);
    }

    #[test]
    fn enabled_appends_and_reads_in_order() {
        let path = tmp_path("append");
        let _ = std::fs::remove_file(&path);
        let log = ObservationLog::new(true, &path);
        assert!(log.is_enabled());

        log.record(&ObservationEvent::commit("はんい", "範囲"))
            .unwrap();
        log.record(&ObservationEvent::correction(
            "こんばん",
            "今晩",
            "こんばん",
        ))
        .unwrap();

        let events = log.read_all().unwrap();
        assert_eq!(events.len(), 2);
        match &events[0] {
            ObservationEvent::Commit {
                reading, surface, ..
            } => {
                assert_eq!(reading, "はんい");
                assert_eq!(surface, "範囲");
            }
            ev @ ObservationEvent::Correction { .. } => panic!("1件目が Commit でない: {ev:?}"),
        }
        match &events[1] {
            ObservationEvent::Correction {
                emitted, corrected, ..
            } => {
                assert_eq!(emitted, "今晩");
                assert_eq!(corrected, "こんばん");
            }
            ev @ ObservationEvent::Commit { .. } => panic!("2件目が Correction でない: {ev:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clear_removes_all() {
        let path = tmp_path("clear");
        let _ = std::fs::remove_file(&path);
        let log = ObservationLog::new(true, &path);
        log.record(&ObservationEvent::commit("あ", "亜")).unwrap();
        assert_eq!(log.count().unwrap(), 1);
        log.clear().unwrap();
        assert_eq!(log.count().unwrap(), 0);
        assert!(!path.exists());
    }

    /// JSONL は 1 行 1 イベントで人間可読(透明性)。
    #[test]
    fn jsonl_is_one_line_per_event() {
        let path = tmp_path("jsonl");
        let _ = std::fs::remove_file(&path);
        let log = ObservationLog::new(true, &path);
        log.record(&ObservationEvent::commit("に", "二")).unwrap();
        log.record(&ObservationEvent::commit("さん", "三")).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.lines().count(), 2);
        assert!(content.contains("\"kind\":\"commit\""));
        let _ = std::fs::remove_file(&path);
    }
}
