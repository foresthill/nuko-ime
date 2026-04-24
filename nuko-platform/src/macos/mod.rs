//! macOS Input Method Kit統合
//!
//! Input Method Kit を使用したmacOS IME実装。

#![cfg(target_os = "macos")]

mod ime;

pub use ime::NukoIME;
