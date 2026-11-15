//! dreaming (Layer 3) の **実行履歴** (透明性: いつ・何が・どう学習へ反映されたか)。
//!
//! `~/Library/Application Support/nuko-ime/dream_history.jsonl` に **1 行 1 実行**で追記する。
//! `nuko dream` が [`record`]、`nuko dream --history` や IME メニュー
//! 「dreaming の履歴を見る」から [`read_all`] / [`format_text`] で閲覧する。
//!
//! これは **ユーザーが手動で実行したときだけ積まれる** 監査ログ (自動大量生成ではない)。
//! スコア透明化 (#183) の設計思想に沿い、「何がどう変わったか」を後から辿れるようにする。
//! nuko-cli と nuko-macos 両方から使うため nuko-core に置く。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// この実行で **新規に反映された**選好 1 件 (読み→表層)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamAdded {
    /// 読み (かな)。
    pub reading: String,
    /// 反映後に優先される表層。
    pub prefer: String,
}

/// dreaming 1 実行の記録。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamRun {
    /// ISO8601 の時刻。
    pub time: String,
    /// プロバイダ ("ollama" / "openrouter" / "anthropic" / "mock")。
    pub provider: String,
    /// 使用モデル。
    pub model: String,
    /// 対象にした観察イベント総数。
    pub observations_total: usize,
    /// 要約した読みの数。
    pub readings_summarized: usize,
    /// AI が提案した件数。
    pub proposed: usize,
    /// **新規に反映された**選好 (読み→表層)。空なら学習は不変。
    pub added: Vec<DreamAdded>,
    /// 恒等 (読み==表層) で除外した件数。
    pub skipped_identity: usize,
    /// 既存と重複で除外した件数。
    pub skipped_existing: usize,
    /// `--apply` で実際に保存したか (false なら dry-run)。
    pub applied: bool,
}

/// 履歴ファイルのパス (`dream_history.jsonl`)。
#[must_use]
pub fn history_path(dir: &Path) -> PathBuf {
    dir.join("dream_history.jsonl")
}

/// 1 実行を追記する (失敗は握りつぶす — 履歴で本処理を止めない)。
pub fn record(dir: &Path, run: &DreamRun) {
    let _ = std::fs::create_dir_all(dir);
    let path = history_path(dir);
    if let Ok(line) = serde_json::to_string(run) {
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

/// 全履歴を読む (壊れた行は飛ばす。古い順)。
#[must_use]
pub fn read_all(dir: &Path) -> Vec<DreamRun> {
    let path = history_path(dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<DreamRun>(l).ok())
        .collect()
}

/// 履歴を人間可読テキストに整形する (新しい実行が上。最大 `limit` 件)。
///
/// IME メニュー「dreaming の履歴を見る」/ `nuko dream --history` 共用。
#[must_use]
pub fn format_text(runs: &[DreamRun], limit: usize) -> String {
    if runs.is_empty() {
        return "🧠 dreaming の履歴\n\nまだありません。\n\
                「🤖 AIで学習を研ぎ直す（BYOK）」を実行すると、\n\
                いつ・何が学習へ反映されたかがここに積まれます。"
            .to_string();
    }
    use std::fmt::Write;
    let mut s = String::from("🧠 dreaming の履歴（新しい順）\n\n");
    for run in runs.iter().rev().take(limit) {
        let _ = writeln!(s, "── {} ──", run.time);
        let _ = writeln!(s, "  {} / {}", run.provider, run.model);
        let _ = writeln!(
            s,
            "  観察 {} 件 → 読み {} 件要約 / AI 提案 {} 件",
            run.observations_total, run.readings_summarized, run.proposed
        );
        if run.added.is_empty() {
            s.push_str("  反映: なし（学習は変わっていません）\n");
        } else {
            let note = if run.applied {
                ""
            } else {
                "（未保存=dry-run）"
            };
            let _ = writeln!(s, "  反映: {} 件{note}", run.added.len());
            for a in &run.added {
                let _ = writeln!(s, "    ＋ {} → {}", a.reading, a.prefer);
            }
        }
        if run.skipped_identity > 0 || run.skipped_existing > 0 {
            let _ = writeln!(
                s,
                "  除外: 恒等 {} / 既存重複 {}",
                run.skipped_identity, run.skipped_existing
            );
        }
        s.push('\n');
    }
    s.push_str("──────────\n全文は dream_history.jsonl / `nuko dream --history` を参照。");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!(
            "nuko-dreamhist-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        d
    }

    fn sample(added: Vec<(&str, &str)>, applied: bool) -> DreamRun {
        DreamRun {
            time: "2026-11-15T10:00:00Z".to_string(),
            provider: "ollama".to_string(),
            model: "qwen2.5:14b".to_string(),
            observations_total: 70149,
            readings_summarized: 40,
            proposed: added.len(),
            added: added
                .into_iter()
                .map(|(r, p)| DreamAdded {
                    reading: r.to_string(),
                    prefer: p.to_string(),
                })
                .collect(),
            skipped_identity: 0,
            skipped_existing: 0,
            applied,
        }
    }

    #[test]
    fn record_then_read_roundtrips() {
        let dir = tmp_dir("rt");
        record(&dir, &sample(vec![("ほう", "方")], true));
        record(&dir, &sample(vec![], true));
        let all = read_all(&dir);
        assert_eq!(all.len(), 2, "2 実行が順に積まれる");
        assert_eq!(all[0].added.len(), 1);
        assert_eq!(all[1].added.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_all_missing_is_empty() {
        let dir = tmp_dir("missing");
        assert!(read_all(&dir).is_empty(), "ファイルが無ければ空");
    }

    #[test]
    fn format_text_empty_has_guidance() {
        let txt = format_text(&[], 20);
        assert!(txt.contains("まだありません"), "空のときは案内を出す");
    }

    #[test]
    fn format_text_shows_newest_first_and_changes() {
        let runs = vec![sample(vec![("ほう", "方")], true), sample(vec![], true)];
        let txt = format_text(&runs, 20);
        // 新しい順 = added なし(2件目)が先頭に出る
        let no_change_pos = txt.find("学習は変わっていません").unwrap();
        let change_pos = txt.find("ほう → 方").unwrap();
        assert!(no_change_pos < change_pos, "新しい実行が上に出る");
    }
}
