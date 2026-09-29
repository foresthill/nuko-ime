//! Layer 3: AI dreaming の **土台** (provider 非依存・API キー不要で検証できる範囲)。
//!
//! nuko の学習は 4 層 (下から積む):
//! - Layer 0: 頻度学習 (`learning.json`)
//! - Layer 1: 観察ログ (`observations.jsonl`)
//! - Layer 2: 訂正抽出 (`corrections.toml`、[`super::extract_corrections`])
//! - **Layer 3: AI dreaming** (BYOK / ローカル推論。1 日 1 回、観察を「寝ている間に」
//!   整理して、頻度ヒューリスティックが取りこぼした選好を提案する)
//!
//! このモジュールは Layer 3 の **provider に依存しない核** だけを提供する:
//! 観察の要約 (`digest`) → プロンプト生成 (`build_prompt`) → 応答パース
//! (`parse_proposal`) → 既存選好への churn-free マージ (`merge_proposals`)。
//! いずれも **純粋関数** で、実際の LLM 呼び出し ([`DreamProvider`]) と切り離して
//! 単体テストできる。実プロバイダ (Anthropic BYOK / Ollama 等) は
//! [`DreamProvider`] を実装するだけでよく、ネットワークや API キーはこの核に漏れない。
//!
//! # 設計の意図 (2026-09、グローバル方針との整合)
//! - **「自動生成 + 高頻度 human review」を作らない**: dreaming は on-demand
//!   (`nuko dream`)。常駐で候補を溜め込まない。
//! - **churn-free**: [`merge_proposals`] は恒等・既存重複を除き、決定論的な順序で
//!   のみ追加する。同じ入力から同じ出力 (バイト単位で再現)。
//! - **上書きしない**: 提案は既存の観察由来選好を **消さない**。追加のみ。

use serde::{Deserialize, Serialize};

use super::correction::{CorrectionStore, Preference};
use super::observation::ObservationEvent;
use crate::error::Result;

/// dreaming に渡す観察の **決定論的な要約**。
///
/// 生の観察ログ (数千件) をそのままプロンプトに載せると大きすぎるため、
/// 読みごとに「どの表層で何回確定したか」を集計して圧縮する。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservationDigest {
    /// 集計元の総イベント数 (透明性)。
    pub total_events: usize,
    /// 読みごとの統計 (出現回数の多い順 → 同点は読み昇順で安定)。
    pub readings: Vec<ReadingStat>,
}

/// 1 つの読みに対する確定表層の内訳。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadingStat {
    /// 読み (かな)。
    pub reading: String,
    /// (表層, 回数)。回数の多い順 → 同点は表層昇順。
    pub surfaces: Vec<(String, u32)>,
}

impl ReadingStat {
    /// 全確定が読みそのまま (かな) = まだ変換されていない = dreaming の主対象。
    #[must_use]
    pub fn is_unconverted(&self) -> bool {
        self.surfaces.iter().all(|(s, _)| s == &self.reading)
    }

    /// 複数の表層で確定されている = 揺れている = 正準形の提案対象。
    #[must_use]
    pub fn is_inconsistent(&self) -> bool {
        self.surfaces.len() >= 2
    }
}

/// 観察ログを読みごとに集計して [`ObservationDigest`] を作る (純粋・決定論的)。
///
/// - `Commit` の (reading, surface) を数える。`Correction` の `corrected` も
///   1 票として数える (打ち直し = 強い意図)。
/// - `max_readings` 件までに絞る (出現回数の多い読み優先)。
#[must_use]
pub fn digest_observations(events: &[ObservationEvent], max_readings: usize) -> ObservationDigest {
    use std::collections::BTreeMap;

    // reading -> surface -> count。BTreeMap で決定論的順序。
    let mut tally: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for ev in events {
        let (reading, surface) = match ev {
            ObservationEvent::Commit {
                reading, surface, ..
            } => (reading, surface),
            ObservationEvent::Correction {
                reading, corrected, ..
            } => (reading, corrected),
        };
        if reading.is_empty() || surface.is_empty() {
            continue;
        }
        *tally
            .entry(reading.clone())
            .or_default()
            .entry(surface.clone())
            .or_default() += 1;
    }

    // 読みごとの総回数を出し、多い順 → 読み昇順 で並べる。
    let mut readings: Vec<ReadingStat> = tally
        .into_iter()
        .map(|(reading, surfaces)| {
            let mut surfaces: Vec<(String, u32)> = surfaces.into_iter().collect();
            // 回数降順 → 表層昇順 で安定。
            surfaces.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            ReadingStat { reading, surfaces }
        })
        .collect();
    readings.sort_by(|a, b| {
        let ca: u32 = a.surfaces.iter().map(|(_, c)| c).sum();
        let cb: u32 = b.surfaces.iter().map(|(_, c)| c).sum();
        cb.cmp(&ca).then_with(|| a.reading.cmp(&b.reading))
    });
    readings.truncate(max_readings);

    ObservationDigest {
        total_events: events.len(),
        readings,
    }
}

