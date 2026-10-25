//! アップデート判定の純粋ロジック（クロスプラットフォーム）。
//!
//! ネットワークは持たない（それは nuko-cli の責務）。ここはバージョン文字列の
//! パースと比較だけ。mac/Linux/Windows 共通で使える。

/// `"v1.2.3"` / `"1.2.3"` / `"1.2.3-rc1"` を `(major, minor, patch)` に。
///
/// 先頭の `v`/`V` と、`-` 以降のプレリリース識別子は無視する。欠けた要素は 0。
/// 数値としてパースできなければ `None`。
#[must_use]
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    // "1.2.3-rc1" → "1.2.3"
    let core = s.split('-').next().unwrap_or(s);
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// `latest` が `current` より新しければ `true`。
///
/// どちらかがパース不能なら **`false`（安全側 = 更新を促さない）**。
#[must_use]
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_handles_v_prefix_and_prerelease() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_version("2"), Some((2, 0, 0)));
        assert_eq!(parse_version("v0.1.3-rc1"), Some((0, 1, 3)));
        assert_eq!(parse_version("  v1.0.0  "), Some((1, 0, 0)));
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn is_newer_compares_semantically() {
        assert!(is_newer("0.1.3", "0.1.4"));
        assert!(is_newer("0.1.3", "0.2.0"));
        assert!(is_newer("0.1.3", "1.0.0"));
        assert!(is_newer("v0.1.3", "v0.1.4"), "v 接頭辞を無視");
        // 同じ or 古い → false
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("1.0.0", "0.9.9"));
    }

    #[test]
    fn unparseable_is_safe_false() {
        assert!(!is_newer("0.1.3", "latest"));
        assert!(!is_newer("dev", "0.2.0"));
    }

    #[test]
    fn patch_and_minor_ordering() {
        assert!(
            is_newer("0.1.9", "0.1.10"),
            "patch は数値比較 (文字列比較ではない)"
        );
        assert!(is_newer("0.9.0", "0.10.0"), "minor も数値比較");
    }
}
