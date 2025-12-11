//! 辞書マネージャー

use super::system::SystemDictionary;
use super::user::UserDictionary;
use crate::conversion::{Candidate, CandidateSource};
use crate::error::Result;
use std::path::Path;

/// 辞書マネージャー
///
/// システム辞書とユーザー辞書を統合管理します。
pub struct DictionaryManager {
    /// システム辞書
    system: SystemDictionary,
    /// ユーザー辞書
    user: UserDictionary,
}

impl DictionaryManager {
    /// 新しい辞書マネージャーを作成
    pub fn new() -> Result<Self> {
        Ok(Self {
            system: SystemDictionary::new()?,
            user: UserDictionary::new(),
        })
    }

    /// ユーザー辞書を読み込み
    pub fn load_user_dictionary(&mut self, path: impl AsRef<Path>) -> Result<()> {
        self.user = UserDictionary::load(path)?;
        Ok(())
    }

    /// ユーザー辞書を保存
    pub fn save_user_dictionary(&self) -> Result<()> {
        self.user.save()
    }

    /// 読みから候補を検索
    ///
    /// ユーザー辞書 → システム辞書の順で検索し、結果をマージします。
    pub fn lookup(&self, reading: &str) -> Result<Vec<Candidate>> {
        let mut candidates = Vec::new();

        // ユーザー辞書から検索
        let user_candidates = self.user.lookup(reading);
        for c in user_candidates {
            candidates.push(c.with_source(CandidateSource::User));
        }

        // システム辞書から検索
        let system_candidates = self.system.lookup(reading)?;
        for c in system_candidates {
            // 重複をチェック
            if !candidates.iter().any(|existing| existing.surface == c.surface) {
                candidates.push(c.with_source(CandidateSource::System));
            }
        }

        Ok(candidates)
    }

    /// ユーザー辞書への参照を取得
    #[must_use]
    pub fn user_dictionary(&self) -> &UserDictionary {
        &self.user
    }

    /// ユーザー辞書への可変参照を取得
    pub fn user_dictionary_mut(&mut self) -> &mut UserDictionary {
        &mut self.user
    }

    /// システム辞書への参照を取得
    #[must_use]
    pub fn system_dictionary(&self) -> &SystemDictionary {
        &self.system
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup() {
        let manager = DictionaryManager::new().unwrap();
        let candidates = manager.lookup("にほん").unwrap();

        assert!(!candidates.is_empty());
        assert!(candidates.iter().any(|c| c.surface == "日本"));
    }

    #[test]
    fn test_user_priority() {
        let mut manager = DictionaryManager::new().unwrap();

        // ユーザー辞書に追加
        use super::super::user::UserEntry;
        manager
            .user_dictionary_mut()
            .add(UserEntry::new("二本", "にほん"))
            .unwrap();

        let candidates = manager.lookup("にほん").unwrap();

        // ユーザー辞書の候補が先頭に来る
        assert_eq!(candidates[0].surface, "二本");
        assert_eq!(candidates[0].source, CandidateSource::User);
    }
}
