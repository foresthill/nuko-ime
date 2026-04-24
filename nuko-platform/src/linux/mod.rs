//! Linux IBus/Fcitx5統合
//!
//! IBusまたはFcitx5を使用したLinux IME実装。

#![cfg(target_os = "linux")]

mod ime;

pub use ime::NukoIME;
