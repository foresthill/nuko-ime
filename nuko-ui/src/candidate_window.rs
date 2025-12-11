//! 候補ウィンドウ

use crate::theme::Theme;
use nuko_core::conversion::Candidate;

/// 候補ウィンドウの状態
#[derive(Debug, Clone)]
pub struct CandidateWindow {
    /// 候補リスト
    candidates: Vec<Candidate>,
    /// 選択中のインデックス
    selected: usize,
    /// 表示中かどうか
    visible: bool,
    /// テーマ
    theme: Theme,
    /// 1ページあたりの候補数
    page_size: usize,
    /// 現在のページ
    current_page: usize,
}

impl CandidateWindow {
    /// 新しい候補ウィンドウを作成
    #[must_use]
    pub fn new() -> Self {
        Self {
            candidates: Vec::new(),
            selected: 0,
            visible: false,
            theme: Theme::Light,
            page_size: 9,
            current_page: 0,
        }
    }

    /// 候補を設定
    pub fn set_candidates(&mut self, candidates: Vec<Candidate>) {
        self.candidates = candidates;
        self.selected = 0;
        self.current_page = 0;
        self.visible = !self.candidates.is_empty();
    }

    /// 候補をクリア
    pub fn clear(&mut self) {
        self.candidates.clear();
        self.selected = 0;
        self.current_page = 0;
        self.visible = false;
    }

    /// 次の候補を選択
    pub fn select_next(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = (self.selected + 1) % self.candidates.len();
            self.update_page();
        }
    }

    /// 前の候補を選択
    pub fn select_prev(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = if self.selected == 0 {
                self.candidates.len() - 1
            } else {
                self.selected - 1
            };
            self.update_page();
        }
    }

    /// 番号で候補を選択（1-9）
    pub fn select_by_number(&mut self, number: usize) {
        if number >= 1 && number <= self.page_size {
            let index = self.current_page * self.page_size + number - 1;
            if index < self.candidates.len() {
                self.selected = index;
            }
        }
    }

    /// 次のページへ
    pub fn next_page(&mut self) {
        let total_pages = self.total_pages();
        if total_pages > 0 {
            self.current_page = (self.current_page + 1) % total_pages;
            self.selected = self.current_page * self.page_size;
        }
    }

    /// 前のページへ
    pub fn prev_page(&mut self) {
        let total_pages = self.total_pages();
        if total_pages > 0 {
            self.current_page = if self.current_page == 0 {
                total_pages - 1
            } else {
                self.current_page - 1
            };
            self.selected = self.current_page * self.page_size;
        }
    }

    /// ページを更新（選択に基づいて）
    fn update_page(&mut self) {
        self.current_page = self.selected / self.page_size;
    }

    /// 総ページ数を取得
    #[must_use]
    pub fn total_pages(&self) -> usize {
        if self.candidates.is_empty() {
            0
        } else {
            (self.candidates.len() + self.page_size - 1) / self.page_size
        }
    }

    /// 現在のページの候補を取得
    #[must_use]
    pub fn current_page_candidates(&self) -> &[Candidate] {
        let start = self.current_page * self.page_size;
        let end = std::cmp::min(start + self.page_size, self.candidates.len());
        &self.candidates[start..end]
    }

    /// 選択中の候補を取得
    #[must_use]
    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }

    /// 選択インデックスを取得
    #[must_use]
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// 現在のページ内での選択インデックスを取得
    #[must_use]
    pub fn selected_index_in_page(&self) -> usize {
        self.selected % self.page_size
    }

    /// 表示中かどうかを取得
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 表示状態を設定
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// テーマを設定
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// テーマを取得
    #[must_use]
    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// 候補数を取得
    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// 空かどうか
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
}

impl Default for CandidateWindow {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidate_window() {
        let mut window = CandidateWindow::new();
        assert!(!window.is_visible());
        assert!(window.is_empty());

        let candidates = vec![
            Candidate::new("日本", "にほん"),
            Candidate::new("二本", "にほん"),
        ];
        window.set_candidates(candidates);

        assert!(window.is_visible());
        assert_eq!(window.len(), 2);
        assert_eq!(window.selected_index(), 0);
    }

    #[test]
    fn test_navigation() {
        let mut window = CandidateWindow::new();
        let candidates: Vec<_> = (0..15)
            .map(|i| Candidate::new(format!("候補{i}"), "こうほ"))
            .collect();
        window.set_candidates(candidates);

        assert_eq!(window.selected_index(), 0);

        window.select_next();
        assert_eq!(window.selected_index(), 1);

        window.select_prev();
        assert_eq!(window.selected_index(), 0);

        window.select_prev(); // ループ
        assert_eq!(window.selected_index(), 14);
    }

    #[test]
    fn test_pagination() {
        let mut window = CandidateWindow::new();
        let candidates: Vec<_> = (0..20)
            .map(|i| Candidate::new(format!("候補{i}"), "こうほ"))
            .collect();
        window.set_candidates(candidates);

        assert_eq!(window.total_pages(), 3); // 20 / 9 = 2.2 → 3ページ
        assert_eq!(window.current_page_candidates().len(), 9);

        window.next_page();
        assert_eq!(window.current_page, 1);
        assert_eq!(window.current_page_candidates().len(), 9);

        window.next_page();
        assert_eq!(window.current_page, 2);
        assert_eq!(window.current_page_candidates().len(), 2);
    }
}
