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

/// 候補パネルの操作ヒント行 (順位を**上げる**確定キー と **スコア調整/点数指定**) を、
/// プラットフォーム表記で返す。
///
/// キーの配線: 上げる=#220 (Shift+Enter=教える=絶対1位)、スコア微調整=#228 (⌥→ 上げ / ⌥← 下げ)、
/// ポイント直接指定=#228 (Shift+↑ でスコア編集モード → 数値を打って Enter で確定)。
///
/// ★「⌃⏎=下げる(降格)」はヒントから外した (2026-11 森岡さん: boost を外すだけで高 count だと
/// 大して下がらず分かりにくい。下げたいときは ⌥← か 点数指定で低い値)。配線自体は残すが非表示。
/// ★ 点数指定キーは **Shift+↑**。⌥↑(縦矢印=会話切替) も ⌥=(ズーム等) も Claude Desktop 等が
/// IME より先に奪ったため、アプリに奪われない「テキスト編集キー」(矢印/Shift系) を使う。
///
/// macOS は修飾キー記号 (`⇧⏎` `⌥←→` `⇧↑`)、Windows/Linux は語 (`Shift+Enter` 等) で見せる。
/// すべて実際に配線済みで**届く**キーだけを出す (動かない/奪われるキーは表示しない方針)。
#[must_use]
pub fn candidate_key_hint(platform: Platform) -> String {
    match platform {
        Platform::MacOs => "⇧⏎=上げる  ⌥←→=スコア増減  ⇧↑で点数指定".to_string(),
        Platform::Windows | Platform::Linux => {
            "Shift+Enter=上げる  Alt+←→=スコア増減  Shift+↑で点数指定".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_hint_uses_modifier_glyphs() {
        let h = candidate_key_hint(Platform::MacOs);
        assert!(h.contains('⇧'), "Shift 記号");
        assert!(h.contains('⏎'), "Return 記号 (教える)");
        assert!(h.contains('⌥'), "Option 記号 (スコア増減)");
        assert!(h.contains('↑'), "Shift+↑ 点数指定");
        assert!(h.contains("上げる") && h.contains("スコア") && h.contains("点数"));
        // ⌃⏎=下げる(降格) はヒストから外した (2026-11 森岡さん)。
        assert!(!h.contains("下げる"), "下げる(降格)はヒントに出さない");
    }

    #[test]
    fn windows_and_linux_hint_uses_words() {
        for p in [Platform::Windows, Platform::Linux] {
            let h = candidate_key_hint(p);
            assert!(h.contains("Shift+Enter"), "{p:?} は Shift+Enter 語表記");
            // ⌥=Alt の対応どおり、スコア増減は Alt+←→ の語表記。
            assert!(h.contains("Alt+←→"), "{p:?} は Alt+←→ 語表記");
            assert!(
                h.contains("Shift+↑"),
                "{p:?} はポイント直接指定 Shift+↑ 語表記"
            );
            // macOS 記号は使わない。
            assert!(!h.contains('⇧') && !h.contains('⌃') && !h.contains('⏎') && !h.contains('⌥'));
            // 下げる(降格) は非表示。
            assert!(!h.contains("Ctrl+Enter") && !h.contains("下げる"));
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
