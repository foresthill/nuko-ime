//! プラットフォーム判定と、UI に出す**キー表記**の生成。
//!
//! nuko-ime は現在 macOS のみだが (将来 Linux / Windows 対応予定)、候補パネルの操作ヒントの
//! ような「修飾キーの見せ方」は OS ごとに変わる (macOS は `⌥⌃⇧⏎` の記号、Windows/Linux は
//! `Alt`/`Ctrl`/`Shift`/`Enter` の語)。表記ロジックを**共有クレート (nuko-core) に 1 箇所**置き、
//! 各フロントエンド (今は nuko-macos、将来 Win/Linux) が同じ関数を呼ぶ。こうすると
//! 「どのキーをどう表示するか」の真実の源が 1 つになる (キー配線を変えたら表記も一緒に直る)。
//!
//! ⚠️ **現時点で実際に描画されるのは macOS 版のみ**。Windows/Linux の文字列は定義とテストで
//! 用意してあるが、まだそれらのフロントエンドは存在しない (= レンダリングされる場所が無い)。

/// 動作プラットフォーム。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS (現行唯一のフロントエンド)。
    MacOs,
    /// Windows (将来対応予定)。
    Windows,
    /// Linux (将来対応予定)。
    Linux,
}

impl Platform {
    /// ビルドターゲットの OS を返す (実行時は現在の OS)。
    ///
    /// macOS 以外の非 Windows はすべて Linux 扱い (BSD 等も含むが、キー表記は Linux と同じ)。
    #[must_use]
    pub fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Platform::MacOs
        }
        #[cfg(target_os = "windows")]
        {
            Platform::Windows
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Platform::Linux
        }
    }
}

/// 候補パネルの操作ヒント行 (順位を**上げる/下げる**確定キー) を、プラットフォーム表記で返す。
///
/// キー自体の配線は #220 済み (Shift+Enter=教える=上げる / Ctrl+Enter=降格=下げる)。
/// macOS は修飾キー記号 (`⇧⏎` `⌃⏎`)、Windows/Linux は語 (`Shift+Enter` 等) で見せる。
///
/// スコア微調整 (`⌥←→` / `Alt+←→`、#228) は controller 配線 (smoke 必須) が済んでから
/// この関数に 1 行足す。今はまだ出さない (= 動かないキーは表示しない)。
#[must_use]
pub fn candidate_key_hint(platform: Platform) -> String {
    match platform {
        Platform::MacOs => "⇧⏎=上げる  ⌃⏎=下げる".to_string(),
        Platform::Windows | Platform::Linux => "Shift+Enter=上げる  Ctrl+Enter=下げる".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_hint_uses_modifier_glyphs() {
        let h = candidate_key_hint(Platform::MacOs);
        assert!(h.contains('⇧'), "Shift 記号");
        assert!(h.contains('⌃'), "Control 記号");
        assert!(h.contains('⏎'), "Return 記号");
        assert!(h.contains("上げる") && h.contains("下げる"));
    }

    #[test]
    fn windows_and_linux_hint_uses_words() {
        for p in [Platform::Windows, Platform::Linux] {
            let h = candidate_key_hint(p);
            assert!(h.contains("Shift+Enter"), "{p:?} は Shift+Enter 語表記");
            assert!(h.contains("Ctrl+Enter"), "{p:?} は Ctrl+Enter 語表記");
            // macOS 記号は使わない。
            assert!(!h.contains('⇧') && !h.contains('⌃') && !h.contains('⏎'));
        }
    }

    #[test]
    fn windows_and_linux_share_the_same_hint() {
        // ⌥=Alt の対応どおり、Win と Linux の表記は同一 (現状どちらも語表記)。
        assert_eq!(
            candidate_key_hint(Platform::Windows),
            candidate_key_hint(Platform::Linux)
        );
    }

    #[test]
    fn current_is_macos_on_mac_build() {
        // CI/ローカルとも macOS でビルドするのでここは MacOs になる。
        // (他 OS でビルドしたらそのプラットフォームを返す = cfg 分岐の確認)。
        #[cfg(target_os = "macos")]
        assert_eq!(Platform::current(), Platform::MacOs);
        #[cfg(target_os = "windows")]
        assert_eq!(Platform::current(), Platform::Windows);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        assert_eq!(Platform::current(), Platform::Linux);
    }
}
