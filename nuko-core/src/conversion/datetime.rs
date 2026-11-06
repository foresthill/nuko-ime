//! 「きょう」「いま」等の読みに、今日の日付 / 現在時刻を候補として生成する (①規則/生成器)。
//!
//! 動的 (現在時刻に依存) なので静的辞書でなく **変換時に注入** する。MS-IME/Google
//! にもある定番機能。純粋関数 `candidates_at` に `now` を渡してテストできる。

use chrono::{DateTime, Datelike, Duration, Local, Timelike};

use super::{Candidate, CandidateSource};

/// 日付/時刻候補のスコア (そこそこ上位に出すが、今日(kanji) 等の既定は残す)。
const DATETIME_SCORE: i32 = 50;

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
}
