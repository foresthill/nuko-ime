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
use crate::learning::{CorrectionStore, LearningManager};

/// 自然分割の境界 (`nat_bounds` = 0 と `total` を含む累積オフセット) に対し、
/// `hint` 読みが 1 文節になるよう強制境界を作った `force_ranges` を返す。
///
/// hint が自然境界の run を **ちょうど覆う** (= 隣接する複数の自然文節を統合するだけ) か。
///
/// - 両端 (`range.start` / `range.end`) が自然境界 (`nat_bounds`) に一致し、
/// - 内側に自然境界が 1 つ以上ある (= 現状 2 文節以上に割れている)
///
/// ときだけ `true`。これは「隣接文節をまとめる」= **merge-only** な安全操作を意味する。
///
/// 片端でも境界からズレる hint は自然文節を **分割** する。短い共通語 (いか→以下 等) が
/// 「体格→た以下く」「生活→背以下つ」のように無関係語を破壊するため、分割は採用しない
/// (2026-09-29 回帰修正: ユーザー報告「急に精度が糞化した」の根治)。
#[cfg(feature = "akaza")]
fn hint_merges_whole_segments(nat_bounds: &[usize], range: &std::ops::Range<usize>) -> bool {
    let starts_at_bound = nat_bounds.contains(&range.start);
    let ends_at_bound = nat_bounds.contains(&range.end);
    let has_internal_bound = nat_bounds.iter().any(|&b| b > range.start && b < range.end);
    starts_at_bound && ends_at_bound && has_internal_bound
}

/// hint の内側にある自然境界は除去し、hint の両端を境界に加える。hint の外側は
/// 自然境界を保つ。結果は 0..total を隙間なく覆う。
#[cfg(feature = "akaza")]
fn force_ranges_with_hint(
    nat_bounds: &[usize],
    total: usize,
    hint: &std::ops::Range<usize>,
) -> Vec<std::ops::Range<usize>> {
    let mut b: Vec<usize> = nat_bounds
        .iter()
        .copied()
        .filter(|&x| x <= hint.start || x >= hint.end) // hint 内側の境界を除去
        .collect();
    b.push(0);
    b.push(total);
    b.push(hint.start);
    b.push(hint.end);
    b.retain(|&x| x <= total);
    b.sort_unstable();
    b.dedup();
    b.windows(2).map(|w| w[0]..w[1]).collect()
}

/// ひらがな / カタカナ / 半角カタカナ / 長音符か (辞書の「語」判定用)。
fn is_kana_char(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{309F}'    // ひらがな
        | '\u{30A0}'..='\u{30FF}'  // カタカナ (長音符 ー = U+30FC を含む)
        | '\u{FF61}'..='\u{FF9F}'  // 半角カタカナ・記号
    )
}

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

/// 単語登録 (ユーザー辞書) 候補に乗せる優先度ブースト。
///
/// 辞書候補は素の score が -100 前後で、libakaza (BOOST 100_000) の下に沈む。
/// 「ユーザーが明示的に登録した語」は **最優先で 1 位に出す** べきなので、
/// libakaza より大きいブーストを乗せる。
const USER_DICT_BOOST: i32 = 200_000;

