//! アプリ自動アップデート（GitHub Releases ベース）。
//!
//! 最新リリースの tag を取得して [`nuko_core::update::is_newer`] で判定し、
//! macOS では `.app` を取得して置換・再起動する。ネットワークは nuko-cli に閉じる。
//!
//! Sparkle を使わない理由: ①macOS 専用でマルチプラットフォーム化と相反する、
//! ②IME は OS が常駐ロードするため置換後に入力ソース切替/再ログインが要り、
//! 自動再起動だけでは接続が張り直らない（旨味が薄い）。GitHub Releases の tag
//! 比較なら mac/Linux/Windows 共通に書ける。

use anyhow::{anyhow, Result};

/// 対象リポジトリ（owner/repo）。
const REPO: &str = "foresthill/nuko-ime";

/// 最新リリースの要点。
pub struct ReleaseInfo {
    /// タグ（例: `v0.1.3`）。
    pub tag: String,
    /// リリースページ URL。
    pub html_url: String,
    /// `.app` zip の直 DL URL（`NukoIME-v*-macos.zip`。無ければ `None`）。
    pub app_zip_url: Option<String>,
}

/// GitHub API から最新リリースを取得する。
///
/// # エラー
/// ネットワーク失敗 / JSON 不正 / `tag_name` 欠落時。
pub fn fetch_latest() -> Result<ReleaseInfo> {
    // `/releases/latest` は **pre-release を除外** する (nuko は全部 pre-release なので
    // 404 になる)。一覧 `/releases` を取り、新しい順の先頭の非ドラフトを採用する。
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=10");
    let json: serde_json::Value = ureq::get(&url)
        // GitHub API は User-Agent 必須。
        .set("User-Agent", "nuko-ime-updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| anyhow!("GitHub Releases 取得失敗: {e}"))?
        .into_json()
        .map_err(|e| anyhow!("Release JSON 解析失敗: {e}"))?;

    let releases = json
        .as_array()
        .ok_or_else(|| anyhow!("Releases 応答が配列ではありません"))?;
    // 新しい順の先頭の非ドラフト (pre-release は含める)。
    let rel = releases
        .iter()
        .find(|r| {
            !r.get("draft")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("公開リリースが見つかりません"))?;

    let tag = rel
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("tag_name が見つかりません"))?
        .to_string();
    let html_url = rel
        .get("html_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let app_zip_url = rel
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| {
            arr.iter().find_map(|a| {
                let name = a.get("name").and_then(serde_json::Value::as_str)?;
                if name.starts_with("NukoIME-") && name.ends_with("-macos.zip") {
                    a.get("browser_download_url")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                } else {
                    None
                }
            })
        });
    Ok(ReleaseInfo {
        tag,
        html_url,
        app_zip_url,
    })
}

/// macOS: `.app` zip を取得して `~/Library/Input Methods/NukoIME.app` を置換・再起動する。
///
/// 実行中バイナリは直接上書きできないため、置換は **デタッチした小さなシェル**が
/// NukoIME を終了させてから行う（旧版は `.old` に退避してロールバック可能に）。
/// 反映には入力ソースの切替/再ログインが要る旨は呼び出し側が案内する。
///
/// # エラー
/// DL / 解凍 / 展開結果に `NukoIME.app` が無い場合。
#[cfg(target_os = "macos")]
pub fn apply_macos(zip_url: &str) -> Result<()> {
    use std::io::copy;
    use std::process::{Command, Stdio};

    let tmp = std::env::temp_dir().join(format!("nuko-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let zip_path = tmp.join("NukoIME.zip");

    // 1. ダウンロード
    let mut reader = ureq::get(zip_url)
        .set("User-Agent", "nuko-ime-updater")
        .call()
        .map_err(|e| anyhow!("ダウンロード失敗: {e}"))?
        .into_reader();
    let mut f = std::fs::File::create(&zip_path)?;
    copy(&mut reader, &mut f)?;
    drop(f);

    // 2. 解凍（ditto は macOS 標準で zip の .app を正しく展開できる）
    let tmp_str = tmp.to_str().ok_or_else(|| anyhow!("一時パスが不正"))?;
    let zip_str = zip_path.to_str().ok_or_else(|| anyhow!("zip パスが不正"))?;
    let status = Command::new("/usr/bin/ditto")
        .args(["-x", "-k", zip_str, tmp_str])
        .status()?;
    if !status.success() {
        return Err(anyhow!("解凍に失敗しました"));
    }
    let new_app = tmp.join("NukoIME.app");
    if !new_app.exists() {
        return Err(anyhow!("解凍結果に NukoIME.app がありません"));
    }

    // 3. 置換ヘルパーを書いてデタッチ実行（NukoIME を止めてから入れ替え、再起動）。
    let home = std::env::var("HOME")?;
    let dest = format!("{home}/Library/Input Methods/NukoIME.app");
    let new_app_str = new_app.to_str().ok_or_else(|| anyhow!("展開パスが不正"))?;
    let helper = tmp.join("swap.sh");
    let script = format!(
        r#"#!/bin/sh
# nuko-ime 自動更新: NukoIME を止めてから .app を入れ替え、再起動する。
sleep 1
killall NukoIME 2>/dev/null || true
sleep 1
rm -rf "{dest}.old"
[ -d "{dest}" ] && mv "{dest}" "{dest}.old"
cp -R "{new_app_str}" "{dest}"
open "{dest}" 2>/dev/null || true
"#
    );
    std::fs::write(&helper, script)?;
    let helper_str = helper
        .to_str()
        .ok_or_else(|| anyhow!("helper パスが不正"))?;
    // デタッチ（nuko / NukoIME が終了しても生き残るよう stdio を null に）。
    Command::new("/bin/sh")
        .arg(helper_str)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}
