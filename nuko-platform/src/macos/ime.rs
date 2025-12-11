//! macOS IME実装

#![cfg(target_os = "macos")]

use crate::config::Config;
use crate::error::{PlatformError, Result};
use nuko_core::prelude::*;
use tracing::{debug, info};

/// macOS用ぬこIME
pub struct NukoIME {
    /// コンバージョンエンジン
    engine: ConversionEngine,
    /// 設定
    config: Config,
    /// 登録済みフラグ
    registered: bool,
}

impl NukoIME {
    /// 新しいIMEインスタンスを作成
    pub fn new() -> Result<Self> {
        info!("ぬこIME (macOS) を初期化中...");

        let engine = ConversionEngine::new()
            .map_err(|e| PlatformError::Core(e))?;

        let config = Config::load(Config::default_path())
            .unwrap_or_default();

        Ok(Self {
            engine,
            config,
            registered: false,
        })
    }

    /// IMEをシステムに登録
    pub fn register(&mut self) -> Result<()> {
        if self.registered {
            return Ok(());
        }

        info!("Input Method KitにIMEを登録中...");

        // TODO: 実際のInput Method Kit登録処理
        // - IMKServerを初期化
        // - IMKInputControllerを実装

        self.registered = true;
        debug!("IME登録完了");
        Ok(())
    }

    /// IMEの登録を解除
    pub fn unregister(&mut self) -> Result<()> {
        if !self.registered {
            return Ok(());
        }

        info!("Input Method KitからIMEを登録解除中...");

        // TODO: 実際の登録解除処理

        self.registered = false;
        debug!("IME登録解除完了");
        Ok(())
    }

    /// 設定を取得
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// 設定を更新
    pub fn set_config(&mut self, config: Config) {
        self.config = config;
    }

    /// エンジンへの参照を取得
    #[must_use]
    pub fn engine(&self) -> &ConversionEngine {
        &self.engine
    }

    /// エンジンへの可変参照を取得
    pub fn engine_mut(&mut self) -> &mut ConversionEngine {
        &mut self.engine
    }
}

impl Drop for NukoIME {
    fn drop(&mut self) {
        if self.registered {
            if let Err(e) = self.unregister() {
                tracing::error!("IME登録解除に失敗: {}", e);
            }
        }
    }
}
