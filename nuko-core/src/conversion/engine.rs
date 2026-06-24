//! 変換エンジン本体

use std::path::Path;

#[cfg(feature = "akaza")]
use super::backend::LibakazaBackend;
#[cfg(feature = "akaza")]
use super::SegmentedConversion;
use super::{Candidate, CandidateList, CandidateSource, ConversionContext};
use crate::dictionary::DictionaryManager;
use crate::error::{NukoError, Result};
use crate::input::{to_halfwidth_katakana, to_katakana};
use crate::learning::LearningManager;

/// libakaza 由来候補に上乗せする優先度ブースト。
///
/// 静的辞書のスコアは概ね -100〜100 のレンジ、libakaza の cost_to_score は
/// 長文で大きな負値になる (実観測: 「きょうはいいてんき」 score=-8870)。
/// libakaza が動いていれば最優先で表示するため、想定最悪値を上回る大きな
/// ブーストを乗せる。BOOST=100_000 なら libakaza score=-50_000 の入力でも
/// 静的辞書 (max 100) を必ず上回る。
///
/// 実機検証 (2026-06-04, PROFILE=min):
/// - BOOST=1_000 では「わたしのなまえ」「きょうはいいてんき」がカタカナ候補
///   より下に来てしまい、Space を 3〜4 回押すまで漢字変換が出なかった
/// - BOOST=100_000 に引き上げて libakaza 候補が常に先頭に来るよう調整
///
/// Phase 1.3 で複数候補対応 (案 B) する際は、libakaza 出力内の相対順序は
/// 元の cost で決まるため、本 BOOST は全体の oxford 順序のみに影響する。
#[cfg(feature = "akaza")]
const LIBAKAZA_PRIORITY_BOOST: i32 = 100_000;

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

    /// 学習データの永続化パスを設定する。
    ///
    /// パスが指す JSON ファイルがあれば内容を読み込み、以降の `commit()` で
    /// 自動的に save される。プラットフォーム層が起動時に 1 度呼ぶ想定。
    ///
    /// ファイル不在時は新規作成扱い (= 空学習データから開始)。
    ///
    /// # エラー
    /// ファイルが存在するが JSON パースに失敗した場合。
    /// パスが存在しないこと自体はエラーにしない。
    pub fn set_learning_path(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        // ファイルがあれば load してエントリを引き継ぐ
        if path.exists() {
            self.learning = LearningManager::load(path)?;
            tracing::info!(
                path = %path.display(),
                entries = self.learning.entry_count(),
                "学習データを load"
            );
        } else {
            // 新規: ManagerにPathだけ設定して以降のsaveを有効化
            self.learning.set_path(path);
            tracing::info!(
                path = %path.display(),
                "学習データの永続化パスを設定 (ファイル不在、新規開始)"
            );
        }
        Ok(())
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

        // 1. 学習データから候補を取得 (surface 一致は重複扱い)
        let learned = self.learning.get_candidates(reading, context)?;
        for candidate in learned {
            if !candidates.iter().any(|c| c.surface == candidate.surface) {
                candidates.push(candidate.with_source(CandidateSource::Learned));
            }
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

        // 4. かなそのままも候補に追加 (既存と surface 一致なら重複扱いで skip)
        if !candidates.iter().any(|c| c.surface == reading) {
            candidates.push(
                Candidate::new(reading, reading)
                    .with_score(-100)
                    .with_source(CandidateSource::System),
            );
        }

        // 5. カタカナ変換も候補に追加
        let katakana = to_katakana(reading);
        if !candidates.iter().any(|c| c.surface == katakana) {
            candidates.push(
                Candidate::new(&katakana, reading)
                    .with_score(-90)
                    .with_source(CandidateSource::System),
            );
        }

        // 6. 半角カタカナも候補に追加
        let half_katakana = to_halfwidth_katakana(reading);
        if !candidates.iter().any(|c| c.surface == half_katakana) {
            candidates.push(
                Candidate::new(&half_katakana, reading)
                    .with_score(-95)
                    .with_source(CandidateSource::System),
            );
        }

        // スコア順にソート
        candidates.sort_by_score();

        Ok(candidates)
    }

    /// 文節別の変換結果を返す (libakaza バックエンド有効時のみ)
    ///
    /// Phase 1.3 Step 2 以降の候補ウィンドウ・文節境界編集の上流 API。
    /// 既存の `convert()` が返す flat な `CandidateList` とは別経路で、
    /// 文節ごとの全候補をそのまま保持した `SegmentedConversion` を返す。
    ///
    /// # 戻り値
    ///
    /// - `Ok(None)` — `akaza` feature 無効、libakaza モデル未 load、または空入力
    /// - `Ok(Some(SegmentedConversion))` — 文節列が得られた (空でないことを保証)
    /// - `Err(_)` — libakaza が変換中にエラーを返した (呼び出し側は静的辞書フォールバックを検討)
    ///
    /// 静的辞書フォールバックはこの API では行わない。プラットフォーム層は
    /// `None` を受け取った場合に既存の `convert()` ベースのフローへ切り替えること。
    #[cfg(feature = "akaza")]
    pub fn convert_segmented(&self, reading: &str) -> Result<Option<SegmentedConversion>> {
        if reading.is_empty() {
            return Ok(None);
        }
        let Some(backend) = &self.libakaza else {
            return Ok(None);
        };
        let segmented = backend.convert_segmented(reading)?;
        if segmented.is_empty() {
            Ok(None)
        } else {
            Ok(Some(segmented))
        }
    }

    /// 文節境界を伸縮して再変換する (Shift+→ / Shift+← 用、libakaza 有効時のみ)。
    ///
    /// 現在の `segmented` の各文節読みと `focused` から
    /// [`crate::conversion::extend_clause`] で `force_ranges` を計算し、libakaza に
    /// 強制境界で再変換させる。`extend_right = true` で focused 文節を右に伸ばし、
    /// `false` で左に縮める (左隣を伸ばす)。
    ///
    /// # 戻り値
    /// - `Ok(Some(_))` — 伸縮後の新しい `SegmentedConversion` (focused は維持)
    /// - `Ok(None)` — libakaza 無効 / 入力が空 / これ以上伸縮できない
    #[cfg(feature = "akaza")]
    pub fn resize_segment(
        &self,
        segmented: &SegmentedConversion,
        extend_right: bool,
    ) -> Result<Option<SegmentedConversion>> {
        let Some(backend) = &self.libakaza else {
            return Ok(None);
        };
        let readings: Vec<&str> = segmented
            .segments
            .iter()
            .map(|s| s.reading.as_str())
            .collect();
        if readings.is_empty() {
            return Ok(None);
        }

        let force = if extend_right {
            crate::conversion::extend_clause::extend_right(&readings, segmented.focused)
        } else {
            crate::conversion::extend_clause::extend_left(&readings, segmented.focused)
        };
        if force.is_empty() {
            return Ok(None);
        }

        let full_reading = readings.concat();
        let mut new_seg = backend.convert_segmented_forced(&full_reading, &force)?;
        if new_seg.is_empty() {
            return Ok(None);
        }

        // フォーカス位置を維持 (文節数が減るケースがあるのでクランプ)
        let new_focus = segmented.focused.min(new_seg.segments.len() - 1);
        new_seg.focus(new_focus);
        Ok(Some(new_seg))
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
        // 学習データの永続化が設定されていれば自動 save。失敗は warn にとどめ
        // commit 自体は成功扱い (= 学習はメモリには載った)。
        if self.learning.has_path() {
            if let Err(e) = self.learning.save() {
                tracing::warn!(error = %e, "学習データ save 失敗 (in-memory のみ保持)");
            }
        }
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
    fn convert_segmented_returns_none_when_libakaza_unavailable() {
        // libakaza load 失敗時は convert_segmented は静的辞書を一切触らず None を返す
        let engine = ConversionEngine::with_libakaza(
            "/tmp/nuko-ime-test-no-model-for-segmented-DOES-NOT-EXIST",
        )
        .unwrap();
        let result = engine.convert_segmented("にほん").unwrap();
        assert!(result.is_none(), "libakaza 不在時は None を返すべき");
    }

    #[cfg(feature = "akaza")]
    #[test]
    fn convert_segmented_returns_none_for_empty_input() {
        let engine = ConversionEngine::new().unwrap();
        let result = engine.convert_segmented("").unwrap();
        assert!(result.is_none(), "空入力は None");
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