/// dreaming プロバイダに送るプロンプトを組み立てる (純粋・決定論的)。
///
/// LLM に「頻度学習が取りこぼした読み→表層の選好」を JSON で提案させる。
/// 応答形式を厳密に指定し、[`parse_proposal`] でパースできるようにする。
#[must_use]
pub fn build_prompt(digest: &ObservationDigest, existing: &CorrectionStore) -> String {
    use std::fmt::Write as _;

    let mut s = String::new();
    s.push_str(
        "あなたは日本語 IME の学習補助です。ユーザーの確定履歴を見て、\n\
         かな漢字変換の個人選好 (読み→表層) の候補を提案してください。\n\
         提案は次の場合に有用です:\n\
         - ある読みを毎回かなのまま確定している (漢字語の候補を提案)\n\
         - 同じ読みを複数の表層で確定していて揺れている (正準形を提案)\n\n\
         必ず次の JSON だけを出力してください (前後に説明文やコードフェンス不要):\n\
         {\"proposals\":[{\"reading\":\"...\",\"surface\":\"...\",\"reason\":\"...\"}]}\n\
         提案が無ければ {\"proposals\":[]} と出力してください。\n\n",
    );

    if !existing.is_empty() {
        s.push_str("## 既に学習済み (重複提案は不要)\n");
        for p in &existing.preferences {
            let _ = writeln!(s, "- {} → {}", p.reading, p.prefer);
        }
        s.push('\n');
    }

    let _ = writeln!(
        s,
        "## 確定履歴の要約 (総イベント {} 件)",
        digest.total_events
    );
    for r in &digest.readings {
        let inner: Vec<String> = r
            .surfaces
            .iter()
            .map(|(surf, c)| format!("{surf}×{c}"))
            .collect();
        let _ = writeln!(s, "- {} : {}", r.reading, inner.join(", "));
    }
    s
}

/// LLM が返す 1 件の提案。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedPreference {
    /// 読み (かな)。
    pub reading: String,
    /// 提案する表層。
    pub surface: String,
    /// 提案理由 (透明性用。ユーザーに見せる)。
    #[serde(default)]
    pub reason: String,
}

/// LLM が返す提案の集合 (`parse_proposal` の出力)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamProposal {
    /// 提案リスト。
    #[serde(default)]
    pub proposals: Vec<ProposedPreference>,
}

/// プロバイダの生応答 (JSON 文字列) を [`DreamProposal`] にパースする。
///
/// LLM は ```json フェンスや前置きを付けがちなので、最初の `{` から最後の `}`
/// までを切り出してからパースする (寛容パース)。
///
/// # エラー
/// JSON オブジェクトが見つからない / パースに失敗した場合。
pub fn parse_proposal(raw: &str) -> Result<DreamProposal> {
    use crate::error::NukoError;

    let start = raw.find('{');
    let end = raw.rfind('}');
    let json = match (start, end) {
        (Some(a), Some(b)) if b >= a => &raw[a..=b],
        _ => {
            return Err(NukoError::Conversion(format!(
                "dreaming 応答に JSON オブジェクトが見つかりません: {raw:?}"
            )))
        }
    };
    serde_json::from_str::<DreamProposal>(json).map_err(|e| {
        NukoError::Conversion(format!("dreaming 応答のパースに失敗: {e} (入力={json:?})"))
    })
}

/// [`merge_proposals`] の結果レポート (透明性用)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeReport {
    /// 新たに追加された選好。
    pub added: Vec<Preference>,
    /// 恒等 (読み == 表層) で除外した読み。
    pub skipped_identity: Vec<String>,
    /// 既に同じ選好があり変更不要だった読み。
    pub skipped_existing: Vec<String>,
}

