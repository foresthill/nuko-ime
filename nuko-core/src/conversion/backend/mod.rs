//! 変換バックエンドの実装
//!
//! 異なる変換エンジン (libakaza ベースの統計変換等) を pluggable に組み込むためのモジュール。
//! 当面は libakaza バックエンドのみだが、将来的に他のバックエンドを追加する余地を残す。
//!
//! `akaza` feature が無効な場合は何もエクスポートされない。

#[cfg(feature = "akaza")]
pub mod libakaza;

#[cfg(feature = "akaza")]
pub use libakaza::LibakazaBackend;
