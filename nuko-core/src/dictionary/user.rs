//! ユーザー辞書

use crate::conversion::Candidate;
use crate::error::{NukoError, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// ユーザー辞書エントリ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEntry {
    /// 表層形
    pub surface: String,
    /// 読み
    pub reading: String,
    /// 品詞（オプション）
    pub pos: Option<String>,
    /// コメント（オプション）
    pub comment: Option<String>,
}

impl UserEntry {
    /// 新しいエントリを作成
    #[must_use]
    pub fn new(surface: impl Into<String>, reading: impl Into<String>) -> Self {
        Self {
            surface: surface.into(),
            reading: reading.into(),
            pos: None,
            comment: None,
        }
    }

    /// 品詞を設定
    #[must_use]
    pub fn with_pos(mut self, pos: impl Into<String>) -> Self {
        self.pos = Some(pos.into());
        self
    }

    /// コメントを設定
    #[must_use]
    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }
}

/// ユーザー辞書
pub struct UserDictionary {
    /// エントリのマップ（読み → エントリリスト）
    entries: Arc<RwLock<HashMap<String, Vec<UserEntry>>>>,
    /// ファイルパス
    path: Option<std::path::PathBuf>,
    /// 変更フラグ
    dirty: Arc<RwLock<bool>>,
}

impl UserDictionary {
    /// 新しいユーザー辞書を作成
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            path: None,
            dirty: Arc::new(RwLock::new(false)),
        }
    }

    /// ファイルから読み込み
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(Self {
                entries: Arc::new(RwLock::new(HashMap::new())),
                path: Some(path.to_path_buf()),
                dirty: Arc::new(RwLock::new(false)),
            });
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| NukoError::Dictionary(format!("辞書ファイルの読み込みに失敗: {e}")))?;

        let entries: Vec<UserEntry> = serde_json::from_str(&content)
            .map_err(|e| NukoError::Dictionary(format!("辞書ファイルのパースに失敗: {e}")))?;

        let mut map: HashMap<String, Vec<UserEntry>> = HashMap::new();
        for entry in entries {
            map.entry(entry.reading.clone()).or_default().push(entry);
        }

        Ok(Self {
            entries: Arc::new(RwLock::new(map)),
            path: Some(path.to_path_buf()),
            dirty: Arc::new(RwLock::new(false)),
        })
    }

    /// ファイルに保存
    pub fn save(&self) -> Result<()> {
        let path = self
            .path
            .as_ref()
            .ok_or_else(|| NukoError::Dictionary("保存先が設定されていません".to_string()))?;

        let entries = self.entries.read();
        let all_entries: Vec<&UserEntry> = entries.values().flatten().collect();

        let content = serde_json::to_string_pretty(&all_entries)
            .map_err(|e| NukoError::Dictionary(format!("シリアライズに失敗: {e}")))?;

        // ディレクトリがなければ作成
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, content)?;

        *self.dirty.write() = false;
        Ok(())
    }

    /// 保存先パスを設定
    pub fn set_path(&mut self, path: impl AsRef<Path>) {
        self.path = Some(path.as_ref().to_path_buf());
    }

    /// エントリを追加
    pub fn add(&mut self, entry: UserEntry) -> Result<()> {
        let mut entries = self.entries.write();
        entries
            .entry(entry.reading.clone())
            .or_default()
            .push(entry);
        *self.dirty.write() = true;
        Ok(())
    }

    /// エントリを削除
    pub fn remove(&mut self, reading: &str, surface: &str) -> Result<bool> {
        let mut entries = self.entries.write();

        if let Some(list) = entries.get_mut(reading) {
            let original_len = list.len();
            list.retain(|e| e.surface != surface);

            if list.len() < original_len {
                *self.dirty.write() = true;
                if list.is_empty() {
                    entries.remove(reading);
                }
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// 読みから候補を検索
    pub fn lookup(&self, reading: &str) -> Vec<Candidate> {
        let entries = self.entries.read();

        entries
            .get(reading)
            .map(|list| {
                list.iter()
                    .map(|e| {
                        let mut c = Candidate::new(&e.surface, &e.reading).with_score(200);
                        if let Some(ref pos) = e.pos {
                            c = c.with_pos(pos);
                        }
                        c
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 変更があるかどうか
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        *self.dirty.read()
    }

    /// エントリ数を取得
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.read().values().map(|v| v.len()).sum()
    }

    /// 空かどうか
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    /// すべてのエントリを取得
    pub fn all_entries(&self) -> Vec<UserEntry> {
        self.entries.read().values().flatten().cloned().collect()
    }
}

impl Default for UserDictionary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_lookup() {
        let mut dict = UserDictionary::new();
        dict.add(UserEntry::new("猫", "ねこ")).unwrap();

        let candidates = dict.lookup("ねこ");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].surface, "猫");
    }

    #[test]
    fn test_remove() {
        let mut dict = UserDictionary::new();
        dict.add(UserEntry::new("猫", "ねこ")).unwrap();
        dict.add(UserEntry::new("ネコ", "ねこ")).unwrap();

        assert!(dict.remove("ねこ", "猫").unwrap());
        let candidates = dict.lookup("ねこ");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].surface, "ネコ");
    }
}
