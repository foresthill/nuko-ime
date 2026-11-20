//! 「きょう」「いま」等の読みに、今日の日付 / 現在時刻を候補として生成する (①規則/生成器)。
//!
//! 動的 (現在時刻に依存) なので静的辞書でなく **変換時に注入** する。MS-IME/Google
//! にもある定番機能。純粋関数 `candidates_at` に `now` を渡してテストできる。

use chrono::{DateTime, Datelike, Duration, Local, Timelike};

use super::{Candidate, CandidateSource};

/// 日付/時刻候補のスコア (そこそこ上位に出すが、今日(kanji) 等の既定は残す)。
const DATETIME_SCORE: i32 = 50;

/// フォーマット選好 1 回あたりの加点 (日付グループ内の並べ替え用)。
///
/// base `DATETIME_SCORE` に対し、ユーザーがその形式 (`ymd_slash` 等) を確定するたび
/// `count × STEP` だけ上乗せして「選んだ形式ほど日付グループの上」にする。+1/回 では
/// 体感で「スコアが上がらない」ため、1 回で確実に差が出る幅にした (2026-11 ユーザー報告)。
pub const DATE_FORMAT_PREFERENCE_STEP: i32 = 50;

/// フォーマット選好加点の上限。
///
/// 日付候補が libakaza/学習帯 (~98k 以上) を超えて「今日(漢字)」等の既定を飛び越えない
/// よう頭打ちにする。防御的な安全弁で、実使用の確定回数 (90000/50 = 1800 回) では到達しない。
/// この不変条件は engine.rs の
/// `date_format_boost_is_step_times_count_and_stays_below_libakaza_band` テストで固定する。
pub const DATE_FORMAT_PREFERENCE_MAX_BOOST: i32 = 90_000;

/// 読みに対応する日付/時刻候補を返す (現在時刻を使用。該当なしは空)。
#[must_use]
pub fn datetime_candidates(reading: &str) -> Vec<Candidate> {
    candidates_at(reading, Local::now())
}

/// この読みが日付/時刻を生成するトリガーか (きょう/あした/… )。
#[must_use]
pub fn is_datetime_reading(reading: &str) -> bool {
    matches!(
        reading,
        "きょう" | "あした" | "あす" | "きのう" | "いま" | "じこく" | "にちじ"
    )
}

/// 確定した (reading, surface) が **一過性の日付/時刻生成候補** かどうか。true なら
/// 学習層はこれを記録してはいけない。
///
/// 2026-11 ユーザー報告の根治: 10/20 に「きょう」→「2026年10月20日」を確定したら
/// それが学習され、翌日以降も毎日その**古い日付**が今日の日付より上位に出続けた。
/// 日付/時刻は `datetime_candidates` が毎回 now で生成するので、学習する意味が無く、
/// むしろ古い値を固定してしまう害だけがある。
///
/// 判定は「datetime トリガー読み」かつ「表層に数字 (ASCII 0-9) を含む」で行う。
/// 生成される日付/時刻 (2026年10月20日 / 2026/10/20 / 令和8年10月20日 / R8.10.20 /
/// 10:30 / 10時30分 / タイムスタンプ) は**すべて数字を含む**一方、漢字の既定候補
/// (今日/明日/昨日/今/日時/時刻) は数字を含まない。この discriminator は日またぎ・
/// 秒ズレ (確定が生成の数秒後) に影響されず頑健 (now 再生成との厳密一致より安全)。
#[must_use]
pub fn is_datetime_surface(reading: &str, surface: &str) -> bool {
    is_datetime_reading(reading) && surface.chars().any(|c| c.is_ascii_digit())
}

