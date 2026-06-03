//! 変換エンジン本体

#[cfg(feature = "akaza")]
use std::path::Path;

#[cfg(feature = "akaza")]
use super::backend::LibakazaBackend;
use super::{Candidate, CandidateList, CandidateSource, ConversionContext};
use crate::dictionary::DictionaryManager;
use crate::error::{NukoError, Result};
use crate::input::{to_halfwidth_katakana, to_katakana};
use crate::learning::LearningManager;

/// libakaza 由来候補に上乗せする優先度ブースト。
///
/// 静的辞書のスコアは概ね 0〜100 のレンジで、libakaza の cost_to_score は
/// 大きな負値になりがち。Phase 1.2 では「libakaza が動いていれば最優先」という
/// 単純な方針を取り、静的辞書を必ず上回るよう一律ブーストする。
/// 厳密なキャリブレーションは Phase 1.3 で行う。
#[cfg(feature = "akaza")]
const LIBAKAZA_PRIORITY_BOOST: i32 = 1_000;

/// 変換エンジン
pub struct ConversionEngine {
    /// 辞書マネージャー
    dictionary: DictionaryManager,
    /// 学習マネージャー
    learning: LearningManager,
    /// libakaza バックエンド (`akaza` feature 有効時のみ)
    #[cfg(feature = "akaza")]
    libakaza: Option<LibakazaBackend>,
}

impl ConversionEngine {
    /// 新しい変換エンジンを作成 (libakaza バックエンドなし)
    ///
    /// # エラー
    /// 辞書の読み込みに失敗した場合
    pub fn new() -> Result<Self> {
        Ok(Self {
            dictionary: DictionaryManager::new()?,
            learning: LearningManager::new()?,
            #[cfg(feature = "akaza")]
            libakaza: None,
        })
    }

    /// libakaza バックエンドを試行して変換エンジンを作成
    ///
    /// `model_dir` 配下の libakaza モデルファイル群を読み込もうとし、
    /// 失敗した場合は警告ログを出して libakaza なしの状態で起動する
    /// (= 静的辞書フォールバック)。エンジン自体の構築は常に成功する。
    ///
    /// # エラー
    /// 辞書マネージャー/学習マネージャーの初期化に失敗した場合のみ。
    /// libakaza 自体の load 失敗は内部で握り、Err にはしない。
    #[cfg(feature = "akaza")]
    pub fn with_libakaza(model_dir: impl AsRef<Path>) -> Result<Self> {
        let dictionary = DictionaryManager::new()?;
        let learning = LearningManager::new()?;
        let libakaza = match LibakazaBackend::try_new(model_dir.as_ref()) {
            Ok(backend) => Some(backend),
            Err(e) => {
                tracing::warn!(
                    model_dir = %model_dir.as_ref().display(),
                    error = %e,
                    "libakaza バックエンド初期化失敗。静的辞書フォールバックで起動"
                );
                None
            }
        };
        Ok(Self {
            dictionary,
            learning,
            libakaza,
        })
    }

    /// libakaza バックエンドが有効か (= load 成功して保持されているか)
    #[cfg(feature = "akaza")]
    #[must_use]
    pub fn has_libakaza(&self) -> bool {
        self.libakaza.is_some()
    }

