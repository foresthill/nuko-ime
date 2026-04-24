//! Linux IME実装

#![cfg(target_os = "linux")]

use crate::config::Config;
use crate::error::{PlatformError, Result};
use nuko_core::prelude::*;
use tracing::{debug, info};

/// Linux用ぬこIME
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
        info!("ぬこIME (Linux) を初期化中...");

        let engine = ConversionEngine::new().map_err(|e| PlatformError::Core(e))?;

        let config = Config::load(Config::default_path()).unwrap_or_default();

        Ok(Self {
            engine,
            config,
            registered: false,
        })
    }

    /// IMEをシステムに登録 (IBus)
    pub fn register(&mut self) -> Result<()> {
        if self.registered {
            return Ok(());
        }

        info!("IBusにIMEを登録中...");

        // TODO: 実際のIBus登録処理
        // - IBusEngineを実装
        // - D-Busに登録

        self.registered = true;
        debug!("IME登録完了");
        Ok(())
    }

    /// IMEの登録を解除
    pub fn unregister(&mut self) -> Result<()> {
        if !self.registered {
            return Ok(());
        }

        info!("IBusからIMEを登録解除中...");

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