/// 生成された日付/時刻の表層を **フォーマット種別 (format id)** へ分類する。
///
/// リテラル日付 (`2026/11/18`) は一過性なので学習しないが、その**形式** (`ymd_slash`) は
/// ユーザーの選好として記録できる。これはその分類器で、**フォーマット選好学習の鍵**。
/// `candidates_at` が生成し得る全表層が Some を返す (下の束縛テストで保証＝生成器と
/// 分類器の乖離を防ぐ。docs/CODE_COMMENT_DISCIPLINE.md の方針)。
///
/// 返す id は `date_strings` / `time_strings` / `timestamp_strings` の各フォーマットに対応:
/// `ymd_kanji`(年月日) / `ymd_kanji_wd`(年月日（曜）) / `ymd_slash`(Y/m/d) /
/// `ymd_hyphen`(Y-m-d) / `reiwa_kanji`(令和…) / `reiwa_dot`(R…) /
/// `hm_colon`(HH:MM) / `hm_ja`(H時M分) / `hms_colon`(HH:MM:SS) /
/// `ts_kanji_hms` / `ts_kanji_hm` / `ts_slash_hms` / `ts_hyphen_hms`(日付＋時刻)。
#[must_use]
pub fn classify_datetime_format(surface: &str) -> Option<&'static str> {
    let has_space = surface.contains(' ');
    let colons = surface.matches(':').count();
    // タイムスタンプ (日付＋時刻、半角スペース区切り) を先に判定。
    if has_space {
        if surface.contains('年') {
            return Some(if colons >= 2 {
                "ts_kanji_hms"
            } else {
                "ts_kanji_hm"
            });
        }
        if surface.contains('/') {
            return Some("ts_slash_hms");
        }
        if surface.contains('-') {
            return Some("ts_hyphen_hms");
        }
        return None;
    }
    // 日付 (年月日系)。
    if surface.contains('年') && surface.contains('日') {
        if surface.starts_with("令和") {
            return Some("reiwa_kanji");
        }
        if surface.contains('（') {
            return Some("ymd_kanji_wd");
        }
        return Some("ymd_kanji");
    }
    if surface.starts_with('R') && surface.contains('.') {
        return Some("reiwa_dot");
    }
    if surface.contains('/') {
        return Some("ymd_slash");
    }
    if surface.contains('-') && surface.starts_with(|c: char| c.is_ascii_digit()) {
        return Some("ymd_hyphen");
    }
    // 時刻のみ。
    if surface.contains('時') && surface.contains('分') {
        return Some("hm_ja");
    }
    if colons >= 2 {
        return Some("hms_colon");
    }
    if colons == 1 {
        return Some("hm_colon");
    }
    None
}

/// 純粋版: `now` を渡してテスト可能にする。
fn candidates_at(reading: &str, now: DateTime<Local>) -> Vec<Candidate> {
    let surfaces: Vec<String> = match reading {
        "きょう" => date_strings(now, 0),
        "あした" | "あす" => date_strings(now, 1),
        "きのう" => date_strings(now, -1),
        "いま" | "じこく" => {
            // 時刻だけでなく「日付＋時刻」版も出す (ユーザー要望)。
            let mut v = time_strings(now);
            v.extend(timestamp_strings(now));
            v
        }
        "にちじ" => {
            let mut v = timestamp_strings(now);
            v.extend(date_strings(now, 0));
            v
        }
        _ => return Vec::new(),
    };
    surfaces
        .into_iter()
        .map(|s| {
            Candidate::new(&s, reading)
                .with_score(DATETIME_SCORE)
                .with_source(CandidateSource::System)
        })
        .collect()
}

/// 曜日を漢字 1 文字で返す。
fn weekday_ja(w: chrono::Weekday) -> &'static str {
    use chrono::Weekday::{Fri, Mon, Sat, Sun, Thu, Tue, Wed};
    match w {
        Mon => "月",
        Tue => "火",
        Wed => "水",
        Thu => "木",
        Fri => "金",
        Sat => "土",
        Sun => "日",
    }
}

/// `now` から `offset_days` ずらした日付を各種フォーマットで返す。
fn date_strings(now: DateTime<Local>, offset_days: i64) -> Vec<String> {
    let d = now + Duration::days(offset_days);
    let (y, m, day) = (d.year(), d.month(), d.day());
    let wd = weekday_ja(d.weekday());
    // 令和 = 2019 年始まり (令和1)。2019 年以降で有効。
    let reiwa = y - 2018;
    let mut v = vec![
        format!("{y}年{m}月{day}日"),
        format!("{y}年{m}月{day}日（{wd}）"),
        format!("{y}/{m:02}/{day:02}"),
        format!("{y}-{m:02}-{day:02}"),
    ];
    if reiwa >= 1 {
        v.push(format!("令和{reiwa}年{m}月{day}日"));
        v.push(format!("R{reiwa}.{m:02}.{day:02}"));
    }
    v
}