    /// かなを漢字に変換
    ///
    /// # 引数
    /// * `reading` - 変換する読み（ひらがな）
    /// * `context` - 変換コンテキスト
    ///
    /// # 戻り値
    /// 変換候補のリスト
    pub fn convert(&self, reading: &str, context: &ConversionContext) -> Result<CandidateList> {
        if reading.is_empty() {
            return Err(NukoError::InvalidInput("空の入力です".to_string()));
        }

        let mut candidates = CandidateList::new();

        // 1. 学習データから候補を取得
        let learned = self.learning.get_candidates(reading, context)?;
        for candidate in learned {
            candidates.push(candidate.with_source(CandidateSource::Learned));
        }

        // 2. libakaza バックエンドが有効なら最優先で候補を追加
        #[cfg(feature = "akaza")]
        if let Some(backend) = &self.libakaza {
            match backend.convert(reading) {
                Ok(libakaza_candidates) => {
                    for mut candidate in libakaza_candidates {
                        candidate.score = candidate.score.saturating_add(LIBAKAZA_PRIORITY_BOOST);
                        if !candidates.iter().any(|c| c.surface == candidate.surface) {
                            candidates.push(candidate);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        reading = %reading,
                        error = %e,
                        "libakaza 変換失敗、静的辞書のみで継続"
                    );
                }
            }
        }

        // 3. 辞書から候補を取得
        let dict_candidates = self.dictionary.lookup(reading)?;
        for candidate in dict_candidates {
            // 重複を避ける
            if !candidates.iter().any(|c| c.surface == candidate.surface) {
                candidates.push(candidate);
            }
        }

        // 4. かなそのままも候補に追加
        candidates.push(
            Candidate::new(reading, reading)
                .with_score(-100)
                .with_source(CandidateSource::System),
        );

        // 5. カタカナ変換も候補に追加
        let katakana = to_katakana(reading);
        candidates.push(
            Candidate::new(&katakana, reading)
                .with_score(-90)
                .with_source(CandidateSource::System),
        );

        // 6. 半角カタカナも候補に追加
        let half_katakana = to_halfwidth_katakana(reading);
        candidates.push(
            Candidate::new(&half_katakana, reading)
                .with_score(-95)
                .with_source(CandidateSource::System),
        );

        // スコア順にソート
        candidates.sort_by_score();

        Ok(candidates)
    }

    /// 予測変換（入力途中で候補を提示）
    ///
    /// # 引数
    /// * `prefix` - 入力途中の読み（ひらがな）
    /// * `max_results` - 最大結果数
    ///
    /// # 戻り値
    /// (完全な読み, 変換候補) のリスト
    pub fn predict(&self, prefix: &str, max_results: usize) -> Result<Vec<(String, Candidate)>> {
        if prefix.is_empty() {
            return Ok(Vec::new());
        }

        let mut predictions = Vec::new();

        // 前方一致で辞書を検索
        let results = self.dictionary.prefix_search(prefix)?;

        for (reading, candidates) in results {
            for candidate in candidates {
                predictions.push((reading.clone(), candidate));
                if predictions.len() >= max_results {
                    return Ok(predictions);
                }
            }
        }

        Ok(predictions)
    }

    /// 変換を確定し、学習データを更新
    ///
    /// # 引数
    /// * `candidate` - 確定した候補
    /// * `context` - 変換コンテキスト
    pub fn commit(&mut self, candidate: &Candidate, context: &ConversionContext) -> Result<()> {
        self.learning.record(candidate, context)?;
        Ok(())
    }

    /// 学習データをクリア
    pub fn clear_learning_data(&mut self) -> Result<()> {
        self.learning.clear()
    }

    /// 辞書マネージャーへの参照を取得
    #[must_use]
    pub fn dictionary(&self) -> &DictionaryManager {
        &self.dictionary
    }

    /// 辞書マネージャーへの可変参照を取得
    pub fn dictionary_mut(&mut self) -> &mut DictionaryManager {
        &mut self.dictionary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let engine = ConversionEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_basic_convert() {
        let engine = ConversionEngine::new().unwrap();
        let context = ConversionContext::new();
        let candidates = engine.convert("にほん", &context).unwrap();

        assert!(!candidates.is_empty());
        // かなそのまま、カタカナの候補は必ず含まれる
        assert!(candidates.iter().any(|c| c.surface == "にほん"));
        assert!(candidates.iter().any(|c| c.surface == "ニホン"));
    }

    #[cfg(feature = "akaza")]
    #[test]
    fn with_libakaza_falls_back_when_model_dir_missing() {
        // spike-2 + LibakazaBackend で確認した契約:
        // モデル不在でも with_libakaza は Ok を返し、libakaza なしで起動する。
        let engine = ConversionEngine::with_libakaza(
            "/tmp/nuko-ime-test-no-model-dir-for-engine-wireup-DOES-NOT-EXIST",
        )
        .expect("エンジン構築は libakaza load 失敗でも成功すべき");
        assert!(
            !engine.has_libakaza(),
            "モデル不在時は libakaza バックエンドを保持しない"
        );
    }

    #[cfg(feature = "akaza")]
    #[test]
    fn convert_works_when_libakaza_unavailable() {
        // libakaza load に失敗してフォールバックした状態でも、
        // 既存の静的辞書フローが動作することを確認する。
        let engine = ConversionEngine::with_libakaza(
            "/tmp/nuko-ime-test-no-model-for-convert-fallback-DOES-NOT-EXIST",
        )
        .unwrap();
        let context = ConversionContext::new();
        let candidates = engine.convert("にほん", &context).unwrap();

        assert!(!candidates.is_empty());
        // 静的辞書とカタカナ展開は必ず返る
        assert!(candidates.iter().any(|c| c.surface == "にほん"));
        assert!(candidates.iter().any(|c| c.surface == "ニホン"));
    }
}
