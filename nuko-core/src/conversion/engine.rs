//! 変換エンジン本体

use std::path::{Path, PathBuf};

#[cfg(feature = "akaza")]
use super::backend::LibakazaBackend;
#[cfg(feature = "akaza")]
use super::SegmentedConversion;
use super::{Candidate, CandidateList, CandidateSource, ConversionContext};
use crate::dictionary::DictionaryManager;
use crate::error::{NukoError, Result};
use crate::input::{to_halfwidth_katakana, to_katakana};
use crate::learning::{
    ContextFrequencyStore, CorrectionStore, DateFormatStore, LearningManager, ManualScoreStore,
    PhraseStore, SegmentationStore,
};

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

/// 表層が全て かな/カナ/半角カナ/長音 か (= 機械生成されうる「疑似語」)。
///
/// 漢字・英字を含む実語は false。純カタカナの外来語 (リアクト/コーヒー) も true になるので、
/// これ単体では降格しない。降格の可否は [`curate_pseudo_words`] が「最上位が実語か」で守る
/// (= 未知語ガード、F7 強要の回避)。
fn is_pseudo_word(surface: &str) -> bool {
    !surface.is_empty() && surface.chars().all(is_kana_char)
}

/// 表層が全て半角カタカナ域か (ﾆﾝｲ / ﾌﾟﾛｸﾞﾗﾑ 等)。半角は常に下位へ沈める。
fn is_halfwidth_kana_word(surface: &str) -> bool {
    !surface.is_empty()
        && surface
            .chars()
            .all(|c| matches!(c, '\u{FF61}'..='\u{FF9F}'))
}

/// 疑似語を降格するときに score から引く penalty。
///
/// 降格候補を「どんな実語よりも確実に下」へ送るための大きな一律ペナルティ。固定値 (-90 等) への
/// 置換ではダメ: 経路でスコア帯が違う (flat は `LIBAKAZA_PRIORITY_BOOST` 込みで +97129 /
/// segmented は libakaza 生スコアの -2871 等) ため、固定値だと segmented の大きな負値の実語
/// (任い=-5578) より上に浮いてしまう。相対的に大きく引けば帯に依らず常に実語の下に行く。
const PSEUDO_WORD_DEMOTE_PENALTY: i32 = 1_000_000;