/// 現在時刻を各種フォーマットで返す。
fn time_strings(now: DateTime<Local>) -> Vec<String> {
    let (h, mi, s) = (now.hour(), now.minute(), now.second());
    vec![
        format!("{h:02}:{mi:02}"),
        format!("{h}時{mi}分"),
        format!("{h:02}:{mi:02}:{s:02}"),
    ]
}

/// 日付＋時刻のタイムスタンプを返す (秒まで。例: 2026年10月20日 18:55:28)。
fn timestamp_strings(now: DateTime<Local>) -> Vec<String> {
    let (y, m, day) = (now.year(), now.month(), now.day());
    let (h, mi, s) = (now.hour(), now.minute(), now.second());
    vec![
        format!("{y}年{m}月{day}日 {h:02}:{mi:02}:{s:02}"),
        format!("{y}年{m}月{day}日 {h:02}:{mi:02}"),
        format!("{y}/{m:02}/{day:02} {h:02}:{mi:02}:{s:02}"),
        format!("{y}-{m:02}-{day:02} {h:02}:{mi:02}:{s:02}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// 2026-10-20 10:30:45 (固定) を Local として作る。フィールドはそのまま読める。
    fn fixed() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 10, 20, 10, 30, 45)
            .single()
            .unwrap()
    }

    #[test]
    fn kyou_generates_date_formats() {
        let c = candidates_at("きょう", fixed());
        let surfaces: Vec<&str> = c.iter().map(|x| x.surface.as_str()).collect();
        assert!(surfaces.contains(&"2026年10月20日"), "和暦なし日付");
        assert!(surfaces.contains(&"2026/10/20"), "スラッシュ区切り");
        assert!(surfaces.contains(&"2026-10-20"), "ハイフン区切り");
        assert!(surfaces.contains(&"令和8年10月20日"), "令和 (2026=令和8)");
    }

    #[test]
    fn ashita_kinou_offset() {
        let a: Vec<String> = candidates_at("あした", fixed())
            .iter()
            .map(|x| x.surface.clone())
            .collect();
        assert!(a.contains(&"2026年10月21日".to_string()), "あした=+1日");
        let k: Vec<String> = candidates_at("きのう", fixed())
            .iter()
            .map(|x| x.surface.clone())
            .collect();
        assert!(k.contains(&"2026年10月19日".to_string()), "きのう=-1日");
    }

    #[test]
    fn ima_generates_time_and_timestamp() {
        let c = candidates_at("いま", fixed());
        let surfaces: Vec<&str> = c.iter().map(|x| x.surface.as_str()).collect();
        assert!(surfaces.contains(&"10:30"), "HH:MM");
        assert!(surfaces.contains(&"10時30分"), "日本語時刻");
        assert!(surfaces.contains(&"10:30:45"), "秒まで");
        // 日付＋時刻版も出る (ユーザー要望)。
        assert!(
            surfaces.contains(&"2026年10月20日 10:30:45"),
            "いま に日付＋時刻版"
        );
    }

    #[test]
    fn unrelated_reading_is_empty() {
        assert!(candidates_at("ねこ", fixed()).is_empty());
        assert!(candidates_at("", fixed()).is_empty());
    }

    /// ★ 一過性の日付/時刻生成候補は学習対象外 (2026-11「きょう→古い日付」根治)。
    #[test]
    fn datetime_surfaces_are_not_learnable() {
        // 生成された日付/時刻 (数字を含む) → true (学習しない)。
        for s in [
            "2026年10月20日",
            "2026年10月20日（火）",
            "2026/10/20",
            "2026-10-20",
            "令和8年10月20日",
            "R8.10.20",
        ] {
            assert!(
                is_datetime_surface("きょう", s),
                "きょう の生成日付 {s} は学習対象外"
            );
        }
        assert!(is_datetime_surface("いま", "10:30"));
        assert!(is_datetime_surface("いま", "10時30分"));
        assert!(is_datetime_surface("にちじ", "2026年10月20日 10:30:45"));
        assert!(is_datetime_surface("あした", "2026年10月21日"));
        assert!(is_datetime_surface("きのう", "2026-10-19"));

        // 漢字の既定候補 (数字なし) → false (学習してよい)。
        assert!(!is_datetime_surface("きょう", "今日"));
        assert!(!is_datetime_surface("きょう", "京"));
        assert!(!is_datetime_surface("あした", "明日"));
        assert!(!is_datetime_surface("いま", "今"));
        assert!(!is_datetime_surface("にちじ", "日時"));

        // datetime トリガーでない読みは、数字を含んでも対象外ではない (学習してよい)。
        assert!(!is_datetime_surface("ねこ", "猫2"));
        assert!(!is_datetime_surface("きょうと", "京都"));
    }

    #[test]
    fn classify_datetime_format_matches_patterns() {
        assert_eq!(
            classify_datetime_format("2026年10月20日"),
            Some("ymd_kanji")
        );
        assert_eq!(
            classify_datetime_format("2026年10月20日（火）"),
            Some("ymd_kanji_wd")
        );
        assert_eq!(classify_datetime_format("2026/10/20"), Some("ymd_slash"));
        assert_eq!(classify_datetime_format("2026-10-20"), Some("ymd_hyphen"));
        assert_eq!(
            classify_datetime_format("令和8年10月20日"),
            Some("reiwa_kanji")
        );
        assert_eq!(classify_datetime_format("R8.10.20"), Some("reiwa_dot"));
        assert_eq!(classify_datetime_format("10:30"), Some("hm_colon"));
        assert_eq!(classify_datetime_format("10時30分"), Some("hm_ja"));
        assert_eq!(classify_datetime_format("10:30:45"), Some("hms_colon"));
        assert_eq!(
            classify_datetime_format("2026年10月20日 10:30:45"),
            Some("ts_kanji_hms")
        );
        assert_eq!(
            classify_datetime_format("2026年10月20日 10:30"),
            Some("ts_kanji_hm")
        );
        assert_eq!(
            classify_datetime_format("2026/10/20 10:30:45"),
            Some("ts_slash_hms")
        );
        assert_eq!(
            classify_datetime_format("2026-10-20 10:30:45"),
            Some("ts_hyphen_hms")
        );
        // 日付/時刻でない表層は None。
        assert_eq!(classify_datetime_format("今日"), None);
        assert_eq!(classify_datetime_format("猫"), None);
    }

    /// ★ 束縛テスト: `candidates_at` が生成し得る**全表層が Some に分類される**こと。
    /// 生成器 (date_strings 等) と分類器 (classify_datetime_format) の乖離を防ぐ
    /// (フォーマットを足したのに分類器を更新し忘れる＝選好が効かない、を赤で検知)。
    #[test]
    fn every_generated_surface_classifies_to_some_format() {
        for reading in [
            "きょう",
            "あした",
            "あす",
            "きのう",
            "いま",
            "じこく",
            "にちじ",
        ] {
            for cand in candidates_at(reading, fixed()) {
                assert!(
                    classify_datetime_format(&cand.surface).is_some(),
                    "生成表層 {} ({}) が分類器で None。classify_datetime_format を更新せよ",
                    cand.surface,
                    reading
                );
            }
        }
    }

    /// ★ characterization: **生成器 `candidates_at` 自体は選好で変わらない**。
    /// 全フォーマットが同一スコア `DATETIME_SCORE`(=50) で生成され、全て学習対象外。
    /// フォーマット選好は**エンジン層**で候補へ加点して反映する（生成器は不変）ので、
    /// この「生成は均一」特性は選好学習を入れても保たれる（engine 側テスト
    /// `date_format_preference_ranks_picked_format_first` が反映側を担保）。
    #[test]
    fn date_candidates_share_fixed_score_and_are_unlearnable() {
        let c = candidates_at("きょう", fixed());
        assert!(!c.is_empty(), "きょう は日付候補を生成する");
        for cand in &c {
            assert_eq!(
                cand.score, DATETIME_SCORE,
                "生成時は全フォーマットが固定スコア {DATETIME_SCORE}: {}",
                cand.surface
            );
            assert!(
                is_datetime_surface("きょう", &cand.surface),
                "生成日付 {} は (リテラルとしては) 学習対象外",
                cand.surface
            );
        }
    }
}
