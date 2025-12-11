//! Windows TSF統合
//!
//! Text Services Framework (TSF) を使用したWindows IME実装。

#![cfg(target_os = "windows")]

mod ime;

pub use ime::NukoIME;

/// Windows用のGUID
pub const CLSID_NUKO_IME: &str = "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX";
