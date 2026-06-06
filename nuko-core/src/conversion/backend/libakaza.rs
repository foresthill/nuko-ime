//! libakaza ベースの変換バックエンド (Path B)
//!
//! `BigramWordViterbiEngine` を ぬこIME の `ConversionEngine` に橋渡しする。
//! モデルファイル (`unigram.model` / `bigram.model` / `skip_bigram.model` / `SKK-JISYO.akaza`)
//! の配置はこのモジュールのスコープ外 (Phase 2 のモデル生成パイプラインで配備する)。
//!
//! ## 設計方針
//!
//! - **起動時に 1 度だけ load**: `try_new` で失敗したら呼び出し側は静的辞書にフォールバック
//! - **エラーは握り潰す**: 詳細は spike-2 (`docs/spikes/libakaza-no-model-spike.md`) 参照。
//!   `BigramWordViterbiEngineBuilder::build()` の Err は内部の `io::Error::NotFound`
//!   が見えるだけで、どのファイルが無いかは取れないので、判定は `Err(_)` で十分。
//! - **segments flatten = 案 C**: Phase 1.2 では最良パスを連結した 1 候補を返す。
//!   文節別 API への拡張は Phase 1.3 (`docs/spikes/libakaza-api-survey.md` §4.4)。
//!
//! ## 注意
//!
//! libakaza 自体は MIT (Tokuhiro Matsuno, 2023) でぬこIME のライセンスと
//! 矛盾しないが、**akaza-default-model は SKK-JISYO.L (GPL-2.0) を含むため
//! 同梱・配布してはならない**。モデルは自前のパイプラインで生成すること。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use libakaza::config::EngineConfig;
use libakaza::engine::base::HenkanEngine;
use libakaza::engine::bigram_word_viterbi_engine::{
    BigramWordViterbiEngine, BigramWordViterbiEngineBuilder,
};
use libakaza::graph::reranking::ReRankingWeights;
use libakaza::kana_kanji::marisa_kana_kanji_dict::MarisaKanaKanjiDict;
use libakaza::lm::system_bigram::MarisaSystemBigramLM;
use libakaza::lm::system_unigram_lm::MarisaSystemUnigramLM;
use libakaza::user_side_data::user_data::UserData;

use crate::conversion::{Candidate, CandidateSource, Segment, SegmentedConversion};
use crate::error::{NukoError, Result};

type Engine =
    BigramWordViterbiEngine<MarisaSystemUnigramLM, MarisaSystemBigramLM, MarisaKanaKanjiDict>;

/// libakaza バックエンド
///
/// 統計的かな漢字変換 (Viterbi + bigram + skip-bigram) を提供する。
pub struct LibakazaBackend {
    engine: Engine,
    model_dir: PathBuf,
}

impl LibakazaBackend {
    /// モデルディレクトリを指定してバックエンドを構築する。
    ///
    /// `model_dir` は以下のファイルを含むディレクトリ:
    /// - `unigram.model` (必須)
    /// - `bigram.model` (必須)
    /// - `skip_bigram.model` (任意)
    /// - `SKK-JISYO.akaza` (必須)
    ///
    /// # エラー
    ///
    /// いずれかの必須ファイルが見つからない/破損している場合 `NukoError::Conversion` を返す。
    /// 呼び出し側はこのエラーを捕捉して既存の `DictionaryManager` にフォールバックすべき。
    pub fn try_new(model_dir: impl AsRef<Path>) -> Result<Self> {
        let model_dir = model_dir.as_ref().to_path_buf();
        let model_str = model_dir
            .to_str()
            .ok_or_else(|| {
                NukoError::Conversion(format!(
                    "モデルディレクトリのパスを文字列化できません: {}",
                    model_dir.display()
                ))
            })?
            .to_string();

        let config = EngineConfig {
            dicts: vec![],
            dict_cache: false,
            model: model_str,
            reranking_weights: ReRankingWeights::default(),
        };

        let mut builder = BigramWordViterbiEngineBuilder::new(config);
        builder.user_data(Arc::new(Mutex::new(UserData::default())));

        let engine = builder.build().map_err(|e| {
            NukoError::Conversion(format!(
                "libakaza モデルの読み込みに失敗 (model_dir={}): {e}",
                model_dir.display()
            ))
        })?;

        tracing::info!(model_dir = %model_dir.display(), "libakaza バックエンドを初期化しました");

        Ok(Self { engine, model_dir })
    }

    /// 読み (ひらがな) を変換し、最良パスを連結した 1 候補を返す。
    ///
    /// Phase 1.2 のスコープでは「最良パスを単純連結 → 1 候補」とする (案 C)。
    /// 文節別候補や複数候補は Phase 1.3 で対応する。
    pub fn convert(&self, reading: &str) -> Result<Vec<Candidate>> {
        if reading.is_empty() {
            return Ok(Vec::new());
        }

        let segments = self.engine.convert(reading, None).map_err(|e| {
            NukoError::Conversion(format!("libakaza 変換に失敗 (reading={reading}): {e}"))
        })?;

        if segments.is_empty() {
            return Ok(Vec::new());
        }

        // 最良パス: 各文節の先頭候補 (= cost 最小) を連結
        let surface: String = segments
            .iter()
            .filter_map(|seg| seg.first().map(|c| c.surface_with_dynamic()))
            .collect();

        if surface.is_empty() {
            return Ok(Vec::new());
        }

        // libakaza の cost: f32 (低=良) → nuko-core の score: i32 (高=良) へ反転
        let total_cost: f32 = segments
            .iter()
            .filter_map(|seg| seg.first().map(|c| c.cost))
            .sum();
        let score = cost_to_score(total_cost);

        Ok(vec![Candidate::new(surface, reading)
            .with_score(score)
            .with_source(CandidateSource::System)])
    }