/// AI 提案を既存の選好に **churn-free** でマージした新しいストアとレポートを返す。
///
/// - 恒等 (読み == 表層) は除外。
/// - 既に `reading → surface` が存在すれば何もしない (churn を出さない)。
/// - それ以外は重み `weight` の [`Preference`] を追加する (AI 由来、`seen` は 0)。
/// - 既存を **消さない・書き換えない**。追加のみ。
/// - 返るストアの `preferences` は読み昇順で安定。
///
/// この関数は書き込みをしない。呼び出し側が明示的に保存する (`nuko dream --apply`)。
#[must_use]
pub fn merge_proposals(
    store: &CorrectionStore,
    proposal: &DreamProposal,
    weight: u32,
) -> (CorrectionStore, MergeReport) {
    let mut report = MergeReport::default();
    let mut preferences = store.preferences.clone();

    for p in &proposal.proposals {
        if p.reading.is_empty() || p.surface.is_empty() {
            continue;
        }
        if p.reading == p.surface {
            report.skipped_identity.push(p.reading.clone());
            continue;
        }
        // 既に同じ reading→surface があれば churn を出さない。
        if preferences
            .iter()
            .any(|e| e.reading == p.reading && e.prefer == p.surface)
        {
            report.skipped_existing.push(p.reading.clone());
            continue;
        }
        let pref = Preference {
            reading: p.reading.clone(),
            prefer: p.surface.clone(),
            weight,
            seen: 0,
        };
        report.added.push(pref.clone());
        preferences.push(pref);
    }

    // 読み昇順 → 表層昇順 で安定させる (churn-free)。
    preferences.sort_by(|a, b| {
        a.reading
            .cmp(&b.reading)
            .then_with(|| a.prefer.cmp(&b.prefer))
    });
    (CorrectionStore { preferences }, report)
}

/// dreaming の推論プロバイダ。プロンプト文字列を受け取り生応答を返すだけ。
///
/// この trait を実装すれば任意のバックエンド (Anthropic BYOK / OpenAI / Ollama /
/// ローカル) を差し込める。ネットワーク・API キー・非同期はすべて実装側に閉じ、
/// nuko の核 ([`build_prompt`] / [`parse_proposal`] / [`merge_proposals`]) には
/// 漏れない。
pub trait DreamProvider {
    /// プロバイダ名 (ログ・表示用)。
    fn name(&self) -> &str;
    /// プロンプトを送り、生の応答テキスト (JSON を含む) を返す。
    ///
    /// # エラー
    /// 通信・認証・推論に失敗した場合。
    fn complete(&self, prompt: &str) -> Result<String>;
}

/// テスト・`--dry-run` 用の決定論的プロバイダ。
///
/// 固定の応答文字列を返すだけ。API キー不要で dreaming の end-to-end
/// (プロンプト → 応答 → パース → マージ) を検証できる。
pub struct MockProvider {
    /// 返す固定応答 (JSON)。
    pub canned: String,
}

impl MockProvider {
    /// 空の提案 (`{"proposals":[]}`) を返すモック。
    #[must_use]
    pub fn empty() -> Self {
        Self {
            canned: r#"{"proposals":[]}"#.to_string(),
        }
    }

    /// 指定の JSON を返すモック。
    #[must_use]
    pub fn with_response(json: impl Into<String>) -> Self {
        Self {
            canned: json.into(),
        }
    }
}