/// 候補キュレーション: 機械生成の疑似語 (かな/カナ/半角) を実語より下へ沈める。
///
/// libakaza は読みの transliteration (「にんに」→ にんに/ニンニ/ﾆﾝﾆ) を候補として返し、実語の漢字と
/// 同じスコア空間で並ぶ (実測: flat は +97129、segmented は生の -2871)。結果「にんに→任意」の直下に
/// ニンニ/にんに/ﾆﾝﾆ/ニンイ/ﾆﾝｲ が 5 連で並ぶ。これを出自でなく「疑似語か」で一網打尽に沈める。
///
/// ルール:
/// - **実語の最高スコアが疑似語の最高スコア以上** のとき (= 読みが実語に解決した) だけ、
///   ひらがな/全角カナの疑似語を降格する。**同点も実語を優先**する: segmented は学習なしだと
///   任意/にんい/ニンイ が同点 (-2871) で並び、`max_by_key` の単純 top では疑似語が「top」に
///   選ばれて降格をすり抜ける (2026-11-22 実測の回帰)。best_real vs best_pseudo で比較して防ぐ。
/// - **実語が無い / 実語より疑似語が高い** (リアクト/コーヒー等の外来語で、カタカナ自体が答え)
///   なら全角カナ・ひらがなは触らない = **未知語ガード** (降格すると毎回 F7 を強いるため)。
/// - **半角カタカナは常に**下位へ (Mozc 定石: 半角は 1 ページ目に出さない)。
///
/// 降格は `PSEUDO_WORD_DEMOTE_PENALTY` を引くだけなので候補は**消えず**順序だけ下がる
/// (「カタカナは必ず出る」等の既存テストは不変)。意図的に低スコアで足した代替読みのかな
/// (`ALT_READING_KANA_SCORE`、しんに→しんい) も実語が勝つなら下へ送られるが候補には残る。
/// 純粋関数。flat `convert()` と segmented の両経路から呼ぶ (CLAUDE.md #178/#184 の教訓)。
fn curate_pseudo_words(cands: &mut [Candidate]) {
    let best_real = cands
        .iter()
        .filter(|c| !is_pseudo_word(&c.surface))
        .map(|c| c.score)
        .max();
    let best_pseudo = cands
        .iter()
        .filter(|c| is_pseudo_word(&c.surface))
        .map(|c| c.score)
        .max();
    // 実語が疑似語と同点以上 = 読みが実語に解決 → 疑似語を降格。実語無し or 疑似語の方が高い
    // (外来語) → 未知語ガードで全角カナ・ひらがなは据え置く。
    let demote_pseudo = matches!((best_real, best_pseudo), (Some(r), Some(p)) if r >= p);
    for c in cands.iter_mut() {
        let is_half = is_halfwidth_kana_word(&c.surface);
        let demote = is_half || (demote_pseudo && is_pseudo_word(&c.surface));
        if demote {
            // 半角カナは全角カナ/ひらがなより更に 1 段下へ。
            let extra = i32::from(is_half);
            c.score = c
                .score
                .saturating_sub(PSEUDO_WORD_DEMOTE_PENALTY)
                .saturating_sub(extra);
        }
    }
    cands.sort_by_key(|c| std::cmp::Reverse(c.score));
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

/// 代替読み (しんに→しんい) が実在語のとき、その**読みのひらがな**を候補に足すときの score。
///
/// 位置の意図: **旧字体 (`KYUJITAI_SCORE` = -80) より上・実在漢字 (BOOST 済み) より下**。
/// 「真意」等の漢字変換は押し下げず、かつ「しんい」を旧字体の眞意/眞に より前に出す。
/// この不変条件 (> -80) を壊すと旧字体の下に沈むので、kyujitai 側を変えたら要見直し。
#[cfg(feature = "akaza")]
const ALT_READING_KANA_SCORE: i32 = -70;

/// 単語登録 (ユーザー辞書) 候補に乗せる優先度ブースト。
///
/// 辞書候補は素の score が -100 前後で、libakaza (BOOST 100_000) の下に沈む。
/// 「ユーザーが明示的に登録した語」は **最優先で 1 位に出す** べきなので、
/// libakaza より大きいブーストを乗せる。
const USER_DICT_BOOST: i32 = 200_000;

/// 平仮名ライブ変換 (#229) で、1 打鍵ごとの推測変換を走らせる**読みの最大文字数**。
///
/// ライブ変換は未確定の読み全体を毎打鍵 [`ConversionEngine::live_convert`] に渡すが、その中身
/// (libakaza のラティス k-best 探索。segmented も flat も内部で `backend.convert` を呼ぶ) は
/// **読みが長くなるとほぼ二次的にコストが増える**。実測 (2026-11, Apple Silicon・実モデル) では
/// 約 20 字までは 1 打鍵あたりの増分がほぼ無視できるのに対し、40 字強で ~1 秒/打鍵に達し、
/// それ以上では打鍵のたびにアプリがひっかかる (= 実用不能)。
///
/// そこで、この文字数を超える読みは**推測変換を行わず生のかなをそのまま返す** (O(1))。
/// 普通の文節変換は Space を押すまで 20 字未満に収まるので実害は無く、超長文の未確定ランだけ
/// 「かな表示のまま」に degrade する (Space を押せば通常どおり変換される)。
/// 値を上げるとより長い読みでもライブ推測が効くが、その分 1 打鍵の最悪レイテンシが伸びる。
const LIVE_CONVERT_MAX_READING_CHARS: usize = 24;

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

/// 「ん＋母音」境界を回復する代替読みを生成する（**文全体比較用・広め**、原文は含めない）。
///
/// 2 種類の崩れを両方カバーする:
/// 1. **ん＋な行 → ん＋母音**（`nn`打ち: `にんに`→`にんい`）。`gokakuninn…` のように ん を
///    二重 n で明示したが、続く母音が な行に固定された崩れ。
/// 2. **な行 → ん＋母音**（**単一 n 打ち**: `にに`→`にんい`）。`gokakunin…` のように ん を
///    単一 n で打ち、直後の母音と結合して な行に潰れた崩れ（＝ん が消える）。これは #192
///    の `nn_alternate_readings`（パターン1 のみ）では救えなかった本命ケース。
///
/// いずれも libakaza + [`sentence_score`] の **strict->** で選別する前提なので、ここでは
/// 候補を広めに出してよい（正しさは文スコアが守る＝残念/案内/私の名前 等は原文が高スコアで不変）。
/// 位置ごとに 1 箇所だけ変換し、`MAX` で総数を抑えて perf と組合せ爆発を防ぐ。
#[must_use]
pub fn vowel_boundary_alternate_readings(reading: &str) -> Vec<String> {
    const NA_ROW: [(char, char); 5] = [
        ('な', 'あ'),
        ('に', 'い'),
        ('ぬ', 'う'),
        ('ね', 'え'),
        ('の', 'お'),
    ];
    const MAX: usize = 8; // 1 確定あたりの代替変換回数の上限（perf 保護）

    let chars: Vec<char> = reading.chars().collect();
    let mut out: Vec<String> = Vec::new();
    for i in 0..chars.len() {
        if out.len() >= MAX {
            break;
        }
        let Some(&(_, vowel)) = NA_ROW.iter().find(|(na, _)| *na == chars[i]) else {
            continue;
        };
        if i >= 1 && chars[i - 1] == 'ん' {
            // パターン1: ん＋な行 → ん＋母音（な行を母音に置換）
            let mut alt = chars.clone();
            alt[i] = vowel;
            let s: String = alt.into_iter().collect();
            if s != reading && !out.contains(&s) {
                out.push(s);
            }
        } else {
            // パターン2: な行 → ん＋母音（単一 n 崩れ回復。ん を挿入し母音に）
            let mut alt: Vec<char> = chars[..i].to_vec();
            alt.push('ん');
            alt.push(vowel);
            alt.extend_from_slice(&chars[i + 1..]);
            let s: String = alt.into_iter().collect();
            if s != reading && !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

/// 文スコア = 各文節の**選択中候補**の score 合計 (i64 で桁あふれ回避)。
///
/// 異なる分割 (原文 vs ん+母音代替) を比較するための指標。score は同一 libakaza
/// モデル由来なので比較可能。候補が空の文節は 0 とみなす。
#[must_use]
pub fn sentence_score(segmented: &super::SegmentedConversion) -> i64 {
    segmented
        .segments
        .iter()
        .filter_map(super::Segment::current)
        .map(|c| i64::from(c.score))
        .sum()
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

/// flat の候補列に手動スコア調整 (#228, ⌥←→) の bias を加減算する。空なら無変化。
fn apply_manual_scores_flat(
    cands: &mut [Candidate],
    manual_scores: &ManualScoreStore,
    reading: &str,
) {
    if manual_scores.is_empty() {
        return;
    }
    for c in cands.iter_mut() {
        let b = manual_scores.bias(reading, &c.surface);
        if b != 0 {
            c.score = c.score.saturating_add(b);
        }
    }
}

/// 文節別変換結果に **文節ごとの** 手動スコア調整 (#228, ⌥←→) を適用し並べ替える。
///
/// flat の [`ConversionEngine::convert`] と同じ手動バイアスを segmented 経路にも効かせる
/// (片方だけだと文中で効かない = [[nuko-ime-segment-frequency-gap]] #178/#184 の教訓)。
/// 読み完全一致の候補だけに加減算する (接尾/分割流用はしない = 過剰汎化を避ける)。空なら無変化。
#[cfg_attr(not(feature = "akaza"), allow(dead_code))]
pub fn apply_segment_manual_scores(
    segmented: &mut super::SegmentedConversion,
    manual_scores: &crate::learning::ManualScoreStore,
) {
    if manual_scores.is_empty() {
        return;
    }
    for seg in &mut segmented.segments {
        let seg_reading = seg.reading.clone();
        let mut changed = false;
        for c in &mut seg.candidates {
            let bias = manual_scores.bias(&seg_reading, &c.surface);
            if bias != 0 {
                c.score = c.score.saturating_add(bias);
                changed = true;
            }
        }
        if changed {
            seg.candidates.sort_by_key(|c| std::cmp::Reverse(c.score));
            seg.select(0);
        }
    }
}

/// 文節別変換結果に **文節ごとの** Layer 0 頻度学習を適用し並べ替える。
///
/// flat の [`ConversionEngine::convert`] は頻度候補 ([`LearningManager::get_candidates`]) を
/// マージするが、`convert_segmented` では **未適用** だった。このため「文中に現れる語」を
/// 何度確定しても、次回の文節変換で頻度が参照されず **libakaza の既定順が永遠に勝つ**
/// バグがあった (2026-11 ユーザー報告:「砲→方 / 用→様 が何度直しても勝てない。
/// 学習に勝って欲しい」)。
///
/// Layer 2 (訂正学習 [`apply_segment_corrections`]) は観察ログ **opt-in** 前提だが、
/// Layer 0 頻度は **確定のたびに常時記録** される ([`ConversionEngine::commit`])。よって
/// これを segmented にも効かせれば、オプトイン無効のユーザーでも
/// **「一度でも選び直せば次回から勝つ」** が成立する。
///
/// 一致は **読み完全一致のみ** (`seg.reading == entry.reading`)。接尾・分割への流用は
/// しない (over-generalization で常用語を壊すため — `correction.rs` の 2026-09-29 回帰
/// 「だい→台 が だいじょうぶ を 台じょうぶ に壊す」の教訓)。
// 呼び出し元 convert_segmented は akaza-gated なので、非 akaza の lib ビルドでは未使用
// (テストからは使う)。
#[cfg_attr(not(feature = "akaza"), allow(dead_code))]
pub fn apply_segment_frequency(
    segmented: &mut super::SegmentedConversion,
    learning: &LearningManager,
    context_freq: &ContextFrequencyStore,
) {
    let ctx = ConversionContext::new();
    let n = segmented.segments.len();
    // index ループ: 文脈つき学習 (#228/A) で「直前文節の現在表層」を読むため。
    // 直前文節は本ループで既に Layer0/文脈適用済みなので、その確定表層が文脈になる (cascade)。
    for i in 0..n {
        // 文脈 = 直前文節の現在表層。先頭文節 (i==0) は文内に直前が無いので文脈適用しない
        // (記録側 commit は文頭でも global prev で記録するが、convert 時はここに global prev が
        //  無いので i>=1 のみ。どう|しよう の主要ケースは i>=1 で成立する)。
        let prev_surface: Option<String> = if i == 0 {
            None
        } else {
            segmented.segments[i - 1].surface().map(|s| s.to_string())
        };

        let seg = &mut segmented.segments[i];
        let seg_reading = seg.reading.clone();
        let mut changed = false;

        // --- Layer 0 頻度 (読み完全一致・max 合成) ---
        if let Ok(learned) = learning.get_candidates(&seg_reading, &ctx) {
            for lc in learned {
                // 一過性の日付/時刻表層 (きょう→2026年…) は学習に載せない。commit 側で記録を
                // 抑止済みだが、古い learning.json 掃除を兼ねた belt-and-suspenders (flat と同じ)。
                if super::datetime::is_datetime_surface(&seg_reading, &lc.surface) {
                    continue;
                }
                // 読み完全一致の学習表層: Layer 0 頻度スコアを **max で引き上げる** (加算ではない)。
                //
                // ★なぜ max か (2026-11-12、--explain でスコア内訳を透明化して判明した回帰の根治):
                // 既存候補を「加算」にすると、libakaza の基礎スコア (ログ確率ベースで多くは負値、
                // 例 ように = −1408) を学習スコアに足し込むため、同じ count でも基礎が負に沈んで
                // いる表層ほど不利になる。一方、候補に無い表層は下の else で **純 Layer 0** を注入
                // するので、低 count の注入表層が高 count の既存表層に勝つ非対称が生まれていた
                // (例: 用に count 4 = 200040 が ように count 36 = −1408+200360 = 198952 に勝つ)。
                // → 既存も注入も **純 Layer 0 スコア** に揃え、count 順の正しい並びに戻す。
                // 基礎が学習より高い稀なケース (強い libakaza 候補) は max がそのまま基礎を残す。
                if let Some(c) = seg.candidates.iter_mut().find(|c| c.surface == lc.surface) {
                    c.score = c.score.max(lc.score);
                    changed = true;
                } else {
                    seg.candidates.push(
                        Candidate::new(&lc.surface, &seg_reading)
                            .with_score(lc.score)
                            .with_source(CandidateSource::User),
                    );
                    changed = true;
                }
            }
        }

        // --- 文脈つき学習 (#228 残件③/A): 直前文節が文脈に一致する表層を他候補の上へ ---
        //
        // 例「どうしようかな」= どう|しよう|… で、しよう の直前が どう のとき、過去に
        // (しよう, しよう, prev=どう) を確定していれば しよう を 1 位へ。別文脈 (仕様書など) で
        // 学習した 仕様 (Layer0・文脈なし) に、文脈一致の しよう が勝てるようにする。
        //
        // 持ち上げ先は「他候補の最上位のすぐ上」= 文脈一致候補が確実に 1 位。ただし訂正tier
        // (CORRECTION_BOOST) **未満**にクランプするので、打ち直し訂正・強い手動スコアは文脈より
        // 上に残る (階層: 教える/訂正/手動 > 文脈 > 素の Layer0 > libakaza)。
        if let Some(prev) = prev_surface.as_deref() {
            if !context_freq.is_empty() {
                if let Some((surf, _count)) = context_freq.best_surface(&seg_reading, prev) {
                    let surf = surf.to_string();
                    let max_other = seg
                        .candidates
                        .iter()
                        .filter(|c| c.surface != surf)
                        .map(|c| c.score)
                        .max()
                        .unwrap_or(i32::MIN);
                    let target = max_other
                        .saturating_add(crate::learning::CONTEXT_MATCH_MARGIN)
                        .min(crate::learning::correction::CORRECTION_BOOST - 1);
                    if let Some(c) = seg.candidates.iter_mut().find(|c| c.surface == surf) {
                        if target > c.score {
                            c.score = target;
                            changed = true;
                        }
                    } else {
                        seg.candidates.push(
                            Candidate::new(&surf, &seg_reading)
                                .with_score(target)
                                .with_source(CandidateSource::User),
                        );
                        changed = true;
                    }
                }
            }
        }

        if changed {
            seg.candidates.sort_by_key(|c| std::cmp::Reverse(c.score));
            seg.select(0); // 並べ替え後の先頭 (最良) を選択に戻す
        }
        // 学習が効いた = Layer 2 と同様「学習適用」フラグを立てる (seg borrow を離してから)。
        // controller は nn 曖昧語で segmented を flat より優先する判定に使う。
        if changed {
            segmented.corrections_applied = true;
        }
    }
}

/// 文節ごとに **単語登録 (ユーザー辞書)** を適用する。
///
/// flat の [`ConversionEngine::convert`] は辞書 lookup で `source=User` の候補を得て
/// `USER_DICT_BOOST` を足し 1 位にするが、`convert_segmented` は未適用だったため
/// 「け→家 を登録しても**文中**では出ない」バグがあった (2026-11 ユーザー報告)。
/// #178 (Layer0)・#184 (segmented スコア) と同型の「flat にあって segmented に無い」漏れ。
///
/// 各文節の読みに一致する登録語を **注入** (libakaza k-best に無くても出す) し、
/// [`pin_user_dict_top`] で他候補の最大より上に固定して「登録語は 1 位」を保証する。
/// 固定ブーストでなく相対持ち上げにするのは、Layer0 頻度 (確定回数で上限なく伸びる)
/// に負けないため (下記 fn のコメント参照)。
#[cfg(feature = "akaza")]
fn apply_segment_user_dict(
    segmented: &mut super::SegmentedConversion,
    dictionary: &DictionaryManager,
) {
    for seg in &mut segmented.segments {
        let seg_reading = seg.reading.clone();
        let entries = dictionary.user_dictionary().lookup(&seg_reading);
        if entries.is_empty() {
            continue;
        }
        let mut surfaces: Vec<String> = Vec::with_capacity(entries.len());
        for e in entries {
            surfaces.push(e.surface.clone());
            // libakaza k-best に無い登録語は注入する (無ければ文中で出ない)。
            if !seg.candidates.iter().any(|c| c.surface == e.surface) {
                seg.candidates.push(
                    Candidate::new(&e.surface, &seg_reading)
                        .with_score(e.score.saturating_add(USER_DICT_BOOST))
                        .with_source(CandidateSource::User),
                );
            }
        }
        pin_user_dict_top(&mut seg.candidates, &surfaces);
        seg.candidates.sort_by_key(|c| std::cmp::Reverse(c.score));
        seg.select(0);
    }
}

/// 登録語 (ユーザー辞書) の `surfaces` を、同じ候補リスト内の他のどの候補より
/// 確実に上へ固定する (= 変換 1 位)。
///
/// Layer0 頻度は確定のたび伸び (`200_000 + count*10`)、さらに追い越し boost が
/// 単調に積まれる ([`FrequencyEntry::score`]) ため、固定値の `USER_DICT_BOOST` だけだと
/// 「何度も確定したかな」に負け得る (2026-11 ユーザー報告「け→家 を登録したのに文中で
/// け が 1 位」: 高頻度＋boost で伸びた け が固定ブーストの 家 を超えた)。
/// そこで固定ブーストでなく **「登録語以外の最大 + 1」** へ持ち上げ、
/// 頻度の大小に依らず 1 位を保証する。`surfaces` が空なら完全に無変化。
/// flat ([`ConversionEngine::convert`]) と segmented 両方から呼び、挙動を一致させる。
fn pin_user_dict_top(cands: &mut [Candidate], surfaces: &[String]) {
    if surfaces.is_empty() {
        return;
    }
    let is_user = |c: &Candidate| surfaces.iter().any(|s| s == &c.surface);
    let max_other = cands
        .iter()
        .filter(|c| !is_user(c))
        .map(|c| c.score)
        .max()
        .unwrap_or(0);
    let floor = max_other.saturating_add(1);
    for c in cands.iter_mut() {
        if is_user(c) {
            c.score = c.score.max(floor);
            c.source = CandidateSource::User;
        }
    }
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
    /// 文節位置の学習 (ユーザーが切り直した「切り方」)。空なら無変化。
    segmentations: SegmentationStore,
    /// フレーズ予測 (#187)。確定した文を読み全体→表層全体で覚え、前方一致で予測する。
    /// 空なら [`Self::predict`] は静的辞書のみ。個人データ・非共有。
    phrases: PhraseStore,
    /// 日付/時刻フォーマット選好。確定した形式 (ymd_slash 等) を覚え、日付候補を
    /// その形式ほど上位に出す。リテラル日付は学習しない。空なら無変化。個人・非共有。
    date_formats: DateFormatStore,
    /// `date_formats` の保存先 (set されていれば commit 時に自動 save)。
    date_formats_path: Option<PathBuf>,
    /// 手動スコア調整 (#228, ⌥←→)。読み×表層へユーザーが付けた符号付きバイアス。空なら無変化。個人・非共有。
    manual_scores: ManualScoreStore,
    /// 文脈つき候補学習 (#228 残件③/A)。(読み,表層,直前文節表層) ごとの確定回数。空なら無変化。個人・非共有。
    context_freq: ContextFrequencyStore,
    /// `context_freq` の保存先 (set されていれば commit 時に自動 save)。
    context_freq_path: Option<PathBuf>,
    /// 旧字体モード。ON なら候補の旧字体版 (國/體/學…) を末尾に追加する。既定 OFF。
    kyujitai_mode: bool,
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
            segmentations: SegmentationStore::default(),
            phrases: PhraseStore::default(),
            date_formats: DateFormatStore::default(),
            date_formats_path: None,
            manual_scores: ManualScoreStore::default(),
            context_freq: ContextFrequencyStore::default(),
            context_freq_path: None,
            kyujitai_mode: false,
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
            segmentations: SegmentationStore::default(),
            phrases: PhraseStore::default(),
            date_formats: DateFormatStore::default(),
            date_formats_path: None,
            manual_scores: ManualScoreStore::default(),
            context_freq: ContextFrequencyStore::default(),
            context_freq_path: None,
            kyujitai_mode: false,
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

    /// この読みに対して **segmented 変換を flat より優先すべきか** の判定（唯一の真実源）。
    ///
    /// `do_convert`（controller）と `live_convert`（#229 ライブ変換）の両方がこの 1 つを使い、
    /// 経路選択のロジックが 2 箇所に分裂しないようにする（CLAUDE.md「コメント/コード乖離の防止」
    /// と同じ思想＝規則は 1 箇所）。条件（do_convert の旧インライン判定を移設）:
    /// - 2 文節以上であること（単一文節は flat の方が候補が豊富）。
    /// - whole-reading 訂正 or 静的辞書の語があるなら flat 優先（segmented は誤分割で不発し得る）。
    /// - nn 曖昧語（ん＋な行↔ん＋母音）は原則 flat（flat は代替読みをマージする）。ただし
    ///   per-segment 訂正が効いた（`corrections_applied`）ときは segmented を優先。
    ///
    /// libakaza (`akaza` feature) 無効ビルドには segmented 経路自体が無いので gate する。
    #[cfg(feature = "akaza")]
    #[must_use]
    pub fn segmented_preferred(&self, reading: &str, seg: &SegmentedConversion) -> bool {
        if seg.segments.len() < 2 {
            return false;
        }
        let nn_ambiguous = nn_alternate_readings(reading).len() > 1;
        let prefer_flat = self.has_whole_correction(reading) || self.has_dict_word(reading);
        !prefer_flat && (!nn_ambiguous || seg.corrections_applied)
    }

    /// 平仮名ライブ変換（#229）の「推測表層」を返す。
    ///
    /// 打鍵中に marked text として inline 表示するための、**確定前のベストゲス**。
    /// `segmented_preferred` で `do_convert` と同じ経路選択（segmented 優先 /
    /// 条件次第で flat）を行い、**表層文字列のみ** を返す（候補リスト・パネルは作らない）。
    /// （intra-doc link にしないのは `segmented_preferred` が `akaza` feature gated で、
    /// 無効ビルドの rustdoc がリンクを解決できず `-Dwarnings` で落ちるため。）
    /// 変換不能・空読み・エラー時は読みをそのまま返す（呼び出し側が平仮名表示にフォールバック）。
    #[must_use]
    pub fn live_convert(&self, reading: &str, context: &ConversionContext) -> String {
        if reading.is_empty() {
            return String::new();
        }
        // 長すぎる読みは推測変換を走らせず生かなを返す (打鍵レイテンシの上限を担保)。
        // 理由と閾値は LIVE_CONVERT_MAX_READING_CHARS の doc を参照。
        if reading.chars().count() > LIVE_CONVERT_MAX_READING_CHARS {
            return reading.to_string();
        }
        // segmented を優先（libakaza 有効時のみ。無効ビルドは flat 一択）。
        #[cfg(feature = "akaza")]
        {
            if let Ok(Some(seg)) = self.convert_segmented(reading) {
                if self.segmented_preferred(reading, &seg) {
                    return seg.current_surface();
                }
            }
        }
        // flat（単一文節 / segmented 非優先 / libakaza 無効）。
        match self.convert(reading, context) {
            Ok(candidates) => candidates
                .selected()
                .map(|c| c.surface.clone())
                .unwrap_or_else(|| reading.to_string()),
            Err(_) => reading.to_string(),
        }
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

    /// 手動スコア調整 (#228, ⌥←→) を設定する (変換時に該当候補へ bias)。空なら無変化。
    pub fn set_manual_scores(&mut self, manual_scores: ManualScoreStore) {
        tracing::info!(count = manual_scores.len(), "手動スコア調整を設定");
        self.manual_scores = manual_scores;
    }

    /// `manual_score.json` から手動スコア調整を load する (無ければ空)。
    pub fn load_manual_scores(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.manual_scores = ManualScoreStore::load(path)?;
        tracing::info!(count = self.manual_scores.len(), "手動スコア調整を load");
        Ok(())
    }

    /// 読み×表層の手動スコアを `delta` 動かす (#228, ⌥←→)。新しいバイアスを返す。
    /// controller がキー入力で呼び、直後に [`Self::save_manual_scores`] で永続化する。
    pub fn nudge_manual_score(&mut self, reading: &str, surface: &str, delta: i32) -> i32 {
        self.manual_scores.nudge(reading, surface, delta)
    }

    /// 読み×表層の手動スコアを絶対値 `value` に**上書き**する (#228, 候補パネルでのポイント直接指定)。
    /// 新しいバイアスを返す。[`Self::nudge_manual_score`] の刻み調整に対し、こちらは打った数値を置く。
    /// controller がスコア編集モードの確定で呼び、直後に [`Self::save_manual_scores`] で永続化する。
    pub fn set_manual_score(&mut self, reading: &str, surface: &str, value: i32) -> i32 {
        self.manual_scores.set_bias(reading, surface, value)
    }

    /// 読み×表層の現在の手動スコアバイアス (無ければ 0)。スコア編集モードの初期表示に使う。
    #[must_use]
    pub fn manual_score_bias(&self, reading: &str, surface: &str) -> i32 {
        self.manual_scores.bias(reading, surface)
    }

    /// 手動スコア調整を `manual_score.json` に保存する。
    pub fn save_manual_scores(&self, path: impl AsRef<Path>) -> Result<()> {
        self.manual_scores.save(path)
    }

    /// 手動スコア調整を全消去する。除去件数を返す (「学習を忘れる」用)。
    pub fn forget_manual_scores(&mut self) -> usize {
        self.manual_scores.forget_all()
    }

    /// 読み (と任意で表層) の手動スコア調整を忘れる。除去件数を返す。
    /// `surface` が `None` ならその読みの全表層。「読みを指定して忘れる」UI から呼ぶ。
    pub fn forget_manual_score(&mut self, reading: &str, surface: Option<&str>) -> usize {
        self.manual_scores.forget(reading, surface)
    }

    /// `segmentations.toml` から文節位置の学習を load する(無ければ空)。
    ///
    /// # エラー
    /// ファイルはあるがパースに失敗した場合。
    pub fn load_segmentations(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.segmentations = SegmentationStore::load(path)?;
        tracing::info!(count = self.segmentations.len(), "文節位置の学習を load");
        Ok(())
    }

    /// 文節位置の学習ストアを設定する。
    pub fn set_segmentations(&mut self, segmentations: SegmentationStore) {
        self.segmentations = segmentations;
    }

    /// 文節位置の学習ストアへの参照(保存・表示用)。
    #[must_use]
    pub fn segmentations(&self) -> &SegmentationStore {
        &self.segmentations
    }

    /// `phrases.json` からフレーズ予測ストアを load する(無ければ空)。
    ///
    /// # エラー
    /// ファイルはあるがパースに失敗した場合。
    pub fn load_phrases(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.phrases = PhraseStore::load(path)?;
        tracing::info!(count = self.phrases.len(), "フレーズ予測を load");
        Ok(())
    }

    /// フレーズ予測ストアを設定する。
    pub fn set_phrases(&mut self, phrases: PhraseStore) {
        self.phrases = phrases;
    }

    /// `date_format.json` から日付フォーマット選好を load し、以後の自動 save 先に設定する。
    ///
    /// # エラー
    /// ファイルはあるがパースに失敗した場合。
    pub fn load_date_formats(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref().to_path_buf();
        self.date_formats = DateFormatStore::load(&path)?;
        tracing::info!(
            count = self.date_formats.len(),
            "日付フォーマット選好を load"
        );
        self.date_formats_path = Some(path);
        Ok(())
    }

    /// 日付フォーマット選好への参照 (保存・表示・テスト用)。
    #[must_use]
    pub fn date_formats(&self) -> &DateFormatStore {
        &self.date_formats
    }

    /// 日付フォーマット選好を全消しして保存する (「学習を忘れる」用)。除去件数を返す。
    pub fn forget_date_formats(&mut self) -> usize {
        let n = self.date_formats.forget_all();
        if n > 0 {
            if let Some(path) = &self.date_formats_path {
                if let Err(e) = self.date_formats.save(path) {
                    tracing::warn!(error = %e, "date_format.json の保存に失敗 (forget)");
                }
            }
        }
        n
    }

    /// フレーズ予測ストアへの参照(保存・表示用)。
    #[must_use]
    pub fn phrases(&self) -> &PhraseStore {
        &self.phrases
    }

    /// `context_freq.json` から文脈つき候補学習を load する (無ければ空)。保存先も覚える。
    pub fn load_context_freq(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.context_freq = ContextFrequencyStore::load(path.as_ref())?;
        self.context_freq_path = Some(path.as_ref().to_path_buf());
        tracing::info!(count = self.context_freq.len(), "文脈学習を load");
        Ok(())
    }

    /// 文脈つき候補学習への参照 (保存・表示・テスト用)。
    #[must_use]
    pub fn context_freq(&self) -> &ContextFrequencyStore {
        &self.context_freq
    }

    /// 文脈つき候補学習を全消しして保存する (「学習を忘れる」用)。除去件数を返す。
    pub fn forget_context_freq(&mut self) -> usize {
        let n = self.context_freq.forget_all();
        if n > 0 {
            if let Some(path) = &self.context_freq_path {
                if let Err(e) = self.context_freq.save(path) {
                    tracing::warn!(error = %e, "context_freq.json の保存に失敗 (forget)");
                }
            }
        }
        n
    }

    /// 読み (と任意で表層) の文脈つき候補学習を忘れて保存する。除去件数を返す。
    /// 「読みを指定して忘れる」UI から、手動スコア等と対で呼ぶ。
    pub fn forget_context_freq_for(&mut self, reading: &str, surface: Option<&str>) -> usize {
        let n = self.context_freq.forget(reading, surface);
        if n > 0 {
            if let Some(path) = &self.context_freq_path {
                if let Err(e) = self.context_freq.save(path) {
                    tracing::warn!(error = %e, "context_freq.json の保存に失敗 (forget)");
                }
            }
        }
        n
    }

    /// 旧字体モードを設定する。ON なら候補に旧字体版を追加する (既定 OFF)。
    pub fn set_kyujitai_mode(&mut self, on: bool) {
        self.kyujitai_mode = on;
    }

    /// 旧字体モードが有効か。
    #[must_use]
    pub fn kyujitai_mode(&self) -> bool {
        self.kyujitai_mode
    }

    /// 文節の「切り方」を学習する(ユーザーが Shift で切り直して確定したとき)。
    ///
    /// `reading` 全体の切り方を `segments`(各文節の読み)で覚える。学習したら `true`。
    /// 連結不一致・1 文節・同じ切り方は学習しない([`SegmentationStore::learn`] の規約)。
    /// 呼び出し側は `true` のとき `segmentations().save(path)` で永続化する。
    pub fn learn_segmentation(&mut self, reading: &str, segments: &[String]) -> bool {
        let changed = self.segmentations.learn(reading, segments);
        if changed {
            tracing::info!(reading, "文節位置を学習");
        }
        changed
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

    /// flat `convert` 用: libakaza の原文＋代替読み (ん+な行→ん+母音) をマージして
    /// `candidates` 末尾に追加する。
    ///
    /// 「nn 曖昧さ」救済: 原文と代替読みの両方を変換し、言語モデルに正しい方を高スコアに
    /// させる (じかんあるとき > じかんなるとき、残念 > ざんえん)。代替読みそのまま (純かな)
    /// はノイズなので入れないが、代替読みが **実在語** (全体 1 語の漢字辞書語) のときだけ
    /// その読みのひらがなを足す (しんに→しんい を選べるように。おんな→おんあ は出さない)。
    #[cfg(feature = "akaza")]
    fn merge_libakaza_candidates(
        backend: &LibakazaBackend,
        reading: &str,
        candidates: &mut CandidateList,
    ) {
        for alt in nn_alternate_readings(reading) {
            let libakaza_candidates = match backend.convert(&alt) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(reading = %alt, error = %e, "libakaza 変換失敗、静的辞書のみで継続");
                    continue;
                }
            };
            for mut candidate in libakaza_candidates {
                // 代替読みそのまま (surface == alt = 純かな) はノイズなので入れない
                // (おんな→おんあ 等)。segmented 側 (merge_nn_alternates) と同じ扱い。
                if alt != *reading && candidate.surface == alt {
                    continue;
                }
                candidate.score = candidate.score.saturating_add(LIBAKAZA_PRIORITY_BOOST);
                // 代替読み由来の候補も学習は **入力読み** に紐づける (#160)。candidate.reading は
                // しんい のままだと Layer0 が しんい でキーされ しんに で引けないため付け替える。
                candidate.reading = reading.to_string();
                if !candidates.iter().any(|c| c.surface == candidate.surface) {
                    candidates.push(candidate);
                }
            }
            // 代替読みが **実在語** なら、その読みのひらがなも候補に足す。
            // score は旧字体より上・実在漢字より下 (ALT_READING_KANA_SCORE 参照)。
            // 一度選べば学習で上位化する。判定詳細は reading_is_single_dict_word 参照。
            if alt != reading
                && backend.reading_is_single_dict_word(&alt)
                && !candidates.iter().any(|c| c.surface == alt)
            {
                candidates.push(
                    Candidate::new(&alt, reading)
                        .with_score(ALT_READING_KANA_SCORE)
                        .with_source(CandidateSource::System),
                );
            }
        }
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

        // 2. libakaza バックエンドが有効なら最優先で候補を追加 (原文＋代替読みのマージ)。
        #[cfg(feature = "akaza")]
        if let Some(backend) = &self.libakaza {
            Self::merge_libakaza_candidates(backend, reading, &mut candidates);
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

        // 記号の代表 1 つを page 1 (2 位) に引き上げる (①規則クラス、下記 fn 参照)。
        boost_representative_symbol(&mut candidates);

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

        // 「きょう」→今日の日付 / 「いま」→現在時刻 等を動的に候補生成する (①規則/生成器)。
        // 該当読み以外は空なので無変化。
        //
        // datetime トリガー読みでは、**過去に学習/辞書で紛れ込んだ古い日付/時刻の表層を
        // 先に除去** してから、now で生成した新しい候補を注入する。これにより既に
        // learning.json に残っている古いエントリ (例: きょう→2026年10月20日) も
        // 自己修復的に出なくなる (2026-11 ユーザー報告の即時解消。今後の学習記録自体は
        // `commit()` 側で抑止済みなので、これは既存データの掃除を兼ねる belt-and-suspenders)。
        if super::datetime::is_datetime_reading(reading) {
            candidates.retain(|c| !super::datetime::is_datetime_surface(reading, &c.surface));
            for mut c in super::datetime::datetime_candidates(reading) {
                // フォーマット選好: 過去に選んだ形式ほど日付グループ内で上位に出す。
                // リテラル日付は学習しないが、形式 (ymd_slash 等) の選好は加点する。
                // 加点は `count × STEP` (上限 MAX_BOOST)。日付候補は低スコア帯 (base 50)
                // に留まり、上限込みでも libakaza/学習候補 (98k/200k+) を超えないので
                // 「今日(漢字)」等の既定は上位に残る (datetime.rs の 2 定数を参照)。
                if let Some(fmt) = super::datetime::classify_datetime_format(&c.surface) {
                    let raw = i64::from(self.date_formats.count(fmt))
                        * i64::from(super::datetime::DATE_FORMAT_PREFERENCE_STEP);
                    let capped =
                        raw.min(i64::from(super::datetime::DATE_FORMAT_PREFERENCE_MAX_BOOST));
                    let boost = i32::try_from(capped)
                        .unwrap_or(super::datetime::DATE_FORMAT_PREFERENCE_MAX_BOOST);
                    c.score = c.score.saturating_add(boost);
                }
                candidates.push(c);
            }
        }

        // 旧字体モード: 候補の旧字体版を末尾に追加 (ソート前。低スコアなので下位に出る)。
        // OFF (既定) なら完全に無変化。
        if self.kyujitai_mode {
            super::kyujitai::append_kyujitai_variants(&mut candidates);
        }

        // 単語登録 (ユーザー辞書) は Layer0 頻度にも勝って常に 1 位にする (登録語=1位)。
        // 上の USER_DICT_BOOST (固定値) だけだと「何度も確定したかな」に負けるため、
        // 他候補の最大を見て確実に上へ固定する。登録が無ければ無変化。segmented と同方式。
        let user_surfaces: Vec<String> = self
            .dictionary
            .user_dictionary()
            .lookup(reading)
            .into_iter()
            .map(|c| c.surface)
            .collect();
        pin_user_dict_top(candidates.all_mut(), &user_surfaces);

        // 手動スコア調整 (#228, ⌥←→): 読み×表層へのユーザーのバイアスを加減算 (空なら無変化)。
        // curate の前に当てて「最上位が実語か」の判定に反映させる。
        apply_manual_scores_flat(candidates.all_mut(), &self.manual_scores, reading);

        // 候補キュレーション: 最上位が実語のとき、機械生成の疑似語 (かな/カナ/半角) を下位へ沈める。
        // ソート直前 = 学習/ユーザー辞書/訂正の全ブーストが乗った最終スコアで「最上位が実語か」を見る。
        curate_pseudo_words(candidates.all_mut());

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
        // ★ 文全体の「ん＋母音」境界救済 (2026-11 ユーザー報告「ご確認いただけ…が永遠に
        //   直らない」)。ローマ字 nn+母音 は `ん+な行` に固定されるため、**文節をまたぐ**
        //   「ご確認(…にん)｜いただけ」の境界は、文節内だけ見る merge_nn_alternates では
        //   救えない (ん が前文節末・い が次文節頭)。そこで **文全体の読み** に ん+な行→
        //   ん+母音 (および な行→ん+母音) の代替を当て、**flat 変換の文全体最良スコアが原文を
        //   上回る読み** だけを採って再分割する。= flat convert() が既にやっている「両読みを
        //   libakaza に変換させ LM にスコアで選ばせる」(残念/案内/クリーンな方で は原文の方が
        //   高スコアで自然に守られる) を、segmented の読み選択に流用したもの。
        //   アポストロフィ不要で ATOK 等と同じ挙動。
        segmented = self.pick_best_nn_alternate_segmentation(backend, reading, segmented)?;
        // ★ 文節位置の学習 (最優先): ユーザーが Shift で切り直した「切り方」を覚えていれば、
        //   読み全体一致でその境界を強制再適用する (2026-10 ユーザー要望「文節位置の学習」)。
        //   ヒント方式より強い = 本人が明示的に直した区切りを尊重する。
        if let Some(forced) = self.resegment_from_learned(backend, reading)? {
            segmented = forced;
        } else if let Some(forced) = self.resegment_with_hints(backend, reading, &segmented)? {
            // 学習済みの切り方が無ければ、訂正読みを境界ヒントにして再分割する。
            segmented = forced;
        }
        self.apply_segment_layers(backend, &mut segmented);
        Ok(Some(segmented))
    }

    /// `convert_segmented` が分割確定後に当てるスコア層をまとめて適用する
    /// (訂正学習 → ん+母音代替マージ → 頻度 Layer0 → 単語登録 → 手動スコア → 旧字体 →
    /// 疑似語キュレーションの順)。
    ///
    /// 分割を変えずに再変換する [`Self::reconvert_segmented_preserving`] からも呼ぶので、
    /// **スコア層を足すときはここ 1 箇所に足せば両経路に効く** (= 片方だけ配線して文中で
    /// 再発する CLAUDE.md #178/#184 の罠を避ける)。各層の経緯は呼び出し元と各 `fn` の doc 参照。
    #[cfg(feature = "akaza")]
    fn apply_segment_layers(
        &self,
        backend: &LibakazaBackend,
        segmented: &mut super::SegmentedConversion,
    ) {
        // Layer 2: 文節ごとに個人選好 (訂正学習) の bias を適用して並べ替える
        // (単体「まつや」は松谷なのに文中で松也になる、を防ぐ 2026-09)。
        apply_segment_corrections(segmented, &self.corrections);
        // ん+母音 の代替候補を各文節末尾に追加 (文中で 単位/範囲/千円 等が出るように 2026-10)。
        self.merge_nn_alternates_into_segments(backend, segmented);
        // Layer 0: 頻度学習を適用 (nn 代替も含む最終候補集合に効かせたいので nn マージの後)。
        // 「砲→方 / 用→様 が勝てない」を常時 ON で救う (2026-11)。
        apply_segment_frequency(segmented, &self.learning, &self.context_freq);
        // 単語登録 (ユーザー辞書) を文節ごとに注入して最優先に (「け→家 が文中で出ない」2026-11)。
        apply_segment_user_dict(segmented, &self.dictionary);
        // 手動スコア調整 (#228, ⌥←→ / ⇧↑): flat と同じ手動バイアスを文節候補へ当てる。
        apply_segment_manual_scores(segmented, &self.manual_scores);
        // 旧字体モード: 各文節の候補に旧字体版を追加 (OFF 既定なら無変化)。
        if self.kyujitai_mode {
            for seg in &mut segmented.segments {
                super::kyujitai::append_kyujitai_variants_vec(&mut seg.candidates);
            }
        }
        // 候補キュレーション: 機械生成の疑似語を下位へ沈める (全ブースト適用後に最後)。
        for seg in &mut segmented.segments {
            curate_pseudo_words(&mut seg.candidates);
        }
    }

    /// 現在の文節境界を**保ったまま**再変換する (手動スコア調整後に分割を維持する用, #228)。
    ///
    /// 読みから自動分割し直す [`Self::convert_segmented`] と違い、与えられた `segmented` の
    /// 各文節読みを `force_ranges` にして libakaza に固定境界で変換させ ([`resize_segment`] と
    /// 同じ機構)、同じスコア層 ([`Self::apply_segment_layers`]) を当てる。
    ///
    /// ユーザーが Shift+←→ で切り直した境界は **確定するまで学習ストアに入らない** ので、
    /// 未確定セッション中にスコア調整 (⌥←→ / ⇧↑) で `convert_segmented` を呼ぶと自動分割に
    /// 戻ってしまう (2026-11 ユーザー報告「変換範囲をかえてスコア上下すると区切りが戻る」)。
    ///
    /// [`resize_segment`]: Self::resize_segment
    ///
    /// # 戻り値
    /// - `Ok(Some(_))` — 境界を保った再変換結果 (focus は 0。呼び出し元で復元する)
    /// - `Ok(None)` — libakaza 無効 / 文節が空 / 読み連結が壊れている / 固定境界での変換が空
    #[cfg(feature = "akaza")]
    pub fn reconvert_segmented_preserving(
        &self,
        segmented: &super::SegmentedConversion,
    ) -> Result<Option<SegmentedConversion>> {
        let Some(backend) = &self.libakaza else {
            return Ok(None);
        };
        let readings: Vec<String> = segmented
            .segments
            .iter()
            .map(|s| s.reading.clone())
            .collect();
        if readings.is_empty() {
            return Ok(None);
        }
        let full_reading = readings.concat();
        let Some(force) =
            crate::learning::ranges_from_segment_readings(&readings, full_reading.len())
        else {
            return Ok(None); // 連結不一致 (壊れた状態) は再変換しない
        };
        let mut seg = backend.convert_segmented_forced(&full_reading, &force)?;
        if seg.is_empty() {
            return Ok(None);
        }
        self.apply_segment_layers(backend, &mut seg);
        Ok(Some(seg))
    }

    /// 文全体の「ん＋母音」代替読みのうち **flat 変換の文全体最良スコアが原文を上回る**
    /// 読みがあれば、その読みで再分割した結果を採る。
    ///
    /// 文節をまたぐ「ん｜母音」境界 (ご確認｜いただけ) を救うための処理。原文の読みを
    /// 基準に、[`vowel_boundary_alternate_readings`] が返す各代替 — **ん+な行→ん+母音**
    /// (nn打ち にんに) と **な行→ん+母音** (単一n打ち にに) の両方 — の採否を決める。
    ///
    /// 判定指標は **flat 変換 ([`LibakazaBackend::convert`]) の文全体 最良候補スコア**
    /// (= libakaza Viterbi の最良パススコア) を読みごとに比べる。各代替で原文 flat 最良を
    /// **厳密に上回る** もののうち最高スコアの読みを選び、それで `convert_segmented` し直す。
    /// どれも超えなければ原文の分割を維持する (残念/案内/私の名前 等の回帰を防ぐ)。
    ///
    /// 旧実装は文節スコア合計 ([`sentence_score`]) で判定していたが、これは文全体の正しさの
    /// 近似にならず「くりーんなほうで→クリーン阿呆で」のように **正当な ん+な を書き換える
    /// 誤採用** を招いた (2026-11 実機)。flat は両読みを同じ土俵で比較でき、
    /// 「クリーンな方で 95414 > クリーン阿呆で 94746」「ご確認いただけ 93581 > にただけ 90084」
    /// と正しく分離できる。
    #[cfg(feature = "akaza")]
    fn pick_best_nn_alternate_segmentation(
        &self,
        backend: &LibakazaBackend,
        reading: &str,
        original: SegmentedConversion,
    ) -> Result<SegmentedConversion> {
        // 広め生成: ん+な行→ん+母音 (nn打ち) と な行→ん+母音 (単一n打ち) の両方。
        // 原文は含まないので skip 不要。
        let alts = vowel_boundary_alternate_readings(reading);
        if alts.is_empty() {
            return Ok(original); // な行 が無い = 代替なし
        }
        // flat 変換の「文全体 最良候補スコア」。候補は降順だが念のため max を取る。
        let flat_best = |r: &str| -> Option<i64> {
            backend
                .convert(r)
                .ok()?
                .iter()
                .map(|c| i64::from(c.score))
                .max()
        };
        let Some(orig_best) = flat_best(reading) else {
            return Ok(original); // 原文 flat が取れない = 判定不能、原文維持
        };
        // 閾値を原文スコアから開始し、厳密に上回る代替だけを採る (同点は原文維持)。
        let mut best_alt: Option<&String> = None;
        let mut best_alt_score = orig_best;
        for alt in &alts {
            let Some(score) = flat_best(alt) else {
                continue;
            };
            if score > best_alt_score {
                best_alt_score = score;
                best_alt = Some(alt);
            }
        }
        let Some(alt) = best_alt else {
            return Ok(original); // どの代替も原文 flat を超えない = 原文維持
        };
        match backend.convert_segmented(alt) {
            Ok(seg) if !seg.is_empty() => Ok(seg),
            _ => Ok(original), // 代替の再分割失敗は原文維持
        }
    }

    /// 各文節の読みに「ん+な行」があれば、「ん+母音」の代替読みも libakaza で変換し、
    /// その候補を文節の **末尾に追加** する (重複 surface と代替読みそのまま=かな は除く)。
    ///
    /// flat の [`Self::convert`] が原文＋代替読みをマージするのと同じ思想を segmented にも
    /// 適用する。ただし **並べ替えず末尾に足すだけ** にして既存の順序・選択を保つ
    /// (= 残念(ざんねん)/案内(あんない) の正解を押し下げない。ランキング昇格は
    /// モデル実機検証を伴う follow-up で行う)。
    #[cfg(feature = "akaza")]
    fn merge_nn_alternates_into_segments(
        &self,
        backend: &LibakazaBackend,
        segmented: &mut super::SegmentedConversion,
    ) {
        /// 1 文節あたり 1 代替読みから足す候補の上限 (パネル肥大化を防ぐ)。
        const MAX_ALT_CANDS_PER_SEG: usize = 3;
        for seg in &mut segmented.segments {
            let alts = nn_alternate_readings(&seg.reading);
            // alts[0] は原文。代替 (1..) だけを変換して候補末尾に足す。
            for alt in alts.iter().skip(1) {
                let Ok(cands) = backend.convert(alt) else {
                    continue; // 代替変換失敗は無視 (原文候補は維持)
                };
                let mut added = 0usize;
                for mut c in cands {
                    if added >= MAX_ALT_CANDS_PER_SEG {
                        break;
                    }
                    // 代替読みそのまま (かな) はノイズなので入れない。
                    if c.surface == *alt {
                        continue;
                    }
                    // 既存 surface と重複は入れない。
                    if seg.candidates.iter().any(|x| x.surface == c.surface) {
                        continue;
                    }
                    // 学習は入力 (文節) 読みに紐づける。flat 側と同じ理由
                    // (代替読み しんい のままだと しんに で学習を引けない)。
                    c.reading.clone_from(&seg.reading);
                    seg.candidates.push(c);
                    added += 1;
                }
                // 代替読みが **実在語** (全体 1 語の漢字辞書語) のときだけ、その読みの
                // **ひらがな** も候補末尾に足す (2026-11 ユーザー要望:「しんに と打って
                // 真意=漢字だけでなく ひらがな しんい も出したい」)。
                // ん+母音 はローマ字で直接打てない (しんい は shinni=しんに になる) ため、
                // かな しんい は候補に出さないと一生選べない。実在語判定で おんな→おんあ /
                // こんにちは→こんいちは のようなノイズは出さない (= false なので足さない)。
                if backend.reading_is_single_dict_word(alt)
                    && !seg.candidates.iter().any(|x| x.surface == *alt)
                {
                    // surface=かな(しんい)、学習キーは入力読み(しんに)。一度選べば
                    // Layer0 頻度学習で次回先頭に上がる (#160 と同じ帰属)。
                    let mut kana = Candidate::new(alt, &seg.reading);
                    kana.source = CandidateSource::System;
                    seg.candidates.push(kana);
                }
            }
        }
    }

    /// **学習した文節位置** (ユーザーが切り直した切り方) を読み全体一致で強制再適用する。
    ///
    /// `segmentations` に `reading` ちょうどの切り方があれば、その各文節読みから
    /// `force_ranges` を作って `convert_segmented_forced` で再変換する。無ければ `None`。
    /// = 本人が Shift で直した区切りを最優先で尊重する (2026-10)。
    #[cfg(feature = "akaza")]
    fn resegment_from_learned(
        &self,
        backend: &LibakazaBackend,
        reading: &str,
    ) -> Result<Option<SegmentedConversion>> {
        let Some(segs) = self.segmentations.lookup(reading) else {
            return Ok(None);
        };
        let Some(force) = crate::learning::ranges_from_segment_readings(segs, reading.len()) else {
            return Ok(None); // 連結不一致 (壊れた学習) は無視
        };
        let re = backend.convert_segmented_forced(reading, &force)?;
        if re.is_empty() {
            return Ok(None);
        }
        Ok(Some(re))
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
        if prefix.is_empty() || max_results == 0 {
            return Ok(Vec::new());
        }

        let mut predictions = Vec::new();
        // 同一表層の重複を防ぐ (フレーズ予測と辞書で同じ語が出ることがある)。
        let mut seen = std::collections::HashSet::new();

        // 1) フレーズ予測 (#187)。確定済みの定型句を読み全体の前方一致で先頭に出す。
        //    これが「ん＋母音」境界を打ち切る前に候補選択で回避する肝 (= 既存IME の
        //    予測変換と同じ回避策)。個人学習由来なので source = Learned。
        for entry in self.phrases.predict(prefix, max_results) {
            if !seen.insert(entry.surface.clone()) {
                continue;
            }
            let candidate = Candidate::new(entry.surface, entry.reading.clone())
                .with_score(i32::try_from(entry.count).unwrap_or(i32::MAX))
                .with_source(CandidateSource::Learned);
            predictions.push((entry.reading, candidate));
            if predictions.len() >= max_results {
                return Ok(predictions);
            }
        }

        // 2) 前方一致で静的辞書を検索
        let results = self.dictionary.prefix_search(prefix)?;
        for (reading, candidates) in results {
            for candidate in candidates {
                if !seen.insert(candidate.surface.clone()) {
                    continue;
                }
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
        // 日付/時刻の生成候補 (きょう→2026年…/いま→10:30 等) は **一過性** なので
        // 頻度学習 (Layer 0) に**リテラルは**記録しない。記録すると翌日以降も確定時の古い
        // 日付が今日の日付より上位に固定される (2026-11 ユーザー報告「きょう→2026年10月20日」)。
        // 候補自体は毎回 now で生成される (`super::datetime`) ので学習不要。
        // ただし「どの**形式**を選んだか」(ymd_slash 等) は安定した選好なので記録する
        // (リテラルでなく形式を覚えるので古い日付問題は再発しない)。
        if super::datetime::is_datetime_surface(&candidate.reading, &candidate.surface) {
            if let Some(fmt) = super::datetime::classify_datetime_format(&candidate.surface) {
                self.date_formats.record(fmt);
                if let Some(path) = &self.date_formats_path {
                    if let Err(e) = self.date_formats.save(path) {
                        tracing::warn!(error = %e, "date_format.json の保存に失敗");
                    }
                }
            }
            return Ok(());
        }
        self.learning.record(candidate, context)?;

        // 文脈つき候補学習 (#228 残件③/A): 直前文節 (context.prev_words の末尾) のもとで
        // この (読み, 表層) を選んだ回数を記録する。これで「どうしようかな の中の しよう は
        // しよう、仕様書の文脈では 仕様」を両立できる。prev が無い (文頭) なら記録しない。
        // 記録は Layer0 の record と独立 (ContextFrequencyStore = churn-free・加法的)。
        if let Some(prev) = context.prev_words.last() {
            self.context_freq
                .record(&candidate.reading, &candidate.surface, prev);
            if let Some(path) = &self.context_freq_path {
                if let Err(e) = self.context_freq.save(path) {
                    tracing::warn!(error = %e, "context_freq.json の保存に失敗");
                }
            }
        }

        // 学習モードに応じて確定ブーストを積む (#183 スコア可視化の次段 / 王冠 2026-11-21)。
        // - Crown (既定): 上限付き王冠。他表層の王冠を剥奪して「最後に確定した 1 つ」だけを
        //   1 位へ。convert 不要 (相手の base だけ見る = ratchet しない)。暴走の根治。
        // - Strong/Medium: 従来の累積・追い越し。同じ読みの**他候補の最大“合成”スコア**
        //   (libakaza/Layer2 込み) を再変換で求め、それを恒久的に上回るまで引き上げる。
        //   集合知的に上がる挙動を好むユーザー向けの選択肢 (単調増加なので青天井)。
        // - Weak: 頻度のみ (+10)。ブーストしない。
        use crate::learning::LearningStrength;
        match self.learning.strength() {
            LearningStrength::Weak => {}
            LearningStrength::Crown => self.learning.apply_crown_boost(candidate),
            LearningStrength::Medium | LearningStrength::Strong => {
                let rival_top = self
                    .convert(&candidate.reading, context)
                    .ok()
                    .and_then(|cands| {
                        cands
                            .iter()
                            .filter(|c| c.surface != candidate.surface)
                            .map(|c| c.score)
                            .max()
                    });
                self.learning.apply_selection_boost(candidate, rival_top);
            }
        }

        // 学習データの永続化が設定されていれば自動 save。失敗は warn にとどめ
        // commit 自体は成功扱い (= 学習はメモリには載った)。
        if self.learning.has_path() {
            if let Err(e) = self.learning.save() {
                tracing::warn!(error = %e, "学習データ save 失敗 (in-memory のみ保持)");
            }
        }
        Ok(())
    }

    /// 学習モード (= `LearningStrength`) を設定する。
    ///
    /// モード選択 UI / 永続化 (`LEARNING_MODE` マーカー) から起動時・切替時に呼ぶ。
    /// 既定は `Crown` (王冠・暴走なし)。
    pub fn set_learning_strength(&mut self, strength: crate::learning::LearningStrength) {
        self.learning.set_strength(strength);
    }

    /// **教える確定** (Shift+Enter)。通常 commit と同じく記録するが、モードに関わらず
    /// 選んだ表層を**絶対的に 1 位**へ引き上げる (王冠の天井も無視し、訂正tierすら超える)。
    ///
    /// 「この候補を最優先にしろ」というユーザーの明示操作。間違えたら「学習を忘れる」
    /// (`demote_frequency` を呼ぶ) か ⌥← / 点数指定で下げられる。datetime リテラルは
    /// 通常 commit 同様に学習しない。
    pub fn commit_teach(
        &mut self,
        candidate: &Candidate,
        context: &ConversionContext,
    ) -> Result<()> {
        // datetime リテラルは「教える」でも学習しない (翌日以降に古い日付が残るため)。
        // 形式の記録は通常 commit に委ねる (教える対象は漢字変換語が主)。
        if super::datetime::is_datetime_surface(&candidate.reading, &candidate.surface) {
            return self.commit(candidate, context);
        }
        self.learning.record(candidate, context)?;
        let rival_top = self
            .convert(&candidate.reading, context)
            .ok()
            .and_then(|cands| {
                cands
                    .iter()
                    .filter(|c| c.surface != candidate.surface)
                    .map(|c| c.score)
                    .max()
            });
        self.learning.apply_teach_boost(candidate, rival_top);
        if self.learning.has_path() {
            if let Err(e) = self.learning.save() {
                tracing::warn!(error = %e, "教える確定の学習 save 失敗 (in-memory のみ保持)");
            }
        }
        Ok(())
    }

    /// 学習データをクリア
    pub fn clear_learning_data(&mut self) -> Result<()> {
        self.learning.clear()
    }

    /// 指定の読み＋表層を**降格**する (追い越しブーストを除去して頻度ベースへ戻す)。
    ///
    /// 間違って確定し続けて膨張した表層 (実機で 500k〜800k) を下げ、打ち直し訂正
    /// (Layer2 +300_000) が確実に勝つようにする = 悪循環ループの根治 (2026-11 森岡さん)。
    /// 名指しした (reading, surface) だけ下げるのでグローバルな再ランクはしない。
    /// 降格できたら learning.json を保存する。
    ///
    /// 戻り値: 実際に降格した (エントリ存在かつ boost が 0 でなかった) なら `true`。
    pub fn demote_frequency(&mut self, reading: &str, surface: &str) -> bool {
        let changed = self.learning.demote(reading, surface);
        if changed && self.learning.has_path() {
            if let Err(e) = self.learning.save() {
                tracing::warn!(error = %e, "降格後の学習データ save 失敗 (in-memory のみ)");
            }
        }
        changed
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

/// 記号の「代表 1 つ」を page 1 (= 2 位) に引き上げる (①規則クラス)。
///
/// # 背景 (2026-11、実測)
/// 記号 (まる→◯ / さんかく→△ / ほし→★ 等) は静的辞書で素スコア ~100。libakaza 候補は
/// `LIBAKAZA_PRIORITY_BOOST` (+100_000) で 94_000〜99_000 に並ぶため、記号は常にその下
/// ＝ 2 ページ目送りになり「◯ が出てこない」とユーザーに見える (まる で ◯ は 11 位だった)。
///
/// # 方針
/// その読みの **代表記号 (= `pos=="記号"` のうち最大スコア = 静的辞書の先頭 i==0) だけ** を
/// 「非記号の最大スコア − 1」へ引き上げ、#1 (通常の漢字/かな) は動かさず #2 に出す。
///
/// # 設計上の性質
/// - #1 は不変 → 下/右 のような常用漢字の 1 位は維持される (回帰しない)。
/// - 異体字 (● ◎ 〇 ▲ ☆ 等) は据え置き (2 ページ目のまま)。学習が後段で実選択を浮かせる。
/// - 非記号候補が無い/低い (libakaza OFF 等) ときは代表が自然に上位なので無変化。
/// - ユーザー辞書/訂正ブーストはこの後段に乗るので、それらは引き続き記号を上回れる。
fn boost_representative_symbol(candidates: &mut CandidateList) {
    let is_symbol = |c: &Candidate| c.pos.as_deref() == Some("記号");
    let rep_score = candidates
        .iter()
        .filter(|c| is_symbol(c))
        .map(|c| c.score)
        .max();
    let max_real = candidates
        .iter()
        .filter(|c| !is_symbol(c))
        .map(|c| c.score)
        .max();
    let (Some(rep_score), Some(max_real)) = (rep_score, max_real) else {
        return;
    };
    let target = max_real.saturating_sub(1);
    if rep_score >= target {
        return;
    }
    // 最大スコアの記号 1 件だけを引き上げる (異体字は触らない)。
    let mut boosted = false;
    for c in candidates.iter_mut() {
        if !boosted && is_symbol(c) && c.score == rep_score {
            c.score = target;
            boosted = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 候補キュレーション (疑似語の降格) の純粋関数テスト ----

    #[test]
    fn is_pseudo_word_distinguishes_real_from_transliteration() {
        assert!(!is_pseudo_word("任意"), "漢字=実語");
        assert!(!is_pseudo_word("Re・Act"), "英字混じり=実語");
        assert!(is_pseudo_word("にんに"), "ひらがな=疑似語");
        assert!(is_pseudo_word("ニンニ"), "全角カナ=疑似語");
        assert!(is_pseudo_word("ﾆﾝﾆ"), "半角カナ=疑似語");
        assert!(
            is_pseudo_word("リアクト"),
            "純カタカナ外来語も疑似語扱い (降格は top で守る)"
        );
        assert!(!is_pseudo_word(""), "空は疑似語でない");
    }

    #[test]
    fn is_halfwidth_kana_word_matches_only_halfwidth() {
        assert!(is_halfwidth_kana_word("ﾆﾝｲ"));
        assert!(!is_halfwidth_kana_word("ニンイ"), "全角カナは半角でない");
        assert!(!is_halfwidth_kana_word("任意"));
    }

    /// 「にんに→任意」: 最上位が実語なら ニンニ/にんに/ﾆﾝﾆ/ニンイ/ﾆﾝｲ が実語の下へ沈む。
    #[test]
    fn curate_demotes_pseudo_when_top_is_real_word() {
        let cand = |s: &str, score: i32| Candidate::new(s, "にんに").with_score(score);
        // 実測 (スクショ相当) の並び。
        let mut cands = vec![
            cand("任意", 500_109),
            cand("ニンニ", 97_803),
            cand("にんに", 97_129),
            cand("ﾆﾝﾆ", 97_129),
            cand("ニンイ", 97_129),
            cand("ﾆﾝｲ", 97_129),
            cand("任に", 95_764),
            cand("忍に", 95_630),
        ];
        curate_pseudo_words(&mut cands);

        assert_eq!(cands[0].surface, "任意", "実語が最上位のまま");
        assert_eq!(cands[1].surface, "任に", "次も実語 (疑似語が割り込まない)");
        assert_eq!(cands[2].surface, "忍に", "実語が続く");
        // 疑似語はすべて実語より下 (= 2 ページ目相当) に沈む。
        for s in ["ニンニ", "にんに", "ﾆﾝﾆ", "ニンイ", "ﾆﾝｲ"] {
            let c = cands.iter().find(|c| c.surface == s).unwrap();
            assert!(c.score <= 0, "{s} は降格され負値帯 (score={})", c.score);
            let real_min = cands
                .iter()
                .filter(|c| !is_pseudo_word(&c.surface))
                .map(|c| c.score)
                .min()
                .unwrap();
            assert!(c.score < real_min, "{s} は全ての実語より下");
        }
    }

    /// 外来語ガード: 最上位が疑似語 (リアクト) なら全角カナ・ひらがなは触らない (F7 強要回避)。
    /// 半角カナだけは常に沈める。
    #[test]
    fn curate_keeps_katakana_when_top_is_loanword() {
        let cand = |s: &str, score: i32| Candidate::new(s, "りあくと").with_score(score);
        let mut cands = vec![
            cand("リアクト", 97_000), // 最上位が疑似語 (= 答え)
            cand("りあくと", 96_000),
            cand("ﾘｱｸﾄ", 95_000),     // 半角は常に沈める
            cand("リア久斗", 88_000), // junk 実語
        ];
        curate_pseudo_words(&mut cands);

        assert_eq!(cands[0].surface, "リアクト", "外来語カタカナは最上位を維持");
        let half = cands.iter().find(|c| c.surface == "ﾘｱｸﾄ").unwrap();
        assert!(half.score <= 0, "半角カナは top が外来語でも降格");
        // 全角カタカナ (リアクト) は高スコアのまま。
        assert!(
            cands
                .iter()
                .find(|c| c.surface == "リアクト")
                .unwrap()
                .score
                > 0,
            "全角カタカナは未知語ガードで保全"
        );
    }

    /// segmented 経路の帯 = libakaza の **生スコア (大きな負値)**。固定値への降格では実語より
    /// 上に浮く不具合 (任意/にんい/任い が全て負値、任い=-5578 < 固定降格値 -100) を penalty 方式で防ぐ。
    /// これが「flat は直ったが segmented の診断に にんい/ニンイ/ﾆﾝｲ が残る」の回帰 pin (実測 2026-11-22)。
    #[test]
    fn curate_demotes_pseudo_in_negative_score_band() {
        let cand = |s: &str, score: i32| Candidate::new(s, "にんい").with_score(score);
        // segmented の実測相当 (全候補が負値)。
        let mut cands = vec![
            cand("任意", -2871),
            cand("にんい", -2871),
            cand("ニンイ", -2871),
            cand("ﾆﾝｲ", -2871),
            cand("任い", -5578),
        ];
        curate_pseudo_words(&mut cands);

        assert_eq!(cands[0].surface, "任意", "実語が最上位");
        assert_eq!(
            cands[1].surface, "任い",
            "負値の実語 (-5578) も疑似語より上に残る"
        );
        for s in ["にんい", "ニンイ", "ﾆﾝｲ"] {
            let c = cands.iter().find(|c| c.surface == s).unwrap();
            assert!(
                c.score < -5578,
                "{s} は固定値でなく penalty で引かれ、負値の実語 任い(-5578) より下 (score={})",
                c.score
            );
        }
        // 候補は消えていない (降格のみ)。
        assert_eq!(cands.len(), 5, "降格は候補を消さない");
    }

    /// 意図的に低スコアで足した代替読みのかな (ALT_READING_KANA_SCORE、しんに→しんい) は、
    /// top が実語 (真意) なら下へ送られるが**候補には残る** (選べる = 機能は保たれる)。
    #[test]
    fn curate_demotes_but_keeps_intentional_alt_kana() {
        let mut cands = vec![
            Candidate::new("真意", "しんに").with_score(200_000),
            Candidate::new("しんい", "しんに").with_score(-70), // = ALT_READING_KANA_SCORE
        ];
        curate_pseudo_words(&mut cands);
        assert_eq!(cands[0].surface, "真意", "実語が最上位");
        let kana = cands.iter().find(|c| c.surface == "しんい");
        assert!(kana.is_some(), "かな候補は消えず残る (選択可能)");
        assert!(kana.unwrap().score < 0, "ただし実語より下");
    }

    // ---- 手動スコア調整 (#228, ⌥←→) の segmented 適用テスト ----

    #[test]
    fn apply_segment_manual_scores_reorders_by_bias() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::learning::ManualScoreStore;
        let mut store = ManualScoreStore::new();
        store.nudge("ほう", "方", 50_000); // 方 を大きく上げる
        let cand = |s: &str, score: i32| Candidate::new(s, "ほう").with_score(score);
        let mut segmented = SegmentedConversion::new(vec![Segment::new(
            "ほう",
            vec![cand("法", 100), cand("方", 50)],
        )]);
        apply_segment_manual_scores(&mut segmented, &store);
        assert_eq!(
            segmented.segments[0].surface(),
            Some("方"),
            "手動バイアスで 方 が先頭に上がる"
        );
    }

    #[test]
    fn apply_segment_manual_scores_noop_when_empty() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::learning::ManualScoreStore;
        let cand = |s: &str, score: i32| Candidate::new(s, "ほう").with_score(score);
        let mut segmented = SegmentedConversion::new(vec![Segment::new(
            "ほう",
            vec![cand("法", 100), cand("方", 50)],
        )]);
        apply_segment_manual_scores(&mut segmented, &ManualScoreStore::new());
        assert_eq!(
            segmented.segments[0].surface(),
            Some("法"),
            "空ストアは無変化 (元の先頭 法 のまま)"
        );
    }

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

    /// 記号ブースト: 代表記号 1 つだけが「非記号の最大 − 1」= 2 位に上がり、
    /// #1 (漢字) は不変、異体字 (●) は据え置き。(B / 2026-11)
    #[test]
    fn test_symbol_boost_puts_representative_second() {
        let sym =
            |s: &str, score: i32| Candidate::new(s, "まる").with_score(score).with_pos("記号");
        let mut list = CandidateList::new();
        list.push(
            Candidate::new("丸", "まる")
                .with_score(98932)
                .with_pos("名詞"),
        );
        list.push(sym("○", 100)); // 代表 (最大スコアの記号)
        list.push(sym("●", 99)); // 異体字
        list.push(Candidate::new("まる", "まる").with_score(-100)); // かな (pos None)

        boost_representative_symbol(&mut list);

        let score = |surface: &str| list.iter().find(|c| c.surface == surface).unwrap().score;
        assert_eq!(score("○"), 98931, "代表は 非記号最大(98932)-1 = 2 位");
        assert_eq!(score("丸"), 98932, "#1 の漢字は不変");
        assert_eq!(score("●"), 99, "異体字は据え置き");

        list.sort_by_score();
        let order: Vec<&str> = list.iter().map(|c| c.surface.as_str()).collect();
        assert_eq!(order[0], "丸", "漢字が 1 位のまま");
        assert_eq!(order[1], "○", "代表記号が 2 位 (page 1 に出る)");
    }

    /// 記号しか無い (= libakaza OFF 等で非記号が無い) ときは代表を触らない。
    #[test]
    fn test_symbol_boost_noop_without_real_candidate() {
        let mut list = CandidateList::new();
        list.push(Candidate::new("○", "まる").with_score(100).with_pos("記号"));
        list.push(Candidate::new("●", "まる").with_score(99).with_pos("記号"));
        boost_representative_symbol(&mut list);
        assert_eq!(list.iter().find(|c| c.surface == "○").unwrap().score, 100);
    }

    /// 代表が既に非記号より上なら引き下げない (単調性)。
    #[test]
    fn test_symbol_boost_noop_when_already_top() {
        let mut list = CandidateList::new();
        list.push(Candidate::new("○", "まる").with_score(500).with_pos("記号"));
        list.push(
            Candidate::new("丸", "まる")
                .with_score(100)
                .with_pos("名詞"),
        );
        boost_representative_symbol(&mut list);
        assert_eq!(list.iter().find(|c| c.surface == "○").unwrap().score, 500);
    }

    /// ★ フレーズ予測 (#187)。確定済みの定型句が、末尾ん (= 単一 n で打てる) の
    /// 短い前置きで予測候補の先頭に出る。= 「ん＋母音」境界の回避。
    #[test]
    fn predict_surfaces_learned_phrase_first() {
        let mut engine = ConversionEngine::new().unwrap();
        let mut store = PhraseStore::new();
        store.record(
            "ごかくにんいただけますとさいわいです",
            "ご確認いただけますと幸いです",
        );
        engine.set_phrases(store);

        // 末尾ん で引ける短い前置きで、長い表層が先頭に補完される。
        let hits = engine.predict("ごかくにん", 9).unwrap();
        assert!(!hits.is_empty(), "フレーズが予測されるべき");
        assert_eq!(hits[0].1.surface, "ご確認いただけますと幸いです");
        assert_eq!(hits[0].1.source, CandidateSource::Learned, "出所は学習");
    }

    /// フレーズ無しでも静的辞書の予測は従来どおり動く (回帰防止)。
    #[test]
    fn predict_without_phrases_is_unchanged() {
        let engine = ConversionEngine::new().unwrap();
        // 空 prefix / max=0 は空。
        assert!(engine.predict("", 9).unwrap().is_empty());
        assert!(engine.predict("にほん", 0).unwrap().is_empty());
    }

    /// ライブ変換 (#229): 空読みは空、非空読みは flat 変換の 1 位表層と一致する
    /// (libakaza 無効ビルドでは convert_segmented が Ok(None) なので常に flat 経路)。
    #[test]
    fn live_convert_matches_flat_top() {
        let engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();
        assert_eq!(engine.live_convert("", &ctx), "", "空読みは空");
        let flat_top = engine
            .convert("わたし", &ctx)
            .unwrap()
            .selected()
            .map(|c| c.surface.clone())
            .expect("flat に選択候補があるはず");
        assert_eq!(
            engine.live_convert("わたし", &ctx),
            flat_top,
            "ライブ表層は flat 1 位と一致 (単一文節/akaza 無効)"
        );
    }

    /// ライブ変換 (#229): 上限 (LIVE_CONVERT_MAX_READING_CHARS) を超える読みは推測変換を
    /// 走らせず生かなをそのまま返す (打鍵レイテンシの上限担保)。上限ちょうどは変換を走らせる。
    #[test]
    fn live_convert_caps_very_long_reading_to_raw_kana() {
        let engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();
        // 上限超え → 生かなをそのまま返す (変換コストを払わない)。
        let too_long: String = "あ".repeat(LIVE_CONVERT_MAX_READING_CHARS + 1);
        assert_eq!(
            engine.live_convert(&too_long, &ctx),
            too_long,
            "上限超えの読みは生かなのまま返す"
        );
        // 上限ちょうど → 生かな素通しの対象外 (通常経路を通る)。
        let at_cap: String = "あ".repeat(LIVE_CONVERT_MAX_READING_CHARS);
        let out = engine.live_convert(&at_cap, &ctx);
        assert!(!out.is_empty(), "上限ちょうどは通常どおり何か返す");
    }

    /// segmented_preferred: 単一文節は false、2 文節で nn 曖昧/訂正/辞書語が無ければ true。
    /// (do_convert と live_convert が共有する経路選択の唯一の真実源)
    #[cfg(feature = "akaza")]
    #[test]
    fn segmented_preferred_rules() {
        use crate::conversion::{Segment, SegmentedConversion};
        let engine = ConversionEngine::new().unwrap();
        let cand = |s: &str, r: &str| {
            Candidate::new(s, r)
                .with_score(0)
                .with_source(CandidateSource::System)
        };
        // 単一文節 → 常に flat (false)
        let single = SegmentedConversion::new(vec![Segment::new("あ", vec![cand("亜", "あ")])]);
        assert!(
            !engine.segmented_preferred("あ", &single),
            "単一文節は false"
        );
        // 2 文節・nn 非曖昧・辞書語/訂正無し → segmented 優先 (true)
        let two = SegmentedConversion::new(vec![
            Segment::new("ねこ", vec![cand("猫", "ねこ")]),
            Segment::new("かわいい", vec![cand("かわいい", "かわいい")]),
        ]);
        assert!(
            engine.segmented_preferred("ねこかわいい", &two),
            "2 文節・非曖昧は segmented 優先"
        );
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

    /// ★★ Layer 0 頻度が **文節ごと** に効き、文中の語でも学習が勝つ。
    /// (2026-11 ユーザー報告:「砲→方 / 用→様 が何度直しても勝てない。勝って欲しい」。
    ///  原因は convert_segmented が頻度を参照していなかったこと。観察ログ opt-in 不要。)
    #[test]
    fn segment_frequency_reorders_within_sentence() {
        use crate::conversion::{Segment, SegmentedConversion};

        // Layer 0: 「ほう→方」を 1 回確定 (頻度学習に載る)
        let mut learning = LearningManager::new().unwrap();
        let ctx = ConversionContext::new();
        learning
            .record(&Candidate::new("方", "ほう"), &ctx)
            .unwrap();

        // 文「…のほう です」の ほう 文節。libakaza 既定は 砲、方 は下位。
        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        let mut segmented = SegmentedConversion::new(vec![
            Segment::new(
                "ほう",
                vec![cand("砲", "ほう", 99_900), cand("方", "ほう", 99_800)],
            ),
            Segment::new("です", vec![cand("です", "です", 0)]),
        ]);

        assert_eq!(
            segmented.segments[0].surface(),
            Some("砲"),
            "適用前は libakaza 既定 砲"
        );

        apply_segment_frequency(&mut segmented, &learning, &ContextFrequencyStore::new());

        assert_eq!(
            segmented.segments[0].surface(),
            Some("方"),
            "★ 一度確定した 方 が文中でも 1 位 (頻度 200_000+ が libakaza 既定を上回る)"
        );
        assert_eq!(
            segmented.segments[1].surface(),
            Some("です"),
            "他文節は不変"
        );
        assert!(
            segmented.corrections_applied,
            "★ 学習が効いたら corrections_applied が立つ (segmented 優先の判定に使う)"
        );
    }

    /// ★ 学習した表層が候補に無くても **注入** されて 1 位になる (辞書/libakaza に無い語)。
    #[test]
    fn segment_frequency_injects_learned_surface_absent_from_candidates() {
        use crate::conversion::{Segment, SegmentedConversion};

        let mut learning = LearningManager::new().unwrap();
        let ctx = ConversionContext::new();
        learning
            .record(&Candidate::new("方", "ほう"), &ctx)
            .unwrap();

        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        // 候補に 方 が無い (砲 のみ) ケース
        let mut segmented =
            SegmentedConversion::new(vec![Segment::new("ほう", vec![cand("砲", "ほう", 99_900)])]);

        apply_segment_frequency(&mut segmented, &learning, &ContextFrequencyStore::new());

        assert_eq!(
            segmented.segments[0].surface(),
            Some("方"),
            "★ 候補に無くても注入して 1 位"
        );
    }

    /// ★ 学習が無ければ既定順のまま・フラグも立たない (回帰ゼロ)。
    #[test]
    fn segment_frequency_noop_without_learning() {
        use crate::conversion::{Segment, SegmentedConversion};

        let learning = LearningManager::new().unwrap();
        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        let mut segmented = SegmentedConversion::new(vec![Segment::new(
            "ほう",
            vec![cand("砲", "ほう", 99_900), cand("方", "ほう", 99_800)],
        )]);

        apply_segment_frequency(&mut segmented, &learning, &ContextFrequencyStore::new());

        assert_eq!(
            segmented.segments[0].surface(),
            Some("砲"),
            "★ 学習無しなら既定 砲 のまま"
        );
        assert!(
            !segmented.corrections_applied,
            "★ 学習が無ければ corrections_applied は立たない"
        );
    }

    /// ★★ 文脈つき学習 (#228/A): 「どうしようかな」= どう|しよう で、仕様 を別文脈で
    /// 何度も確定していても、しよう の直前が どう のとき (しよう,しよう,prev=どう) を
    /// 学習済みなら しよう が勝つ。文脈なしだと 仕様 が勝つ (= 森岡さん報告の再現と根治)。
    #[test]
    fn segment_context_frequency_prefers_contextual_surface() {
        use crate::conversion::{Segment, SegmentedConversion};

        // Layer0: 仕様 を 24 回 (別文脈で) 確定済み → しよう 文節で既定上位になる。
        let mut learning = LearningManager::new().unwrap();
        let ctx = ConversionContext::new();
        for _ in 0..24 {
            learning
                .record(&Candidate::new("仕様", "しよう"), &ctx)
                .unwrap();
        }
        // しよう (平仮名) も使うが回数は少なめ。
        learning
            .record(&Candidate::new("しよう", "しよう"), &ctx)
            .unwrap();

        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        let build = || {
            SegmentedConversion::new(vec![
                Segment::new("どう", vec![cand("どう", "どう", 99_900)]),
                Segment::new(
                    "しよう",
                    vec![cand("しよう", "しよう", 50), cand("仕様", "しよう", 99_800)],
                ),
            ])
        };

        // 文脈なし: Layer0 で 仕様 (count24) が しよう (count1) に勝つ。
        let mut seg_no_ctx = build();
        apply_segment_frequency(&mut seg_no_ctx, &learning, &ContextFrequencyStore::new());
        assert_eq!(
            seg_no_ctx.segments[1].surface(),
            Some("仕様"),
            "文脈なしなら Layer0 で 仕様 が勝つ (= 報告された誤変換)"
        );

        // 文脈あり: (しよう,しよう,prev=どう) を学習済み → しよう が勝つ。
        let mut context_freq = ContextFrequencyStore::new();
        context_freq.record("しよう", "しよう", "どう");
        let mut seg_ctx = build();
        apply_segment_frequency(&mut seg_ctx, &learning, &context_freq);
        assert_eq!(
            seg_ctx.segments[1].surface(),
            Some("しよう"),
            "★ 直前が どう のとき文脈学習した しよう が 1 位 (#228/A 根治)"
        );
        // 訂正tier未満に収まる (打ち直し訂正は文脈より上に残る階層)。
        let shiyou_score = seg_ctx.segments[1]
            .candidates
            .iter()
            .find(|c| c.surface == "しよう")
            .unwrap()
            .score;
        assert!(
            shiyou_score < crate::learning::correction::CORRECTION_BOOST,
            "文脈持ち上げは訂正tier未満 (score={shiyou_score})"
        );
    }

    /// ★ 文脈が別 (prev が一致しない) なら文脈学習は発火せず Layer0 のまま (回帰ゼロ)。
    #[test]
    fn segment_context_frequency_noop_when_prev_differs() {
        use crate::conversion::{Segment, SegmentedConversion};

        let mut learning = LearningManager::new().unwrap();
        let ctx = ConversionContext::new();
        for _ in 0..24 {
            learning
                .record(&Candidate::new("仕様", "しよう"), &ctx)
                .unwrap();
        }
        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        // 直前が 「この」 (どう ではない)。
        let mut segmented = SegmentedConversion::new(vec![
            Segment::new("この", vec![cand("この", "この", 99_900)]),
            Segment::new(
                "しよう",
                vec![cand("しよう", "しよう", 50), cand("仕様", "しよう", 99_800)],
            ),
        ]);
        let mut context_freq = ContextFrequencyStore::new();
        context_freq.record("しよう", "しよう", "どう"); // どう の文脈だけ学習

        apply_segment_frequency(&mut segmented, &learning, &context_freq);
        assert_eq!(
            segmented.segments[1].surface(),
            Some("仕様"),
            "★ prev が この なら どう の文脈学習は効かず Layer0 の 仕様 のまま"
        );
    }

    /// ★ 単語登録 (ユーザー辞書) は **文中 (segmented)** でも最優先で出る (2026-11 け→家 バグ)。
    /// libakaza の文節候補に無い登録語を注入し、USER_DICT_BOOST で 1 位にする。
    #[cfg(feature = "akaza")]
    #[test]
    fn segment_user_dict_injects_and_ranks_first() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::dictionary::{DictionaryManager, UserEntry};

        let mut dict = DictionaryManager::new().unwrap();
        dict.user_dictionary_mut()
            .add(UserEntry::new("家", "け"))
            .unwrap();

        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        // libakaza の「け」文節候補に 家 は無い (毛/け のみ) ケース。
        let mut segmented = SegmentedConversion::new(vec![Segment::new(
            "け",
            vec![cand("毛", "け", 99_900), cand("け", "け", -90)],
        )]);

        apply_segment_user_dict(&mut segmented, &dict);

        assert_eq!(
            segmented.segments[0].surface(),
            Some("家"),
            "★ 登録語 家 を注入して 1 位 (文中でも効く)"
        );
    }

    /// ★★ 回帰ガード (2026-11-15、け→家): Layer0 頻度は確定のたび伸び
    /// (`200_000 + count*10`)、追い越し boost も積まれるので、固定ブースト
    /// (USER_DICT_BOOST+base=200_200) だけだと「高頻度＋boost で伸びたかな け」に
    /// 登録語 家 が負けていた (ユーザー報告「登録したのに文中で け が 1 位」)。
    /// 相対持ち上げ (他候補の最大+1) で頻度に依らず登録語を 1 位にする。
    /// (下の 200_250 は「USER_DICT base 200_200 を超えた高 Layer0」の代表入力値。)
    #[cfg(feature = "akaza")]
    #[test]
    fn segment_user_dict_beats_layer0_frequency() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::dictionary::{DictionaryManager, UserEntry};

        let mut dict = DictionaryManager::new().unwrap();
        dict.user_dictionary_mut()
            .add(UserEntry::new("家", "け"))
            .unwrap();

        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::Learned)
        };
        // け の Layer0 が 200_250 (高頻度＋追い越し boost) = USER_DICT base+boost(200_200) より上。
        let mut segmented = SegmentedConversion::new(vec![Segment::new(
            "け",
            vec![cand("け", "け", 200_250), cand("毛", "け", 200_037)],
        )]);

        apply_segment_user_dict(&mut segmented, &dict);

        assert_eq!(
            segmented.segments[0].surface(),
            Some("家"),
            "★ 登録語 家 は Layer0 頻度 (け 200_250) にも勝って 1 位"
        );
    }

    /// ★ 登録が無い読みでは無変化 (回帰ゼロ)。
    #[cfg(feature = "akaza")]
    #[test]
    fn segment_user_dict_noop_without_entry() {
        use crate::conversion::{Segment, SegmentedConversion};
        use crate::dictionary::DictionaryManager;

        let dict = DictionaryManager::new().unwrap();
        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        let mut segmented = SegmentedConversion::new(vec![Segment::new(
            "け",
            vec![cand("毛", "け", 99_900), cand("け", "け", -90)],
        )]);

        apply_segment_user_dict(&mut segmented, &dict);

        assert_eq!(
            segmented.segments[0].surface(),
            Some("毛"),
            "★ 登録なしは既定 毛 のまま"
        );
    }

    /// ★★ 回帰ガード (2026-11-12、ように→用に): libakaza 基礎が **負値** の高 count 既存候補が、
    /// 低 count の **注入**候補に負けてはいけない。旧実装 (加算) だと基礎の負値を学習に足し込み、
    /// `ように = base(−500) + 200_030 = 199_530` が `用に = 純 200_010` に沈んで逆転していた。
    /// max に直して「既存も注入も純 Layer 0」に揃えることで count 順 (36>4) が守られる。
    /// ※本テストは旧 `saturating_add` 実装では **落ちる** (= 回帰を捕捉する)。
    #[test]
    fn segment_frequency_high_count_existing_beats_low_count_injection() {
        use crate::conversion::{Segment, SegmentedConversion};

        let mut learning = LearningManager::new().unwrap();
        let ctx = ConversionContext::new();
        // ように を 3 回、用に を 1 回確定 (count: ように 3 > 用に 1)。
        for _ in 0..3 {
            learning
                .record(&Candidate::new("ように", "ように"), &ctx)
                .unwrap();
        }
        learning
            .record(&Candidate::new("用に", "ように"), &ctx)
            .unwrap();

        // 文中の「ように」文節。libakaza 基礎は負値 (ように = −500)。用に は候補に無い。
        let cand = |s: &str, r: &str, score: i32| {
            Candidate::new(s, r)
                .with_score(score)
                .with_source(CandidateSource::System)
        };
        let mut segmented = SegmentedConversion::new(vec![Segment::new(
            "ように",
            vec![cand("ように", "ように", -500), cand("様に", "ように", -800)],
        )]);

        apply_segment_frequency(&mut segmented, &learning, &ContextFrequencyStore::new());

        assert_eq!(
            segmented.segments[0].surface(),
            Some("ように"),
            "★ 高 count の既存 ように(3) が 低 count の注入 用に(1) に勝つ (旧加算では 用に に逆転していた)"
        );
        // 用に も注入はされるが先頭ではない。
        let top3: Vec<&str> = segmented.segments[0]
            .candidates
            .iter()
            .take(3)
            .map(|c| c.surface.as_str())
            .collect();
        assert_eq!(top3.first(), Some(&"ように"), "★ 先頭は ように");
        assert!(
            top3.contains(&"用に"),
            "用に も候補には注入されている (ただし下位)"
        );
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
        // 単語: はんねい → はんえい (反映/繁栄)。ユーザー報告 2026-10
        // (hannei と打つと標準ローマ字で はんねい になる。反映 は はんえい 読み)。
        let alts3 = nn_alternate_readings("はんねい");
        assert!(
            alts3.contains(&"はんえい".to_string()),
            "★ 単語: ん+ね→ん+え で はんえい を生成"
        );
        // センテンス: これをはんねいする → これをはんえいする (これを反映する)
        let alts4 = nn_alternate_readings("これをはんねいする");
        assert!(
            alts4.contains(&"これをはんえいする".to_string()),
            "★ センテンス: 文中の ん+ね→ん+え も代替生成"
        );
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

    /// ★★ P2 回帰 (2026-10-16 ユーザー報告「たんに と出て 単位(たんい) が出ない」):
    ///
    /// 「ん+母音」の語は `nn_alternate_readings` で代替読みを生成して救う方式。
    /// ここでは **代替生成が正しいこと** と、**壊してはいけない語の原文が保持される**
    /// ことを純粋関数レベルで固定する (= P1「ローマ字フリップ」なら壊れる語の防波堤)。
    ///
    /// 実際に「単位」が候補に出るか (libakaza ランキング) はモデル依存のため
    /// diag/smoke-test で検証する。本テストは「仕組みの入力側」を保証する。
    #[test]
    fn nn_alternate_tani_and_preserves_zannen_annai() {
        // 単位: たんに → たんい を生成 (んに→んい)。これが libakaza で 単位 になる。
        let tani = nn_alternate_readings("たんに");
        assert!(tani.contains(&"たんに".to_string()), "★ 原文 たんに は保持");
        assert!(
            tani.contains(&"たんい".to_string()),
            "★ 代替 たんい を生成 (→ 単位 の読み)"
        );

        // ★ 壊してはいけない: 残念(ざんねん)/案内(あんない) は **原文が必ず残る**。
        //   代替 (ざんえん/あんあい) は追加されるだけで、libakaza が原文を高スコアにする。
        //   P1 (ローマ字で んね→んえ に固定) はこの原文を**消す**ので禁止。
        let zannen = nn_alternate_readings("ざんねん");
        assert!(
            zannen.contains(&"ざんねん".to_string()),
            "★ 残念: 原文 ざんねん は必ず保持 (P1 なら消える = 回帰)"
        );
        let annai = nn_alternate_readings("あんない");
        assert!(
            annai.contains(&"あんない".to_string()),
            "★ 案内: 原文 あんない は必ず保持"
        );

        // 文中 (複数文節相当) の読みでも ん+母音 代替が出る。P2 で segmented にも効かせる対象。
        let sentence = nn_alternate_readings("めーるたんにで");
        assert!(
            sentence.contains(&"めーるたんいで".to_string()),
            "★ 文中の たんに→たんい も代替生成 (P2: これを segmented 変換にも適用する)"
        );
    }

    /// ★ 単一n崩れ (にに) も ん+母音代替を生成する (whole-sentence 用の広い生成)。
    /// これが本命: #192 の nn_alternate_readings (にんに のみ) では救えなかった にに を救う。
    #[test]
    fn vowel_boundary_generates_single_n_recovery() {
        // 単一n: ごかくににただけ (にに) → ごかくにんいただけ (に→んい) を生成する。
        let alts = vowel_boundary_alternate_readings("ごかくににただけますとさいわいです");
        assert!(
            alts.contains(&"ごかくにんいただけますとさいわいです".to_string()),
            "★ 単一n崩れ にに→にんい を生成 (本命): got {alts:?}"
        );
        // nn打ち: ごかくにんにただけ (にんに) → ごかくにんいただけ も生成 (パターン1)。
        let alts2 = vowel_boundary_alternate_readings("ごかくにんにただけますとさいわいです");
        assert!(
            alts2.contains(&"ごかくにんいただけますとさいわいです".to_string()),
            "★ nn打ち にんに→にんい も生成: got {alts2:?}"
        );
        // な行が無ければ代替なし (perf: 余計な変換を走らせない)。
        assert!(vowel_boundary_alternate_readings("あいうえお").is_empty());
        // 原文は含めない (呼び出し側が原文を基準に持つ)。
        assert!(!vowel_boundary_alternate_readings("にほん").contains(&"にほん".to_string()));
        // 上限 (MAX=8) を超えない。
        assert!(vowel_boundary_alternate_readings("なにぬねのなにぬねのなにぬねの").len() <= 8);
    }

    /// ★ 文スコア = 各文節の選択候補 score 合計。異なる分割の比較に使う
    /// (ご確認いただけ vs ご確認にただけ の境界選択)。
    #[test]
    fn sentence_score_sums_selected_candidate_scores() {
        use super::super::{Segment, SegmentedConversion};
        let seg = |reading: &str, surface: &str, score: i32| {
            Segment::new(
                reading,
                vec![Candidate::new(surface, reading).with_score(score)],
            )
        };
        // 文スコア = 100 + 200 + (-50) = 250
        let s = SegmentedConversion::new(vec![
            seg("ごかくにん", "ご確認", 100),
            seg("いただけますと", "いただけますと", 200),
            seg("さいわいです", "幸いです", -50),
        ]);
        assert_eq!(sentence_score(&s), 250);
        // 空は 0。
        assert_eq!(sentence_score(&SegmentedConversion::new(vec![])), 0);
        // 「正しい境界」の方が高スコア = 採用される、を模した比較。
        let wrong =
            SegmentedConversion::new(vec![seg("ごかくにんにただけ", "ご確認にただけ", 90084)]);
        let right =
            SegmentedConversion::new(vec![seg("ごかくにんいただけ", "ご確認いただけ", 93581)]);
        assert!(
            sentence_score(&right) > sentence_score(&wrong),
            "正しい境界 (いただけ) の方が文スコアが高い"
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

    /// ★ 日付/時刻の生成候補は頻度学習しない (2026-11「きょう→古い日付」根治)。
    ///
    /// datetime トリガー読みで数字付き表層を確定しても学習されず、次の変換に
    /// 出てこない (候補は毎回 now で生成されるため)。一方、datetime でない読みの
    /// 数字付き表層は従来どおり学習される (ガードが広すぎないことの確認)。
    #[test]
    fn datetime_commit_is_not_learned_but_normal_is() {
        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();

        // (1) 日付を確定しても学習されない → 翌日以降に出てこない。
        let date = Candidate::new("2099年1月1日", "きょう");
        engine.commit(&date, &ctx).unwrap();
        let after = engine.convert("きょう", &ctx).unwrap();
        assert!(
            !after.iter().any(|c| c.surface == "2099年1月1日"),
            "確定した古い日付は学習されず候補に残らない"
        );

        // (2) datetime でない読みの数字付き表層は従来どおり学習される。
        let normal = Candidate::new("猫2", "ねこ");
        engine.commit(&normal, &ctx).unwrap();
        let neko = engine.convert("ねこ", &ctx).unwrap();
        assert!(
            neko.iter().any(|c| c.surface == "猫2"),
            "datetime でない読みの学習は維持される (ガードが広すぎない)"
        );
    }

    /// ★ 王冠モード (既定) の commit→convert end-to-end 検証。
    ///
    /// 「確定した候補が一気に 1 位へ来る」思想を保ちつつ、交互確定でも訂正tier(300k)
    /// 未満に収まり**暴走しない**こと (旧 Strong は 500k 超へ膨張していた) を実経路で確認。
    #[test]
    fn crown_commit_puts_last_committed_on_top_and_stays_bounded() {
        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();
        let you = Candidate::new("用", "よう");
        let sama = Candidate::new("様", "よう");

        let top = |engine: &ConversionEngine| -> Candidate {
            engine
                .convert("よう", &ctx)
                .unwrap()
                .iter()
                .max_by_key(|c| c.score)
                .cloned()
                .unwrap()
        };

        // 用 を多用 (count=20)。この時点では 用 が 1 位。
        for _ in 0..20 {
            engine.commit(&you, &ctx).unwrap();
        }
        assert_eq!(top(&engine).surface, "用", "多用した 用 が一旦 1 位");

        // 様 を 1 回確定 → 王冠が 様 へ移り、一発で 1 位。訂正tier未満。
        engine.commit(&sama, &ctx).unwrap();
        let after_sama = top(&engine);
        assert_eq!(after_sama.surface, "様", "最後に確定した 様 が 1 位へ");
        assert!(
            after_sama.score < 300_000,
            "王冠は訂正tier未満 (={})",
            after_sama.score
        );

        // 用 に戻しても王冠が移るだけで暴走しない。
        engine.commit(&you, &ctx).unwrap();
        let back = top(&engine);
        assert_eq!(back.surface, "用", "確定した 用 が再び 1 位");
        assert!(
            back.score < 300_000,
            "交互確定でも訂正tier未満 = 暴走しない (={})",
            back.score
        );
    }

    /// ★ 日付フォーマット選好: commit は**リテラル日付は学習しないが、形式は記録する**。
    /// (バグ報告「yyyy/mm/dd を何度選んでもスコアが上がらない」への対応の記録側。)
    #[test]
    fn datetime_commit_records_format_preference_not_literal() {
        use crate::conversion::datetime::classify_datetime_format;
        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();

        // スラッシュ形式の日付を確定。
        let slash = Candidate::new("2026/10/20", "きょう");
        assert_eq!(classify_datetime_format("2026/10/20"), Some("ymd_slash"));
        engine.commit(&slash, &ctx).unwrap();

        // 形式 (ymd_slash) は選好として記録される。
        assert_eq!(
            engine.date_formats().count("ymd_slash"),
            1,
            "選んだ形式 ymd_slash が記録される"
        );
        // だがリテラル日付は学習されない (翌日以降に古い日付が残らない)。
        let after = engine.convert("きょう", &ctx).unwrap();
        assert!(
            !after.iter().any(|c| c.surface == "2026/10/20"),
            "リテラル日付は学習されず候補に残らない"
        );
    }

    /// ★★ 日付フォーマット選好: 選んだ形式が**日付候補グループの先頭**に来る。
    /// バグ報告「yyyy/mm/dd を何度選んでもスコアが上がらない」を解消する本体。
    #[test]
    fn date_format_preference_ranks_picked_format_first() {
        use crate::conversion::datetime::classify_datetime_format;
        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();

        // 選好前: 日付候補は全部同点 (base 50)。= まだ選好差が無い。
        let before = engine.convert("きょう", &ctx).unwrap();
        let before_scores: Vec<i32> = before
            .iter()
            .filter(|c| classify_datetime_format(&c.surface).is_some())
            .map(|c| c.score)
            .collect();
        assert!(!before_scores.is_empty(), "日付候補が生成される");
        assert!(
            before_scores.windows(2).all(|w| w[0] == w[1]),
            "選好前は全日付候補が同点 (選好差なし)"
        );

        // ユーザーが ymd_slash を 2 回選ぶ。
        engine.date_formats.record("ymd_slash");
        engine.date_formats.record("ymd_slash");

        // 選好後: ymd_slash が他の全日付形式より厳密に上位になる。
        let after = engine.convert("きょう", &ctx).unwrap();
        let slash_score = after
            .iter()
            .find(|c| classify_datetime_format(&c.surface) == Some("ymd_slash"))
            .map(|c| c.score)
            .expect("ymd_slash 候補が存在する");
        let others_max = after
            .iter()
            .filter(|c| {
                let f = classify_datetime_format(&c.surface);
                f.is_some() && f != Some("ymd_slash")
            })
            .map(|c| c.score)
            .max()
            .expect("他の日付形式も存在する");
        assert!(
            slash_score > others_max,
            "★ 選好した ymd_slash ({slash_score}) が他の日付形式 (最大 {others_max}) より上位"
        );
    }

    /// 加点幅 (count×STEP) と上限を固定する。「+1/回では上がった気がしない」への対応を
    /// 数値で pin し (CODE_COMMENT_DISCIPLINE: 数値の主張はテストで裏付ける)、同時に
    /// 上限込みでも libakaza 帯 (+100_000) を侵さない不変条件を固定する。
    #[test]
    fn date_format_boost_is_step_times_count_and_stays_below_libakaza_band() {
        use crate::conversion::datetime::{classify_datetime_format, DATE_FORMAT_PREFERENCE_STEP};
        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();

        // 未選好の日付候補スコア (= base) を実測で取る (定数を複製しない)。
        let before = engine.convert("きょう", &ctx).unwrap();
        let base = before
            .iter()
            .find(|c| classify_datetime_format(&c.surface).is_some())
            .map(|c| c.score)
            .expect("日付候補");

        // 3 回選ぶと「base + 3×STEP」に上がる (= 1 回ごとに STEP ずつ体感で上がる)。
        for _ in 0..3 {
            engine.date_formats.record("ymd_slash");
        }
        let after = engine.convert("きょう", &ctx).unwrap();
        let slash = after
            .iter()
            .find(|c| classify_datetime_format(&c.surface) == Some("ymd_slash"))
            .map(|c| c.score)
            .expect("ymd_slash 候補");
        assert_eq!(
            slash,
            base + 3 * DATE_FORMAT_PREFERENCE_STEP,
            "加点は count×STEP"
        );

        // 不変条件: base + 上限いっぱいでも libakaza 帯 (+100_000) を超えない。
        // libakaza 定数は akaza feature 限定なので gate する (cross-platform clippy 対策)。
        #[cfg(feature = "akaza")]
        assert!(
            base + crate::conversion::datetime::DATE_FORMAT_PREFERENCE_MAX_BOOST
                < LIBAKAZA_PRIORITY_BOOST,
            "日付加点は上限込みでも libakaza 帯を侵さない (今日/既定を飛び越えない)"
        );
    }

    /// ★ 既に学習済みの古い日付も変換時に自己修復される (データ掃除不要)。
    ///
    /// `commit()` のガードを迂回して学習ストアへ直接古い日付を入れても (= 既存ユーザーの
    /// learning.json に残っている状態を模擬)、convert がそれを除去し、代わりに now の
    /// 日付を注入する。2026-11 ユーザー報告の既存データ即時解消を固定する。
    #[test]
    fn stale_learned_date_is_self_healed_on_convert() {
        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();

        // 学習ストアへ直接注入 (commit ガードを迂回し、既存の汚染状態を作る)。
        let stale = Candidate::new("2026年10月20日", "きょう");
        engine.learning.record(&stale, &ctx).unwrap();

        let out = engine.convert("きょう", &ctx).unwrap();
        assert!(
            !out.iter().any(|c| c.surface == "2026年10月20日"),
            "古い学習済み日付は変換時に除去される"
        );
        // 既定の漢字候補は残る。
        assert!(
            out.iter().any(|c| c.surface == "今日"),
            "今日 (漢字既定) は残る"
        );
    }

    /// ★ 旧字体モード: ON のとき候補に旧字体版が加わり、OFF では加わらない (配線確認)。
    #[test]
    fn kyujitai_mode_adds_old_form_candidate() {
        use crate::dictionary::UserEntry;
        let mut engine = ConversionEngine::new().unwrap();
        engine
            .dictionary_mut()
            .user_dictionary_mut()
            .add(UserEntry::new("国語", "こくご"))
            .unwrap();
        let ctx = ConversionContext::new();

        // OFF (既定): 旧字体 國語 は出ない。
        let off = engine.convert("こくご", &ctx).unwrap();
        assert!(
            !off.iter().any(|c| c.surface == "國語"),
            "OFF では旧字体を足さない"
        );

        // ON: 國語 が候補に加わる。
        engine.set_kyujitai_mode(true);
        let on = engine.convert("こくご", &ctx).unwrap();
        assert!(
            on.iter().any(|c| c.surface == "國語"),
            "ON で旧字体 國語 を追加"
        );
        // 新字体 国語 (単語登録) は依然 1 位 (旧字体に押し下げられない)。
        assert_eq!(on.iter().next().unwrap().surface, "国語");
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

    /// ★★ 学習の end-to-end テスト (2026-10-05 ユーザー質問「学習自体のテストは作れるか」)。
    ///
    /// 「文全体の読み→表層」を 1 回意図的に確定した観察から選好を抽出すると、
    /// convert がその複合語を 1 位に注入する。これは **情報公開課 固有ではなくパターン**
    /// (任意の複合語に効く) であることを、同一コードパスを複数語で回して示す。
    /// 単字曖昧文節 (か→課) を bare で学習できない語を、文脈=複合語として救う仕組み。
    #[test]
    fn learned_compound_is_injected_first_as_general_pattern() {
        use crate::learning::{extract_corrections, ObservationEvent};
        let ctx = ConversionContext::new();

        for (reading, surface) in [
            ("じょうほうこうかいか", "情報公開課"),
            ("こうほうかちょう", "広報課長"),
        ] {
            let mut engine = ConversionEngine::new().unwrap();

            // 前提: 学習前は複合語そのものは 1 位でない (辞書に無い長い複合語)
            let before = engine.convert(reading, &ctx).unwrap();
            assert_ne!(
                before.iter().next().unwrap().surface,
                surface,
                "前提: 学習前に {surface} が既定で出ることはない"
            );

            // controller の「複合語記録」を模す: 文全体を 1 回意図的に確定 (picked>0)
            let events = vec![ObservationEvent::commit_with_candidates(
                reading,
                surface,
                Vec::new(),
                Some(1),
            )];
            engine.set_corrections(extract_corrections(&events, 2));

            let after = engine.convert(reading, &ctx).unwrap();
            assert_eq!(
                after.iter().next().unwrap().surface,
                surface,
                "★ 学習した複合語 {surface} が 1 位 (パターン: 任意語に効く)"
            );
        }
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

    #[test]
    fn alt_reading_pick_is_learned_under_input_reading() {
        // 代替読み (nn 曖昧性) 候補の学習帰属バグの回帰テスト。
        // 「しんに」と打つと真意(代替読み しんい)が候補に出るが、選んで確定したとき
        // 学習が「しんい」でキーされると次回「しんに」で引けず「選んでも効かない」。
        // 修正: convert() が代替読み候補の reading を入力読みに付け替えるので、
        // commit される候補は reading=「しんに」。それを学習すれば次回先頭に来る。
        let mut engine = ConversionEngine::new().unwrap();
        let ctx = ConversionContext::new();
        // ユーザーが「しんに」と打って真意を選び確定した状況 (付け替え後の候補を commit)。
        let picked = Candidate::new("真意", "しんに");
        engine.commit(&picked, &ctx).unwrap();
        // 次回「しんに」で変換 → Layer0 学習 (score 200_010) が libakaza/辞書に勝ち先頭。
        let after = engine.convert("しんに", &ctx).unwrap();
        assert_eq!(
            after.selected().unwrap().surface,
            "真意",
            "★ しんに と打って選んだ真意が次回 1 位 (学習でカバーが効く)"
        );
    }

    /// 実機モデルで「しんに」を変換し、代替読み由来の真意候補の reading が
    /// **入力読み「しんに」** になっていること (= 学習が正しく紐づく) を検証する。
    ///
    /// 実行: `cargo test -p nuko-core --release --features akaza alt_reading_candidate_carries -- --ignored --nocapture`
    #[cfg(feature = "akaza")]
    #[test]
    #[ignore = "実機モデルが要る"]
    fn alt_reading_candidate_carries_input_reading_with_libakaza() {
        let home = std::env::var("HOME").expect("HOME");
        let model = format!("{home}/Library/Application Support/nuko-ime/akaza-model");
        let engine = ConversionEngine::with_libakaza(&model).unwrap();
        if !engine.has_libakaza() {
            eprintln!("モデル未配置のためスキップ: {model}");
            return;
        }
        let result = engine.convert("しんに", &ConversionContext::new()).unwrap();
        let shinni = result
            .iter()
            .find(|c| c.surface == "真意")
            .expect("真意 が代替読み (しんに→しんい) 経由で候補に出るはず");
        assert_eq!(
            shinni.reading, "しんに",
            "★ 代替読み候補の reading は入力読み (学習が しんに に紐づく)"
        );
    }

    /// 代替読みが **実在語** のときだけ、その読みのひらがなを候補に出す。
    /// `しんに`→`しんい`(真意あり) は出し、`こんにちは`→`こんいちは`(語なし) は出さない。
    ///
    /// 実行: `cargo test -p nuko-core --release --features akaza alt_reading_kana -- --ignored --nocapture`
    #[cfg(feature = "akaza")]
    #[test]
    #[ignore = "実機モデルが要る"]
    fn alt_reading_kana_offered_only_for_real_words() {
        let home = std::env::var("HOME").expect("HOME");
        let model = format!("{home}/Library/Application Support/nuko-ime/akaza-model");
        let engine = ConversionEngine::with_libakaza(&model).unwrap();
        if !engine.has_libakaza() {
            eprintln!("モデル未配置のためスキップ: {model}");
            return;
        }
        // しんに: 代替読み しんい は実在語(真意) → ひらがな しんい が候補に出る。
        let shinni = engine.convert("しんに", &ConversionContext::new()).unwrap();
        let kana = shinni
            .iter()
            .find(|c| c.surface == "しんい")
            .expect("ひらがな しんい が候補に出るはず (代替読みが実在語 真意)");
        assert_eq!(
            kana.reading, "しんに",
            "★ かな候補の学習キーは入力読み しんに (一度選べば次回上位化)"
        );
        // こんにちは: 代替読み こんいちは は実在語でない → ひらがな こんいちは は出さない。
        let konnichiha = engine
            .convert("こんにちは", &ConversionContext::new())
            .unwrap();
        assert!(
            !konnichiha.iter().any(|c| c.surface == "こんいちは"),
            "こんいちは はノイズなので候補に出してはいけない"
        );
        // おんな: 代替読み おんあ も非実在語 → 出さない。
        let onna = engine.convert("おんな", &ConversionContext::new()).unwrap();
        assert!(
            !onna.iter().any(|c| c.surface == "おんあ"),
            "おんあ はノイズなので候補に出してはいけない"
        );
    }

    /// ★ 回帰 (2026-11 実機「くりーんなほうで→クリーン阿呆で」): 文全体の ん+母音 代替は
    /// **flat 最良スコアで採否を決める**。正当な ん+な (くりーんな=クリーンな) を書き換えて
    /// 阿呆(あほう) を採ってはいけない。同時に単一n の ご確認いただけ は救えること。
    ///
    /// 実行: `cargo test -p nuko-core --release --features akaza vowel_boundary_segmented -- --ignored --nocapture`
    #[cfg(feature = "akaza")]
    #[test]
    #[ignore = "実機モデルが要る"]
    fn vowel_boundary_segmented_keeps_legit_nn_na() {
        let home = std::env::var("HOME").expect("HOME");
        let model = format!("{home}/Library/Application Support/nuko-ime/akaza-model");
        let engine = ConversionEngine::with_libakaza(&model).unwrap();
        if !engine.has_libakaza() {
            eprintln!("モデル未配置のためスキップ: {model}");
            return;
        }
        let joined = |r: &str| -> String {
            engine
                .convert_segmented(r)
                .unwrap()
                .map(|s| {
                    s.segments
                        .iter()
                        .filter_map(|seg| seg.surface())
                        .collect::<String>()
                })
                .unwrap_or_default()
        };
        // 正当な ん+な は書き換えない: クリーン阿呆で になってはいけない。
        let kurin = joined("くりーんなほうで");
        assert!(
            !kurin.contains("阿呆"),
            "★ 正当な ん+な を阿呆に書き換えてはいけない (実機: {kurin})"
        );
        // 単一n の ご確認いただけ は救える (に→んい で ご確認いただけ)。
        let gokakunin = joined("ごかくににただけますとさいわいです");
        assert!(
            gokakunin.contains("ご確認いただけ"),
            "★ 単一n の ご確認いただけ は救えるべき (実機: {gokakunin})"
        );
    }

    /// ★ 回帰 (2026-11 実機「変換範囲をかえてスコア上下すると区切りが戻る」):
    /// `reconvert_segmented_preserving` は手動で切り直した文節境界を保つこと。
    /// 自動分割 (`convert_segmented`) と違い、与えた segmented の各文節読みを維持する。
    ///
    /// 実行: `cargo test -p nuko-core --release --features akaza reconvert_segmented_preserving_keeps -- --ignored --nocapture`
    #[cfg(feature = "akaza")]
    #[test]
    #[ignore = "実機モデルが要る"]
    fn reconvert_segmented_preserving_keeps_boundaries() {
        let home = std::env::var("HOME").expect("HOME");
        let model = format!("{home}/Library/Application Support/nuko-ime/akaza-model");
        let engine = ConversionEngine::with_libakaza(&model).unwrap();
        if !engine.has_libakaza() {
            eprintln!("モデル未配置のためスキップ: {model}");
            return;
        }
        let readings = |s: &super::SegmentedConversion| -> Vec<String> {
            s.segments.iter().map(|seg| seg.reading.clone()).collect()
        };
        // 自然分割を取り、Shift+→ 相当で先頭文節を 1 文字伸ばして「切り直した」状態を作る。
        let natural = engine
            .convert_segmented("きょうはいいてんきだ")
            .unwrap()
            .expect("自然分割が取れること");
        assert!(
            natural.segments.len() >= 2,
            "テスト前提: 2 文節以上に割れること ({:?})",
            readings(&natural)
        );
        let resized = engine
            .resize_segment(&natural, true)
            .unwrap()
            .expect("伸縮できること");
        let resized_readings = readings(&resized);
        assert_ne!(
            resized_readings,
            readings(&natural),
            "テスト前提: 伸縮で分割が変わること"
        );
        // 切り直した境界でスコア再変換 → 分割 (各文節読み) が維持されるべき。
        let rescored = engine
            .reconvert_segmented_preserving(&resized)
            .unwrap()
            .expect("境界保持再変換が結果を返すこと");
        assert_eq!(
            readings(&rescored),
            resized_readings,
            "★ スコア再変換で手動の文節境界が元に戻ってはいけない"
        );
    }

    /// backend 単体: 実在語判定 (`reading_is_single_dict_word`) の分離テスト。
    #[cfg(feature = "akaza")]
    #[test]
    #[ignore = "実機モデルが要る"]
    fn reading_is_single_dict_word_discriminates() {
        let home = std::env::var("HOME").expect("HOME");
        let model = format!("{home}/Library/Application Support/nuko-ime/akaza-model");
        let Ok(backend) = super::super::backend::libakaza::LibakazaBackend::try_new(&model) else {
            eprintln!("モデル未配置のためスキップ: {model}");
            return;
        };
        for w in ["しんい", "はんい", "たんい", "せんえん"] {
            assert!(
                backend.reading_is_single_dict_word(w),
                "{w} は実在語なので true のはず"
            );
        }
        for n in ["こんいちは", "おんあ", "みんあ", "ざんえん"] {
            assert!(
                !backend.reading_is_single_dict_word(n),
                "{n} は非実在語なので false のはず"
            );
        }
    }

    /// 実機モデル + 実機 corrections.toml を読んで「まつやさんとなんとか」の
    /// 文節分割と各文節候補を目視する診断テスト (通常は ignore)。
    ///
    /// 実行: `cargo test -p nuko-core --features akaza diag_segment_matsuya -- --ignored --nocapture`
    #[cfg(feature = "akaza")]
    #[test]
    #[ignore = "実機モデルが要る診断用"]
    #[allow(clippy::too_many_lines)] // 診断テスト: 多数の入力を目視するため長い
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
            // nn 曖昧さ (ユーザー報告 2026-10): hannei→はんねい だが 反映 は はんえい 読み。
            // nn_alternate で はんえい も変換し 反映 を救う。単語とセンテンスの両方。
            "はんねい",               // 単語: 反映/繁栄 が出るべき
            "これをはんねいする",     // センテンス: これを反映する
            "へんこうをはんねいする", // センテンス: 変更を反映する
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

        // ★ 文節位置の学習 end-to-end: 学習した切り方が convert_segmented で再現されるか。
        println!("\n=== 文節位置の学習: ざびさんと → [ざび][さん][と] を学習して再変換 ===");
        let before: Vec<String> = engine
            .convert_segmented("ざびさんと")
            .unwrap()
            .map(|s| s.segments.iter().map(|x| x.reading.clone()).collect())
            .unwrap_or_default();
        println!("  学習前の分割 読み={before:?}");
        let learned = engine.learn_segmentation(
            "ざびさんと",
            &["ざび".to_string(), "さん".to_string(), "と".to_string()],
        );
        println!("  learn_segmentation = {learned}");
        let after: Vec<String> = engine
            .convert_segmented("ざびさんと")
            .unwrap()
            .map(|s| s.segments.iter().map(|x| x.reading.clone()).collect())
            .unwrap_or_default();
        println!("  学習後の分割 読み={after:?}");
        assert_eq!(
            after,
            vec!["ざび".to_string(), "さん".to_string(), "と".to_string()],
            "★ 学習した切り方 [ざび][さん][と] が再適用される"
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