/// 「ん + な行(な/に/ぬ/ね/の)」を「ん + 母音(あ/い/う/え/お)」に置換した **代替読み** を生成する。
///
/// ローマ字の「nn+母音」は `ん+な行` に固定されるため (`jikannarutoki→じかんなるとき`)、
/// `ん+母音` の語 (時間**あ**るとき / 千**円**=せん**え**ん / 繁**栄**=はん**え**い) が出せない。
/// 一方、単純に nn+母音→ん+母音 にすると **残念(ざんねん)/案内(あんない)** を壊す
/// (`sennen`(千円) と `zannen`(残念) は同じ nn+e で欲しい結果が逆で、位置ルールでは区別不能)。
///
/// そこで **両方の読みを libakaza に変換させ、言語モデルにスコアで選ばせる** (ことえりの
/// 辞書判断を nuko の libakaza で再現)。本関数は `[原文, 代替1, ...]` を返す。原文は常に先頭。
/// ん の直後が な行 の各位置につき 1 箇所だけ置換した代替を作る (組合せ爆発を避け上限あり)。
#[must_use]
pub fn nn_alternate_readings(reading: &str) -> Vec<String> {
    const NA_ROW: [(char, char); 5] = [
        ('な', 'あ'),
        ('に', 'い'),
        ('ぬ', 'う'),
        ('ね', 'え'),
        ('の', 'お'),
    ];
    const MAX_ALTERNATES: usize = 3; // 原文 + 最大 3 代替

    let chars: Vec<char> = reading.chars().collect();
    let mut out = vec![reading.to_string()];
    for i in 1..chars.len() {
        if out.len() > MAX_ALTERNATES {
            break;
        }
        if chars[i - 1] != 'ん' {
            continue;
        }
        if let Some(&(_, vowel)) = NA_ROW.iter().find(|(na, _)| *na == chars[i]) {
            let mut alt = chars.clone();
            alt[i] = vowel;
            let s: String = alt.into_iter().collect();
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

/// 文節別変換結果に **文節ごとの** 個人選好 (訂正学習) の bias を適用し並べ替える。
///
/// `convert()` (flat) と同じ bias を segmented 経路にも効かせるための関数。これが無いと
/// 「まつや→松谷」等の学習が複数文節の文の中で無視される。corrections が空なら無変化。
// 呼び出し元 convert_segmented は akaza-gated なので、非 akaza の lib ビルドでは未使用
// (テストからは使う)。
#[cfg_attr(not(feature = "akaza"), allow(dead_code))]
pub fn apply_segment_corrections(
    segmented: &mut super::SegmentedConversion,
    corrections: &crate::learning::CorrectionStore,
) {
    if corrections.is_empty() {
        return;
    }
    let mut any_changed = false;
    for seg in &mut segmented.segments {
        let seg_reading = seg.reading.clone();
        let mut changed = false;
        // (1) 読み完全一致の候補を bias (例: 文節「まつや」→ 松谷)
        for c in &mut seg.candidates {
            let bias = corrections.bias(&seg_reading, &c.surface);
            if bias != 0 {
                c.score = c.score.saturating_add(bias);
                changed = true;
            }
        }
        // (1') 読み完全一致の学習表層が候補に無ければ **注入** する。
        //      境界学習で 1 文節に切り直した語 (例: 文節「ざびさん」候補に「ザビさん」が
        //      無い) や、辞書/libakaza に無い語をユーザー学習で出せるようにする。
        //      flat の convert() と同じ扱い (2026-09 分割境界学習)。
        if let Some((surface, bias)) = corrections.preferred(&seg_reading) {
            if !seg.candidates.iter().any(|c| c.surface == surface) {
                seg.candidates.push(
                    Candidate::new(surface, &seg_reading)
                        .with_score(bias)
                        .with_source(CandidateSource::User),
                );
                changed = true;
            }
        }
        // (2) 敬称/助詞込みで切られた文節を bias / 注入
        //     (例: 文節「まつやさん」→ 松谷さん、「ざびさん」→ ザビさん。
        //      libakaza は文中で名前を「さん」込みに切る)
        for (target, bias) in corrections.suffix_targets(&seg_reading) {
            if let Some(c) = seg.candidates.iter_mut().find(|c| c.surface == target) {
                c.score = c.score.saturating_add(bias);
                changed = true;
            } else {
                // 目標表層 (訂正表層 + 接尾かな) が候補に無ければ注入する。
                seg.candidates.push(
                    Candidate::new(&target, &seg_reading)
                        .with_score(bias)
                        .with_source(CandidateSource::User),
                );
                changed = true;
            }
        }
        if changed {
            seg.candidates.sort_by_key(|c| std::cmp::Reverse(c.score));
            seg.select(0); // 並べ替え後の先頭 (最良) を選択に戻す
            any_changed = true;
        }
    }
    segmented.corrections_applied = any_changed;
}

/// 変換エンジン
pub struct ConversionEngine {
    /// 辞書マネージャー
    dictionary: DictionaryManager,
    /// 学習マネージャー
    learning: LearningManager,
    /// libakaza バックエンド (`akaza` feature 有効時のみ)
    #[cfg(feature = "akaza")]
    libakaza: Option<LibakazaBackend>,
    /// Layer 2 訂正学習の個人選好 (変換時に候補へ bias)。空なら変換は無変化。
    corrections: CorrectionStore,
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
            corrections: CorrectionStore::default(),
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
            corrections: CorrectionStore::default(),
        })
    }

    /// この読みちょうどに whole-reading の訂正選好があるか。
    ///
    /// `true` のとき、その読みは segmented だと誤分割で訂正が効かないことがあるため
    /// (例: さわれる→`さ`+`割れる`)、呼び出し側は flat 変換を優先すべき。flat なら
    /// [`Self::convert`] が訂正表層を注入+bias して 1 位に出す。
    #[must_use]
    pub fn has_whole_correction(&self, reading: &str) -> bool {
        self.corrections.preferred(reading).is_some()
    }

    /// この読みちょうどに静的辞書の **語** (かな以外＝漢字等を含む表層) があるか。
    ///
    /// libakaza は複合語を分割することがある (例: よしゅく→`よ`+`しゅく` で 予祝 が
    /// 出ない、ざびさん→`ざ`+`びさん`)。静的辞書に読み全体の語 (よしゅく→予祝) が
    /// あるなら、呼び出し側は flat 変換を優先して **1 語として** 出すべき。
    /// flat の [`Self::convert`] は静的辞書を候補に含める。
    #[must_use]
    pub fn has_dict_word(&self, reading: &str) -> bool {
        self.dictionary
            .lookup(reading)
            .map(|cands| {
                cands.iter().any(|c| {
                    // 読みそのもの・全カナは「語」とみなさない (漢字等を含むものだけ)
                    c.surface != reading && c.surface.chars().any(|ch| !is_kana_char(ch))
                })
            })
            .unwrap_or(false)
    }

    /// Layer 2 訂正選好を設定する(変換時に該当候補へ bias)。
    pub fn set_corrections(&mut self, corrections: CorrectionStore) {
        tracing::info!(count = corrections.len(), "訂正選好 (Layer 2) を設定");
        self.corrections = corrections;
    }

    /// `corrections.toml` から選好を load する(無ければ空)。
    pub fn load_corrections(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.corrections = CorrectionStore::load(path)?;
        tracing::info!(count = self.corrections.len(), "訂正選好 (Layer 2) を load");
        Ok(())
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

        // 2. libakaza バックエンドが有効なら最優先で候補を追加。
        //    「nn 曖昧さ」救済: 原文＋代替読み (ん+な行 → ん+母音) の両方を変換して
        //    マージ。libakaza の言語モデルが正しい方を高スコアにする
        //    (じかんあるとき > じかんなるとき、残念(ざんねん) > ざんえん)。
        #[cfg(feature = "akaza")]
        if let Some(backend) = &self.libakaza {
            for alt in nn_alternate_readings(reading) {
                match backend.convert(&alt) {
                    Ok(libakaza_candidates) => {
                        for mut candidate in libakaza_candidates {
                            candidate.score =
                                candidate.score.saturating_add(LIBAKAZA_PRIORITY_BOOST);
                            if !candidates.iter().any(|c| c.surface == candidate.surface) {
                                candidates.push(candidate);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            reading = %alt,
                            error = %e,
                            "libakaza 変換失敗、静的辞書のみで継続"
                        );
                    }
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

        // 単語登録 (ユーザー辞書) の候補は最優先で 1 位に出す。
        for c in candidates.iter_mut() {
            if c.source == CandidateSource::User {
                c.score = c.score.saturating_add(USER_DICT_BOOST);
            }
        }

        // Layer 2: 個人選好 (訂正学習) の bias をソート前に加算する。
        // 選好が無い候補は bias=0 = 無変化なので、corrections が空なら挙動は完全に不変。
        if !self.corrections.is_empty() {
            for c in candidates.iter_mut() {
                let bias = self.corrections.bias(reading, &c.surface);
                if bias != 0 {
                    c.score = c.score.saturating_add(bias);
                }
            }
            // 学習した表層が候補に無ければ **注入** する。libakaza/辞書に無い語
            // (例: さわれる→触れる、可能形で辞書に無い) でも学習した選好を出せる
            // (2026-09 ユーザー報告: 分割される語の学習が効かない)。
            if let Some((surface, bias)) = self.corrections.preferred(reading) {
                if !candidates.iter().any(|c| c.surface == surface) {
                    candidates.push(
                        Candidate::new(surface, reading)
                            .with_score(bias)
                            .with_source(CandidateSource::User),
                    );
                }
            }
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
        let mut segmented = backend.convert_segmented(reading)?;
        if segmented.is_empty() {
            return Ok(None);
        }
        // 分割境界の学習: 学習した読み (訂正) を **分割境界のヒント** にして再分割する。
        // libakaza は名前を誤分割することがあり (例: ざびさん→[ざ][びさん])、
        // 文節読みが「ざび」にならないと「ざび→ザビ」の学習が効かない。学習した読みが
        // 文中に現れ、自然分割で 1 文節になっていなければ、そこを強制境界にする
        // (2026-09 ユーザー要望「文節の切り方の学習」)。
        if let Some(forced) = self.resegment_with_hints(backend, reading, &segmented)? {
            segmented = forced;
        }
        // Layer 2: **文節ごとに** 個人選好 (訂正学習) の bias を適用して並べ替える。
        // flat の convert() では適用済みだが convert_segmented では未適用だったため、
        // 「まつや→松谷」等の学習が **複数文節の文の中では効かない** バグがあった
        // (2026-09 ユーザー報告: 単体「まつや」は松谷、「まつやさんと…」は松也)。
        apply_segment_corrections(&mut segmented, &self.corrections);
        Ok(Some(segmented))
    }

    /// 学習した読み (訂正) を分割境界のヒントにして再分割する。
    ///
    /// `natural` の自然分割に対し、訂正読み (2 文字以上) が `reading` の部分文字列として
    /// 現れ、かつ**単一の自然文節になっていない**（誤分割で跨いでいる）ものを探し、
    /// 最長のものをその読みが 1 文節になるよう強制境界を作って再変換する。
    /// ヒントが無い / 自然分割と同じなら `None`（再変換しない）。
    #[cfg(feature = "akaza")]
    fn resegment_with_hints(
        &self,
        backend: &LibakazaBackend,
        reading: &str,
        natural: &SegmentedConversion,
    ) -> Result<Option<SegmentedConversion>> {
        if self.corrections.is_empty() {
            return Ok(None);
        }
        // 自然分割の境界 (0 と total を含む累積バイトオフセット)。
        let mut nat_bounds = vec![0usize];
        let mut off = 0usize;
        for s in &natural.segments {
            off += s.reading.len();
            nat_bounds.push(off);
        }
        let total = reading.len();

        // ヒント候補: 訂正読み (2 文字以上) で reading に部分一致し、**自然境界の run を
        // ちょうど覆う** (= 隣接文節を統合するだけ) もの。最長 (バイト長) を採用する。
        //
        // ★ merge-only 制約 (2026-09-29 回帰修正): 自然文節を **分割** するヒントは採用しない。
        //   「いか→以下」のような短い共通語が「体格(たいかく)→た以下く」「生活→背以下つ」と
        //   無関係語を破壊した (ユーザー報告「急に精度が糞化した」)。統合 (ざびさん) は安全だが
        //   分割は危険なので、両端が自然境界に一致するヒントだけに限定する。
        let mut best: Option<std::ops::Range<usize>> = None;
        for p in &self.corrections.preferences {
            let r = p.reading.as_str();
            if r.chars().count() < 2 {
                continue;
            }
            let mut from = 0usize;
            while let Some(pos) = reading[from..].find(r) {
                let start = from + pos;
                let range = start..start + r.len();
                if hint_merges_whole_segments(&nat_bounds, &range)
                    && best.as_ref().map_or(true, |b| range.len() > b.len())
                {
                    best = Some(range.clone());
                }
                from = start + r.len();
            }
        }
        let Some(hint) = best else {
            return Ok(None);
        };

        let forced = force_ranges_with_hint(&nat_bounds, total, &hint);
        // 自然分割と同じなら再変換不要。
        let nat_ranges: Vec<std::ops::Range<usize>> =
            nat_bounds.windows(2).map(|w| w[0]..w[1]).collect();
        if forced == nat_ranges {
            return Ok(None);
        }
        let re = backend.convert_segmented_forced(reading, &forced)?;
        if re.is_empty() {
            return Ok(None);
        }
        Ok(Some(re))
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

    /// ★ 学習 (corrections) が **文節ごと** に効き、複数文節の文の中でも順位が直る。
    /// (まつや→松谷 を学習したら「まつやさんと…」の中の まつや 文節でも松谷が1位)
    #[test]
    fn segment_corrections_reorder_within_sentence() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::learning::{extract_corrections, ObservationEvent};

        // まつや→松谷 を学習 (既定でない候補を選んだ = 1回で選好化)
        let store = extract_corrections(
            &[ObservationEvent::commit_with_candidates(
                "まつや",
                "松谷",
                vec!["松也".into(), "松谷".into()],
                Some(1),
            )],
            2,
        );

        // 文「まつやさんと」= [まつや(既定 松也), さんと] を模した segmented
        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        let mut segmented = SegmentedConversion::new(vec![
            Segment::new(
                "まつや",
                vec![cand("松也", "まつや", 0), cand("松谷", "まつや", -10)],
            ),
            Segment::new("さんと", vec![cand("さんと", "さんと", 0)]),
        ]);

        // 適用前: まつや文節の先頭は 松也
        assert_eq!(segmented.segments[0].surface(), Some("松也"));

        apply_segment_corrections(&mut segmented, &store);

        // 適用後: 学習により 松谷 が先頭に
        assert_eq!(
            segmented.segments[0].surface(),
            Some("松谷"),
            "★ 文節の中でも学習した松谷が1位"
        );
        // 他文節は不変
        assert_eq!(segmented.segments[1].surface(), Some("さんと"));
    }

    /// ★ 敬称込みで切られた文節でも学習が効く (回帰: #87 が実機で効かなかった件)。
    /// libakaza は文中で「まつやさんと…」を文節読み「まつやさん」に敬称込みで切る。
    /// 学習は「まつや→松谷」なので読み完全一致では当たらない。suffix_targets 経由で
    /// 「松谷さん」(= 松谷 + さん) を押し上げる。
    #[test]
    fn segment_corrections_apply_to_honorific_suffixed_bunsetsu() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::learning::{extract_corrections, ObservationEvent};

        // まつや→松谷 を学習 (「まつや」単体の読みで)
        let store = extract_corrections(
            &[ObservationEvent::commit_with_candidates(
                "まつや",
                "松谷",
                vec!["松也".into(), "松谷".into()],
                Some(1),
            )],
            2,
        );

        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        // 実機 libakaza の分割を模す: 文節読み「まつやさん」候補は敬称込み表層
        let mut segmented = SegmentedConversion::new(vec![
            Segment::new(
                "まつやさん",
                vec![
                    cand("松也さん", "まつやさん", 0),
                    cand("松屋さん", "まつやさん", -5),
                    cand("松谷さん", "まつやさん", -10),
                ],
            ),
            Segment::new("と", vec![cand("と", "と", 0)]),
        ]);

        assert_eq!(
            segmented.segments[0].surface(),
            Some("松也さん"),
            "適用前は既定 松也さん"
        );

        apply_segment_corrections(&mut segmented, &store);

        assert_eq!(
            segmented.segments[0].surface(),
            Some("松谷さん"),
            "★ 敬称込み文節でも学習した松谷(さん)が1位"
        );
        assert_eq!(segmented.segments[1].surface(), Some("と"));
        assert!(
            segmented.corrections_applied,
            "★ 学習が効いたら corrections_applied が立つ (nn 曖昧語で flat より優先する判定に使う)"
        );
    }

    /// ★ 分割境界学習: 敬称込み文節の目標表層が **候補に無くても注入** される。
    /// (2026-09 ざびさん: 境界を [ざびさん] に切り直しても候補は kana ばかりで
    ///  「ザビさん」が無い → suffix_targets 注入で 1 位に出す)
    #[test]
    fn segment_corrections_inject_honorific_target_missing_from_candidates() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::learning::{extract_corrections, ObservationEvent};

        // ざび→ザビ を学習
        let store = extract_corrections(
            &[ObservationEvent::commit_with_candidates(
                "ざび",
                "ザビ",
                vec!["ざび".into(), "ザビ".into()],
                Some(1),
            )],
            2,
        );
        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        // 文節読み「ざびさん」候補は kana/カナのみ (「ザビさん」は無い)
        let mut segmented = SegmentedConversion::new(vec![Segment::new(
            "ざびさん",
            vec![
                cand("ざびさん", "ざびさん", 0),
                cand("ザビサン", "ざびさん", -5),
            ],
        )]);

        apply_segment_corrections(&mut segmented, &store);

        assert_eq!(
            segmented.segments[0].surface(),
            Some("ザビさん"),
            "★ 候補に無い ザビさん を注入して 1 位に出す"
        );
        assert!(segmented.corrections_applied);
    }

    /// ★ 学習にマッチしない文節では corrections_applied は立たない。
    /// (nn 曖昧語で「訂正が無ければ flat (ん+母音 代替) を使う」判定の土台)
    #[test]
    fn segment_corrections_flag_false_when_no_match() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::learning::{extract_corrections, ObservationEvent};

        let store = extract_corrections(
            &[ObservationEvent::commit_with_candidates(
                "まつや",
                "松谷",
                vec!["松也".into(), "松谷".into()],
                Some(1),
            )],
            2,
        );
        let cand = |s: &str, r: &str| {
            Candidate::new(s, r)
                .with_score(0)
                .with_source(CandidateSource::System)
        };
        // 学習と無関係な文「じかん|なるとき」
        let mut segmented = SegmentedConversion::new(vec![
            Segment::new("じかん", vec![cand("時間", "じかん")]),
            Segment::new("なるとき", vec![cand("成るとき", "なるとき")]),
        ]);
        apply_segment_corrections(&mut segmented, &store);
        assert!(
            !segmented.corrections_applied,
            "★ マッチしなければ corrections_applied は false (→ nn は flat 経路へ)"
        );
    }

    /// ★ nn 曖昧さ: ん+な行 の位置に「ん+母音」の代替読みを生成する。
    #[test]
    fn nn_alternate_readings_generates_vowel_variants() {
        // じかんなるとき → じかんあるとき (んな→んあ) も候補に
        let alts = nn_alternate_readings("じかんなるとき");
        assert!(
            alts.contains(&"じかんなるとき".to_string()),
            "★ 原文は必ず含む"
        );
        assert!(
            alts.contains(&"じかんあるとき".to_string()),
            "★ ん+な→ん+あ の代替を生成"
        );
        // せんねん → せんえん (千円) の代替
        let alts2 = nn_alternate_readings("せんねん");
        assert!(alts2.contains(&"せんえん".to_string()), "★ ん+ね→ん+え");
        // ん+な行 が無ければ原文のみ
        assert_eq!(
            nn_alternate_readings("こんにちは").len(),
            2,
            "★ んに→んい も1つ出る"
        );
        assert_eq!(
            nn_alternate_readings("あいうえお"),
            vec!["あいうえお".to_string()],
            "★ ん が無ければ代替なし"
        );
    }

    /// ★ 単語登録した語は変換で 1 位に来る (USER_DICT_BOOST)。
    #[test]
    fn user_dict_candidate_ranks_first() {
        use crate::dictionary::UserEntry;
        let mut engine = ConversionEngine::new().unwrap();
        engine
            .dictionary_mut()
            .user_dictionary_mut()
            .add(UserEntry::new("駒谷", "こまや"))
            .unwrap();
        let ctx = ConversionContext::new();
        let candidates = engine.convert("こまや", &ctx).unwrap();
        assert_eq!(
            candidates.iter().next().unwrap().surface,
            "駒谷",
            "★ 単語登録が最優先で 1 位"
        );
    }

    /// Layer 2: 訂正選好の bias で候補順が変わり、選好を外せば元に戻る (非破壊)。
    #[test]
    fn corrections_bias_reorders_and_is_reversible() {
        use crate::learning::{extract_corrections, CorrectionStore, ObservationEvent};

        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();

        // ベースライン: カタカナ「ニホン」は通常先頭ではない
        let base = engine.convert("にほん", &ctx).unwrap();
        let base_first = base.selected().unwrap().surface.clone();
        assert!(base.iter().any(|c| c.surface == "ニホン"));
        assert_ne!(base_first, "ニホン", "前提: 素では ニホン は先頭でない");

        // 「にほん→ニホン」を 2 回コミットした観察から選好を抽出して設定
        let events = vec![
            ObservationEvent::commit("にほん", "ニホン"),
            ObservationEvent::commit("にほん", "ニホン"),
        ];
        engine.set_corrections(extract_corrections(&events, 2));

        let biased = engine.convert("にほん", &ctx).unwrap();
        assert_eq!(
            biased.selected().unwrap().surface,
            "ニホン",
            "★ 選好が先頭に来る"
        );

        // 選好を外すと元の並びに戻る (非破壊・churn-free)
        engine.set_corrections(CorrectionStore::default());
        let restored = engine.convert("にほん", &ctx).unwrap();
        assert_eq!(
            restored.selected().unwrap().surface,
            base_first,
            "★ 選好を外せば元通り"
        );
    }

    /// ★ Layer 2: 学習した表層が候補に無くても **注入** される (さわれる→触れる 型)。
    /// 辞書/libakaza に無い語 (可能形など) でも、ユーザー学習で 1 位に出せる。
    /// (2026-09 ユーザー報告: 分割される語の学習が効かない)
    #[test]
    fn correction_injects_surface_missing_from_candidates() {
        use crate::learning::{extract_corrections, ObservationEvent};

        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();

        // 素の にほん 候補に架空語は無い
        let base = engine.convert("にほん", &ctx).unwrap();
        assert!(
            !base.iter().any(|c| c.surface == "架空ZZ"),
            "前提: 架空ZZ は候補に無い"
        );
        assert!(!engine.has_whole_correction("にほん"), "前提: 学習なし");

        // にほん→架空ZZ を学習 (辞書に無い表層でも)
        let events = vec![
            ObservationEvent::commit("にほん", "架空ZZ"),
            ObservationEvent::commit("にほん", "架空ZZ"),
        ];
        engine.set_corrections(extract_corrections(&events, 2));

        let after = engine.convert("にほん", &ctx).unwrap();
        assert_eq!(
            after.selected().unwrap().surface,
            "架空ZZ",
            "★ 候補に無い学習表層を注入して 1 位に"
        );
        assert!(engine.has_whole_correction("にほん"), "★ 学習ありは true");
        assert!(
            !engine.has_whole_correction("べつのよみ"),
            "★ 学習の無い読みは false"
        );
    }

    /// ★ 分割境界の学習: 自然境界にヒントの境界を強制する純粋ロジック。
    #[cfg(feature = "akaza")]
    #[test]
    fn force_ranges_with_hint_merges_crossing_boundary() {
        // 自然 [ざ(0..3)][びさん(3..12)][おおさか(12..24)] を hint ざび(0..6) で割る。
        // 内側の 3 を除去、6 を追加 → [0..6(ざび)][6..12(さん)][12..24]
        let nb = vec![0, 3, 12, 24];
        assert_eq!(
            force_ranges_with_hint(&nb, 24, &(0..6)),
            vec![0..6, 6..12, 12..24]
        );
    }

    /// ★ hint が既に単一自然文節なら結果は自然境界と同じ (再変換不要の判定に使う)。
    #[cfg(feature = "akaza")]
    #[test]
    fn force_ranges_with_hint_noop_when_already_segment() {
        let nb = vec![0, 6, 12]; // [0..6][6..12]
        assert_eq!(force_ranges_with_hint(&nb, 12, &(0..6)), vec![0..6, 6..12]);
    }

    /// ★★ 回帰の核 (2026-09-29): 境界学習は **merge-only**。
    /// 隣接文節をまとめる hint だけ採用し、自然文節を分割する hint は拒否する。
    #[cfg(feature = "akaza")]
    #[test]
    fn hint_merges_whole_segments_accepts_merge_rejects_split() {
        // ざびさん: 自然 [ざ(0..3)][びさん(3..12)]。hint ざびさん(0..12) は
        // 両端が境界に一致し内側に境界(3)がある → 統合 = 採用。
        let nb_zabi = vec![0, 3, 12];
        assert!(
            hint_merges_whole_segments(&nb_zabi, &(0..12)),
            "隣接文節の統合 (ざびさん) は採用"
        );

        // 体格(たいかく): 自然 [たい(0..6)][かく(6..12)]。学習 いか→以下 の
        // ヒント範囲は 3..9 で、両端とも自然境界(0,6,12)に無い → 分割 = 拒否。
        // これを採用すると「体格→た以下く」に壊れる (回帰)。
        let nb_taikaku = vec![0, 6, 12];
        assert!(
            !hint_merges_whole_segments(&nb_taikaku, &(3..9)),
            "★ 自然文節を分割する いか(3..9) は拒否 (体格を壊さない)"
        );

        // よいかな: 自然 [よ(0..3)][いか(3..9)][な(9..12)]。よい→良い を学習しても
        // ヒント よい(0..6) は end=6 が境界に無い (いか の途中) → 拒否。
        // よいかな は「分割」が要るため境界学習では自動修正できない (要フルモデル/手動)。
        let nb_yoikana = vec![0, 3, 9, 12];
        assert!(
            !hint_merges_whole_segments(&nb_yoikana, &(0..6)),
            "★ よい(0..6) は いか の途中で終わる → 拒否 (分割は不可)"
        );

        // 既に単一文節の hint (内側に境界なし) は統合対象が無い → 拒否 (再変換不要)。
        assert!(
            !hint_merges_whole_segments(&[0, 6, 12], &(0..6)),
            "内側境界の無い hint は統合しない"
        );
    }

    /// ★ 静的辞書の複合語 (よしゅく→予祝) を has_dict_word で検出し、flat convert が
    /// 候補に含める (2026-09 ユーザー報告: 予祝 が出ない)。segmented だと [よ][しゅく]
    /// に割れて出ないので、has_dict_word=true → flat 優先 で 1 語として出す。
    #[test]
    fn has_dict_word_and_convert_yoshuku() {
        let engine = ConversionEngine::new().unwrap();
        assert!(
            engine.has_dict_word("よしゅく"),
            "★ よしゅく は辞書の語 (予祝)"
        );
        assert!(engine.has_dict_word("にほん"), "★ にほん は辞書の語 (日本)");
        assert!(
            !engine.has_dict_word("ぷぷぷぷ"),
            "★ 辞書に無い読みは false"
        );

        let cands = engine
            .convert("よしゅく", &ConversionContext::new())
            .unwrap();
        assert!(
            cands.iter().any(|c| c.surface == "予祝"),
            "★ flat convert に 予祝 が含まれる: {:?}",
            cands.iter().map(|c| c.surface.as_str()).collect::<Vec<_>>()
        );
    }

    /// ★ 文節ごとに選んだ候補 (picked>0) が **1 回で** 訂正学習される
    /// (2026-09 ユーザー報告: 文節ごとの選択が保持されない)。segmented 確定時に
    /// 文節ごとの観察 (読み, 選択, 候補列, picked) を記録するようにした前提のテスト。
    /// 「あべ→阿部」を picked=1 (既定でない) で 1 回確定した観察から学習が効く。
    #[test]
    fn per_segment_pick_learns_in_one_commit() {
        use crate::learning::{extract_corrections, ObservationEvent};

        // 文節「あべ」で既定でない候補「阿部」(index 1) を選んで確定した観察 1 件。
        let events = vec![ObservationEvent::commit_with_candidates(
            "あべ",
            "阿部",
            vec!["安倍".into(), "阿部".into(), "アベ".into()],
            Some(1),
        )];
        // picked>0 は重み 3、min_seen=2 なので **1 回で** 選好化される。
        let store = extract_corrections(&events, 2);
        assert!(
            store.preferred("あべ").is_some(),
            "★ picked>0 は 1 回で学習される: {store:?}"
        );

        // convert に反映 (辞書に無い 阿部 も注入されて 1 位)。
        let mut engine = ConversionEngine::new().unwrap();
        engine.set_corrections(store);
        let after = engine.convert("あべ", &ConversionContext::new()).unwrap();
        assert_eq!(
            after.selected().unwrap().surface,
            "阿部",
            "★ 文節で選んだ 阿部 が次から 1 位"
        );
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

    /// 実機モデル + 実機 corrections.toml を読んで「まつやさんとなんとか」の
    /// 文節分割と各文節候補を目視する診断テスト (通常は ignore)。
    ///
    /// 実行: `cargo test -p nuko-core --features akaza diag_segment_matsuya -- --ignored --nocapture`
    #[cfg(feature = "akaza")]
    #[test]
    #[ignore = "実機モデルが要る診断用"]
    fn diag_segment_matsuya() {
        let home = std::env::var("HOME").expect("HOME");
        let base = format!("{home}/Library/Application Support/nuko-ime");
        let model_dir = format!("{base}/akaza-model");
        let corrections = format!("{base}/corrections.toml");

        let mut engine = ConversionEngine::with_libakaza(&model_dir).unwrap();
        assert!(
            engine.has_libakaza(),
            "実機モデルが load できていない: {model_dir}"
        );
        if std::path::Path::new(&corrections).exists() {
            engine.load_corrections(&corrections).unwrap();
        }

        for input in [
            "まつや",
            "まつやさんとなんとか",
            "まつやさんなんとか", // 「んな」を含み nn 曖昧扱い → corrections_applied で救済
            "じかんなるとき",     // nn: 訂正なし → corrections_applied=false (flat へ)
            "せんねん",           // nn: 千円
            "みのさんに",         // ユーザー報告: flat で Shift しても文節にならない
            "みのさん",
            "さわれる", // 学習 さわれる→触れる が分割で効かない (ユーザー報告)
            "1もじ",    // 学習 1もじ→1文字 が分割で効かない
            "ざびさんとおおさかにいってきました", // 分割境界の学習: ざび→ザビ を境界ヒントに
            "にほんご？", // 記号混じり読みも libakaza が [日本語][？] と割る
            "こんにちは1", // 数字混じり読みの耐性確認
            "こんにちは12",
            "どうかえしたらよいかな", // ユーザー報告: よいかな→[よ][以下][な] 誤分割
            "たいかく", // 回帰(修正済): いか→以下 が体格を「た以下く」に割ってはいけない
            "せいかつ", // 回帰(修正済): 生活を「背以下つ」に割ってはいけない
            "だいじょうぶ", // ユーザー報告: 最頻出語なのに「台じょうぶ」になる (min モデル弱)
            "めーるしておきましただいじょうぶです", // 長文: 大丈夫が文中で出るか
            "しまもとちょう", // ユーザー報告: 誤学習 しまもとちょう→島本腸(12回確定で自己強化)を
            // forget 後は [しまもと][ちょう]→島本町 になる (地名接尾 町)
            "ちょう", // ちょう単体で 町 が1位に出る (モデルは町を知っている)
        ] {
            let nn = super::nn_alternate_readings(input).len() > 1;
            println!("\n=== 入力: {input} (nn_ambiguous={nn}) ===");
            match engine.convert_segmented(input).unwrap() {
                None => println!("  (segmented None — 単一文節 or 無効)"),
                Some(seg) => {
                    println!(
                        "  corrections_applied={} 連結='{}'",
                        seg.corrections_applied,
                        seg.current_surface()
                    );
                    for (i, s) in seg.segments.iter().enumerate() {
                        let cands: Vec<String> = s
                            .candidates
                            .iter()
                            .take(6)
                            .map(|c| c.surface.clone())
                            .collect();
                        println!(
                            "  文節[{i}] 読み='{}' 選択='{}' 候補={:?}",
                            s.reading,
                            s.candidates
                                .get(s.selected)
                                .map(|c| c.surface.as_str())
                                .unwrap_or("?"),
                            cands
                        );
                    }
                }
            }
        }

        // 単一文節「みのさん」を Shift+← (extend_left) で割れるか確認。
        println!("\n=== resize 単一文節: みのさん を Shift+← ===");
        if let Some(seg) = engine.convert_segmented("みのさん").unwrap() {
            match engine
                .resize_segment(&seg, /*extend_right=*/ false)
                .unwrap()
            {
                None => println!("  resize None (割れない)"),
                Some(rs) => {
                    let readings: Vec<&str> =
                        rs.segments.iter().map(|s| s.reading.as_str()).collect();
                    println!("  → {} 文節 読み={:?}", rs.segments.len(), readings);
                }
            }
        }

        // 数字を flat 変換したとき全角/漢数字候補が出るか (B: 数字変換)。
        let ctx = ConversionContext::new();
        for r in ["さわれる", "1もじ"] {
            let cands: Vec<String> = engine
                .convert(r, &ctx)
                .unwrap()
                .iter()
                .take(6)
                .map(|c| c.surface.clone())
                .collect();
            println!("  flat convert('{r}') = {cands:?}");
        }
        for d in ["1", "12", "123"] {
            let cands: Vec<String> = engine
                .convert(d, &ctx)
                .unwrap()
                .iter()
                .take(6)
                .map(|c| c.surface.clone())
                .collect();
            println!("  flat convert('{d}') = {cands:?}");
        }
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