    /// 読み (ひらがな) を文節別に変換し、`SegmentedConversion` を返す。
    ///
    /// Phase 1.3 Step 1 で追加 (案 B = 文節別 API)。
    /// 文節ごとに libakaza の全候補を保持するため、候補ウィンドウ表示
    /// (Step 2) や文節境界の編集 (Step 3) の基盤となる。
    ///
    /// 既存の [`convert`](Self::convert) は最良パスを 1 候補に flatten する
    /// 案 C のままで残しているので、`ConversionEngine::convert()`
    /// の最優先候補挿入は引き続き利用できる。
    ///
    /// # 戻り値
    ///
    /// - 入力が空 → 空の `SegmentedConversion`
    /// - libakaza が空文節列を返した → 空の `SegmentedConversion`
    /// - 正常 → 文節ごとに candidates が cost 昇順で並んだ `SegmentedConversion`
    pub fn convert_segmented(&self, reading: &str) -> Result<SegmentedConversion> {
        if reading.is_empty() {
            return Ok(SegmentedConversion::default());
        }

        let segments = self.engine.convert(reading, None).map_err(|e| {
            NukoError::Conversion(format!("libakaza 変換に失敗 (reading={reading}): {e}"))
        })?;

        if segments.is_empty() {
            return Ok(SegmentedConversion::default());
        }

        let mut out: Vec<Segment> = Vec::with_capacity(segments.len());
        for seg_candidates in segments {
            if seg_candidates.is_empty() {
                continue;
            }
            // 文節の読みは候補ごとに一致する想定だが、念のため先頭候補の yomi を採用
            let yomi = seg_candidates[0].yomi.clone();
            let candidates: Vec<Candidate> = seg_candidates
                .iter()
                .map(|c| {
                    Candidate::new(c.surface_with_dynamic(), &c.yomi)
                        .with_score(cost_to_score(c.cost))
                        .with_source(CandidateSource::System)
                })
                .collect();
            out.push(Segment::new(yomi, candidates));
        }

        Ok(SegmentedConversion::new(out))
    }

    /// モデルディレクトリへの参照
    #[must_use]
    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }
}

/// libakaza cost (f32, 低い=良い) を nuko-core score (i32, 高い=良い) に変換する。
///
/// 経験的なスケーリング: 既存の静的辞書スコア (100, 90, ...) と
/// 同じレンジに収めるため、cost に -100 を掛ける。
/// 厳密な値の最適化は Phase 1.3 以降で調整する。
fn cost_to_score(cost: f32) -> i32 {
    // i32 の境界に近い f32 リテラル (精度範囲内で表現可能な値)。
    // i32::MIN は 2 の冪なので f32 で正確、i32::MAX は最も近い f32 を採用。
    const I32_MAX_AS_F32: f32 = 2_147_483_520.0;
    const I32_MIN_AS_F32: f32 = -2_147_483_648.0;

    let scaled = -cost * 100.0;
    if !scaled.is_finite() {
        return 0;
    }
    if scaled >= I32_MAX_AS_F32 {
        i32::MAX
    } else if scaled <= I32_MIN_AS_F32 {
        i32::MIN
    } else {
        scaled as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_new_returns_err_when_model_dir_missing() {
        // spike-2 で確認した挙動: 存在しないモデルディレクトリでは必ず Err。
        // 呼び出し側はこの Err を捕捉して静的辞書にフォールバックする契約。
        let result =
            LibakazaBackend::try_new("/tmp/nuko-ime-test-no-such-model-dir-DOES-NOT-EXIST");
        assert!(
            result.is_err(),
            "存在しないモデルディレクトリは Err を返すべき"
        );
        if let Err(e) = result {
            let msg = e.to_string();
            assert!(
                msg.contains("libakaza モデルの読み込みに失敗"),
                "エラーメッセージに libakaza 失敗の context が含まれるべき: {msg}"
            );
        }
    }

    #[test]
    fn cost_to_score_inverts_sign() {
        // cost が低い = 良い → score が高い = 良い の方向性を確認
        assert!(cost_to_score(1.0) < cost_to_score(0.5));
        assert!(cost_to_score(0.5) < cost_to_score(0.1));
        assert_eq!(cost_to_score(0.0), 0);
    }

    #[test]
    fn cost_to_score_handles_extremes() {
        // 極端な値で panic しない
        let _ = cost_to_score(f32::INFINITY);
        let _ = cost_to_score(f32::NEG_INFINITY);
        let _ = cost_to_score(1e30);
        let _ = cost_to_score(-1e30);
    }
}