impl DreamProvider for MockProvider {
    // trait は `-> &str`。リテラルを返すため lifetime は不要だが署名は trait に従う。
    #[allow(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "mock"
    }

    fn complete(&self, _prompt: &str) -> Result<String> {
        Ok(self.canned.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(reading: &str, surface: &str) -> ObservationEvent {
        ObservationEvent::commit(reading, surface)
    }

    #[test]
    fn digest_tallies_and_orders_by_frequency() {
        let events = vec![
            commit("あす", "明日"),
            commit("あす", "明日"),
            commit("あす", "アス"),
            commit("いま", "今"),
        ];
        let d = digest_observations(&events, 10);
        assert_eq!(d.total_events, 4);
        // あす (3) が いま (1) より先。
        assert_eq!(d.readings[0].reading, "あす");
        assert_eq!(d.readings[1].reading, "いま");
        // あす の表層は 明日×2, アス×1 の順。
        assert_eq!(
            d.readings[0].surfaces,
            vec![("明日".to_string(), 2), ("アス".to_string(), 1)]
        );
        assert!(d.readings[0].is_inconsistent());
    }

    #[test]
    fn digest_is_deterministic() {
        let events = vec![commit("あ", "亜"), commit("い", "以"), commit("あ", "亜")];
        assert_eq!(
            digest_observations(&events, 10),
            digest_observations(&events, 10),
            "同じ入力から同じ digest (churn-free)"
        );
    }

    #[test]
    fn reading_stat_unconverted_detects_kana_only() {
        let stat = ReadingStat {
            reading: "ぬこ".into(),
            surfaces: vec![("ぬこ".into(), 5)],
        };
        assert!(stat.is_unconverted(), "かなのまま確定 = 未変換");
    }

    #[test]
    fn build_prompt_lists_readings_and_existing() {
        let events = vec![commit("ぬこ", "ぬこ"), commit("ぬこ", "ぬこ")];
        let digest = digest_observations(&events, 10);
        let mut existing = CorrectionStore::default();
        existing.preferences.push(Preference {
            reading: "あす".into(),
            prefer: "明日".into(),
            weight: 3,
            seen: 3,
        });
        let prompt = build_prompt(&digest, &existing);
        assert!(prompt.contains("ぬこ"), "読みが載る");
        assert!(prompt.contains("あす → 明日"), "学習済みが載る");
        assert!(prompt.contains("\"proposals\""), "出力形式を指示");
    }

    #[test]
    fn parse_proposal_tolerates_code_fence() {
        let raw = "```json\n{\"proposals\":[{\"reading\":\"ぬこ\",\"surface\":\"猫\",\"reason\":\"未変換\"}]}\n```";
        let p = parse_proposal(raw).expect("フェンス付きでもパースできる");
        assert_eq!(p.proposals.len(), 1);
        assert_eq!(p.proposals[0].surface, "猫");
    }

    #[test]
    fn parse_proposal_errors_on_no_json() {
        assert!(parse_proposal("提案はありません").is_err());
    }

    #[test]
    fn merge_proposals_adds_new_and_skips_identity_and_existing() {
        let mut store = CorrectionStore::default();
        store.preferences.push(Preference {
            reading: "あす".into(),
            prefer: "明日".into(),
            weight: 3,
            seen: 3,
        });
        let proposal = DreamProposal {
            proposals: vec![
                ProposedPreference {
                    reading: "ぬこ".into(),
                    surface: "猫".into(),
                    reason: "未変換".into(),
                },
                // 恒等 → 除外
                ProposedPreference {
                    reading: "は".into(),
                    surface: "は".into(),
                    reason: String::new(),
                },
                // 既存と同じ → churn 無し
                ProposedPreference {
                    reading: "あす".into(),
                    surface: "明日".into(),
                    reason: String::new(),
                },
            ],
        };
        let (merged, report) = merge_proposals(&store, &proposal, 2);
        assert_eq!(report.added.len(), 1);
        assert_eq!(report.added[0].reading, "ぬこ");
        assert_eq!(report.skipped_identity, vec!["は".to_string()]);
        assert_eq!(report.skipped_existing, vec!["あす".to_string()]);
        // 既存は消えない。追加後は読み昇順。
        let readings: Vec<&str> = merged
            .preferences
            .iter()
            .map(|p| p.reading.as_str())
            .collect();
        assert_eq!(readings, vec!["あす", "ぬこ"]);
    }

    #[test]
    fn merge_proposals_is_churn_free_when_reapplied() {
        let store = CorrectionStore::default();
        let proposal = DreamProposal {
            proposals: vec![ProposedPreference {
                reading: "ぬこ".into(),
                surface: "猫".into(),
                reason: String::new(),
            }],
        };
        let (once, _) = merge_proposals(&store, &proposal, 2);
        // 同じ提案を再適用しても増えない (既存扱い)。
        let (twice, report) = merge_proposals(&once, &proposal, 2);
        assert_eq!(once.preferences, twice.preferences, "再適用で churn 無し");
        assert!(report.added.is_empty());
    }

    #[test]
    fn mock_provider_end_to_end() {
        // プロンプト → mock 応答 → パース → マージ が API キー無しで通る。
        let events = vec![commit("ぬこ", "ぬこ"), commit("ぬこ", "ぬこ")];
        let digest = digest_observations(&events, 10);
        let store = CorrectionStore::default();
        let prompt = build_prompt(&digest, &store);

        let provider = MockProvider::with_response(
            r#"{"proposals":[{"reading":"ぬこ","surface":"猫","reason":"毎回かな"}]}"#,
        );
        let raw = provider.complete(&prompt).expect("mock は必ず成功");
        let proposal = parse_proposal(&raw).expect("パースできる");
        let (merged, report) = merge_proposals(&store, &proposal, 2);

        assert_eq!(report.added.len(), 1);
        assert_eq!(merged.preferences[0].prefer, "猫");
    }
}
