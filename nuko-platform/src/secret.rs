//! API キー等の秘密情報を OS の secret store に保存する。
//!
//! macOS: **Keychain** を `security` コマンド経由で読み書きする (追加依存なし)。
//! 他 OS: 現状未対応 (将来 keyring crate 等で Secret Service / Credential Manager へ)。
//!
//! `service = "nuko-ime"` 固定、`account = provider` 名 (`"anthropic"` / `"openrouter"`)。
//! 書き手 (設定ウィンドウ / CLI) と読み手 (CLI の dream) が同じ命名を共有する。
//!
//! 注: 保存 (`set_api_key`) は `security -w <key>` にキーを渡すため、実行中の一瞬だけ
//! `ps` にキーが見え得る (同一ユーザのローカルプロセスに限る)。取得はキーを stdout で
//! 受けるため引数には出ない。設定は稀 (1 回) なので許容する。

use crate::error::{PlatformError, Result};

/// Keychain のサービス名 (全エントリ共通)。
const SERVICE: &str = "nuko-ime";

/// provider (例 `"anthropic"`) の API キーを secret store に保存する (既存は上書き)。
pub fn set_api_key(provider: &str, key: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("security")
            .args([
                "add-generic-password",
                "-U", // 既存があれば更新
                "-s",
                SERVICE,
                "-a",
                provider,
                "-w",
                key,
            ])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError::Platform(format!(
                "Keychain への保存に失敗しました (security 終了: {status})"
            )))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (provider, key);
        Err(PlatformError::Unsupported(
            "secret store は現状 macOS (Keychain) のみ対応".into(),
        ))
    }
}

/// provider の API キーを取得する。未登録なら `Ok(None)`。
pub fn get_api_key(provider: &str) -> Result<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("security")
            .args([
                "find-generic-password",
                "-s",
                SERVICE,
                "-a",
                provider,
                "-w", // パスワードのみを stdout へ
            ])
            .output()?;
        if out.status.success() {
            let key = String::from_utf8_lossy(&out.stdout)
                .trim_end_matches('\n')
                .to_string();
            Ok((!key.is_empty()).then_some(key))
        } else {
            // item not found 等は「未登録」として None を返す。
            Ok(None)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = provider;
        Ok(None)
    }
}

/// provider の API キーを削除する。未登録でもエラーにしない。
pub fn delete_api_key(provider: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        // 未登録でも成功扱い (終了コードは無視)。
        std::process::Command::new("security")
            .args(["delete-generic-password", "-s", SERVICE, "-a", provider])
            .status()?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = provider;
        Err(PlatformError::Unsupported(
            "secret store は現状 macOS (Keychain) のみ対応".into(),
        ))
    }
}
