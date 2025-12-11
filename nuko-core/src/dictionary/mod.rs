//! 辞書管理モジュール
//!
//! システム辞書とユーザー辞書の管理を提供します。

mod manager;
mod system;
mod user;

pub use manager::DictionaryManager;
pub use user::UserDictionary;
