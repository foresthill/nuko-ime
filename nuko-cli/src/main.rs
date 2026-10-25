//! ぬこIME CLI

mod anthropic;
mod models;
mod openrouter;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use nuko_core::learning::dreaming::{
    build_prompt, digest_observations, merge_proposals, parse_proposal, DreamProvider, MockProvider,
};
use nuko_core::learning::{extract_corrections, CorrectionStore, ObservationLog};
use nuko_core::prelude::*;
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "nuko")]
#[command(
    author,
    version,
    about = "ぬこIME - 日本人の、日本人による、日本人のためのIME"
)]
struct Cli {
    /// 詳細ログを出力
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 変換をテスト
    Convert {
        /// 変換する読み（ひらがな）
        reading: String,
        /// 候補数
        #[arg(short = 'n', long, default_value = "5")]
        count: usize,
    },
    /// ローマ字をかなに変換
    Romaji {
        /// 変換するローマ字
        input: String,
    },
    /// 予測変換（入力途中で候補を提示）
    Predict {
        /// 入力途中の読み（ひらがな）
        prefix: String,
        /// 候補数
        #[arg(short = 'n', long, default_value = "10")]
        count: usize,
    },
    /// 辞書情報を表示
    DictInfo,
    /// バージョン情報を表示
    Info,
    /// 学習状況の確認・操作 (観察ログ / 訂正選好)
    Learn {
        #[command(subcommand)]
        action: Option<LearnAction>,
    },
    /// Layer 3: AI dreaming (観察を整理して選好を提案)。既定は dry-run。
    Dream {
        /// プロンプトに載せる読みの最大件数
        #[arg(long, default_value = "40")]
        max_readings: usize,
        /// 推論プロバイダ ("mock" / "openrouter" / "anthropic"。未指定なら dry-run でプロンプト表示)
        #[arg(long)]
        provider: Option<String>,
        /// モデル (openrouter/anthropic 用。未指定なら ai.toml の model、それも無ければエラー)
        #[arg(long)]
        model: Option<String>,
        /// 提案を corrections.toml にマージして保存する (プロバイダ指定時のみ有効)
        #[arg(long)]
        apply: bool,
    },
    /// BYOK 設定: API キーを Keychain に保存/確認/削除する
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// ユーザー辞書 (単語登録) を編集する。IME の user_dict.json に直接書く。
    Dict {
        #[command(subcommand)]
        action: DictAction,
    },
    /// プロバイダのモデル一覧を実 API から取得して表示する (設定画面のモデル選択用)。
    Models {
        /// プロバイダ ("anthropic" / "openrouter")
        provider: String,
    },
    /// アップデート確認 (GitHub Releases)。--apply で .app を置換・再起動 (macOS)。
    ///
    /// 終了コード: 0=最新 / 10=更新あり / それ以外=エラー (GUI が分岐に使う)。
    Update {
        /// 更新がある場合、.app をダウンロードして置換・再起動する (macOS のみ)。
        #[arg(long)]
        apply: bool,
    },
    /// AI 呼び出しログ (BYOK dreaming の送信/応答/トークン/コスト) を表示する
    AiLog {
        /// 直近 N 件だけ表示
        #[arg(short = 'n', long, default_value = "10")]
        count: usize,
        /// prompt/response の全文も表示する
        #[arg(long)]
        full: bool,
    },
}

#[derive(Subcommand)]
enum DictAction {
    /// 単語を登録する (例: `nuko dict add わたし 和多志`)。多文字語も一発で登録可。
    Add {
        /// 読み (かな)
        reading: String,
        /// 表層 (変換後の語)
        surface: String,
    },
    /// 登録語を削除する
    Remove {
        /// 読み (かな)
        reading: String,
        /// 表層 (変換後の語)
        surface: String,
    },
    /// 登録語を一覧表示する
    List,
}

#[derive(Subcommand)]
// 全 variant が "Key" 接尾なのは意図的 (すべて API キー操作)。CLI 名 set-key 等を保つ。
#[allow(clippy::enum_variant_names)]
enum ConfigAction {
    /// API キーを Keychain に保存する (値は標準入力から読む。例: `echo $KEY | nuko config set-key anthropic`)
    SetKey {
        /// プロバイダ: anthropic / openrouter
        provider: String,
    },
    /// 保存済みキーの有無を表示する (値は伏せる)
    ShowKey {
        /// プロバイダ: anthropic / openrouter
        provider: String,
    },
    /// 保存済みキーを削除する
    DeleteKey {
        /// プロバイダ: anthropic / openrouter
        provider: String,
    },
}

#[derive(Subcommand)]
enum LearnAction {
    /// 学習状況を表示 (デフォルト)
    Show,
    /// 観察ログをオプトイン ON にする (以後、確定を記録)
    On,
    /// 観察ログを OFF にする (以後、記録しない)
    Off,
    /// 観察ログから訂正選好を今すぐ再学習する
    Relearn {
        /// 選好として採用する最小観察回数
        #[arg(long, default_value = "2")]
        min_seen: u32,
    },
    /// 学習データ (観察ログ + 訂正選好) を削除する
    Clear,
    /// 特定の読みの学習を忘れる (誤学習の取消。観察ログからも除去して再学習で戻さない)
    Forget {
        /// 忘れる読み (かな)。例: いって
        reading: String,
        /// この表層に限定して忘れる (省略時はその読みの全学習)
        #[arg(long)]
        surface: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // ログ設定
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };
    tracing_subscriber::fmt().with_env_filter(filter).init();

    match cli.command {
        Commands::Convert { reading, count } => cmd_convert(&reading, count),
        Commands::Romaji { input } => cmd_romaji(&input),
        Commands::Predict { prefix, count } => cmd_predict(&prefix, count),
        Commands::DictInfo => cmd_dict_info(),
        Commands::Info => cmd_info(),
        Commands::Learn { action } => cmd_learn(action),
        Commands::Dream {
            max_readings,
            provider,
            model,
            apply,
        } => cmd_dream(max_readings, provider.as_deref(), model.as_deref(), apply),
        Commands::Config { action } => cmd_config(action),
        Commands::Models { provider } => cmd_models(&provider),
        Commands::Update { apply } => cmd_update(apply),
        Commands::Dict { action } => cmd_dict(action),
        Commands::AiLog { count, full } => cmd_ai_log(count, full),
    }
}

fn cmd_convert(reading: &str, count: usize) -> Result<()> {
    println!("{}", "ぬこIME 変換テスト".cyan().bold());
    println!("読み: {}", reading.yellow());
    println!();

    let engine = ConversionEngine::new()?;
    let context = nuko_core::conversion::ConversionContext::new();
    let candidates = engine.convert(reading, &context)?;

    println!("{}:", "変換候補".green());
    for (i, candidate) in candidates.iter().take(count).enumerate() {
        let num = format!("{}.", i + 1);
        println!(
            "  {} {} {}",
            num.dimmed(),
            candidate.surface.white().bold(),
            format!("({})", candidate.reading).dimmed()
        );
    }

    if candidates.len() > count {
        println!("  {} 他 {} 件", "...".dimmed(), candidates.len() - count);
    }

    Ok(())
}

fn cmd_romaji(input: &str) -> Result<()> {
    println!("{}", "ぬこIME ローマ字変換".cyan().bold());
    println!("入力: {}", input.yellow());
    println!();

    let mut converter = RomajiConverter::new();
    let result = converter.convert(input)?;

    println!("結果: {}", result.green().bold());

    Ok(())
}

fn cmd_predict(prefix: &str, count: usize) -> Result<()> {
    println!("{}", "ぬこIME 予測変換".cyan().bold());
    println!("入力: {}", prefix.yellow());
    println!();

    let engine = ConversionEngine::new()?;
    let predictions = engine.predict(prefix, count)?;

    if predictions.is_empty() {
        println!("{}", "候補が見つかりませんでした".dimmed());
        return Ok(());
    }

    println!("{}:", "予測候補".green());
    for (i, (reading, candidate)) in predictions.iter().enumerate() {
        let num = format!("{}.", i + 1);
        println!(
            "  {} {} {} {}",
            num.dimmed(),
            candidate.surface.white().bold(),
            format!("[{reading}]").yellow(),
            candidate.pos.as_deref().unwrap_or("-").dimmed()
        );
    }

    Ok(())
}

fn cmd_dict_info() -> Result<()> {
    println!("{}", "ぬこIME 辞書情報".cyan().bold());
    println!();

    let engine = ConversionEngine::new()?;
    let dict = engine.dictionary();

    println!("{}:", "システム辞書".green());
    println!("  種類: IPADIC (デモ版)");
    println!();

    println!("{}:", "ユーザー辞書".green());
    println!("  エントリ数: {}", dict.user_dictionary().len());

    Ok(())
}

fn cmd_info() -> Result<()> {
    println!("{}", "ぬこIME".cyan().bold());
    println!("日本人の、日本人による、日本人のためのIME");
    println!();

    println!("{}:", "バージョン情報".green());
    println!("  nuko-core: {}", nuko_core::VERSION);
    println!("  nuko-platform: {}", nuko_platform::VERSION);
    println!();

    println!("{}:", "ビルド情報".green());
    println!("  Rust Edition: 2021");
    println!("  Target: {}", std::env::consts::ARCH);
    println!("  OS: {}", std::env::consts::OS);
    println!();

    println!("{}:", "ライセンス".green());
    println!("  Apache-2.0 OR MIT");
    println!();

    println!("{}:", "リンク".green());
    println!("  GitHub: https://github.com/your-org/nuko-ime");

    Ok(())
}

/// 学習データの保存ディレクトリ (macOS: ~/Library/Application Support/nuko-ime)。
///
/// nuko-macos の state.rs と同じ場所を指す。CLI からも同じファイルを読み書きする。
fn nuko_data_dir() -> Result<PathBuf> {
    let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME が取得できません"))?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("nuko-ime"))
}

fn cmd_learn(action: Option<LearnAction>) -> Result<()> {
    let dir = nuko_data_dir()?;
    match action.unwrap_or(LearnAction::Show) {
        LearnAction::Show => learn_show(&dir),
        LearnAction::On => learn_toggle(&dir, true),
        LearnAction::Off => learn_toggle(&dir, false),
        LearnAction::Relearn { min_seen } => learn_relearn(&dir, min_seen),
        LearnAction::Clear => learn_clear(&dir),
        LearnAction::Forget { reading, surface } => {
            learn_forget(&dir, &reading, surface.as_deref())
        }
    }
}

/// 特定の読みの学習を忘れる。観察ログから該当イベントを除去して書き直し、
/// 訂正選好を再抽出する (= 再学習しても戻らない、durable な忘却)。
fn learn_forget(dir: &Path, reading: &str, surface: Option<&str>) -> Result<()> {
    use nuko_core::learning::ObservationEvent;

    let obs = ObservationLog::new(true, dir.join("observations.jsonl"));
    let events = obs.read_all()?;
    let before = events.len();

    // 読み一致 (surface 指定時はさらに表層/訂正後一致) を除去。
    let kept: Vec<ObservationEvent> = events
        .into_iter()
        .filter(|ev| {
            if ev.reading() != reading {
                return true; // 別の読みは残す
            }
            match surface {
                None => false, // 読み一致は全部忘れる
                Some(s) => match ev {
                    ObservationEvent::Commit { surface, .. } => surface != s,
                    ObservationEvent::Correction { corrected, .. } => corrected != s,
                },
            }
        })
        .collect();

    let removed = before - kept.len();
    if removed == 0 {
        println!(
            "{} 読み「{}」{}の学習は見つかりませんでした。",
            "…".dimmed(),
            reading.yellow(),
            surface.map(|s| format!("→「{s}」")).unwrap_or_default()
        );
        return Ok(());
    }

    // 観察ログを書き直し → 訂正選好を再抽出して保存 (durable)。
    obs.rewrite(&kept)?;
    let store = extract_corrections(&kept, 2);
    store.save(dir.join("corrections.toml"))?;

    println!(
        "{} 読み「{}」{}の学習を忘れました (観察 {} 件除去、選好 {} 件に再構築)。",
        "✅".green(),
        reading.yellow(),
        surface.map(|s| format!("→「{s}」")).unwrap_or_default(),
        removed.to_string().yellow(),
        store.len().to_string().yellow()
    );
    println!(
        "  {} NukoIME を再起動すると反映されます (入力ソースを切替→戻す)。",
        "注:".dimmed()
    );
    Ok(())
}

/// Layer 3: AI dreaming。観察を要約してプロンプトを組み立て、
/// プロバイダ未指定なら dry-run で表示、指定なら推論→提案マージ。
/// provider 名を正規化・検証する (`claude` は `anthropic` に寄せる)。
fn normalize_provider(p: &str) -> Result<String> {
    match p.to_lowercase().as_str() {
        "anthropic" | "claude" => Ok("anthropic".to_string()),
        "openrouter" => Ok("openrouter".to_string()),
        other => anyhow::bail!("未知のプロバイダ: {other} (anthropic / openrouter)"),
    }
}

/// API キーを環境変数 → Keychain の順で解決する。
fn resolve_key(env_var: &str, provider: &str) -> Option<String> {
    if let Ok(k) = std::env::var(env_var) {
        if !k.is_empty() {
            return Some(k);
        }
    }
    nuko_platform::secret::get_api_key(provider).ok().flatten()
}

/// キーを伏せ字にする (先頭8 + 末尾4 のみ表示)。
///
/// 先頭 8 文字で `sk-ant-a` / `sk-or-v1` の違いが読め、どのプロバイダの鍵か判別しやすい。
fn mask_key(k: &str) -> String {
    let n = k.chars().count();
    if n <= 12 {
        return "*".repeat(n);
    }
    let head: String = k.chars().take(8).collect();
    let tail: String = k.chars().skip(n - 4).collect();
    format!("{head}…{tail} ({n}文字)")
}

/// `nuko ai-log`: AI 呼び出しログ (BYOK dreaming の送信/応答/トークン/コスト) を表示する。
fn cmd_ai_log(count: usize, full: bool) -> Result<()> {
    let all = nuko_core::ai_log::read_all();
    if all.is_empty() {
        println!("AI ログはまだありません（BYOK dreaming を実行すると記録されます）。");
        return Ok(());
    }
    let total_cost: f64 = all.iter().map(|e| e.cost_usd).sum();
    let total_in: u64 = all.iter().map(|e| e.input_tokens).sum();
    let total_out: u64 = all.iter().map(|e| e.output_tokens).sum();
    println!("{}", "ぬこIME AI ログ".cyan().bold());
    println!(
        "累計 {} 回 / 入力 {} tok / 出力 {} tok / 概算 ${:.4}",
        all.len().to_string().yellow(),
        total_in,
        total_out,
        total_cost
    );
    println!();
    for e in all.iter().rev().take(count) {
        let model = e.resolved_model.as_deref().unwrap_or(e.model.as_str());
        println!(
            "{} [{}] {} — in {} / out {} tok / ${:.4} / {}ms",
            e.time.dimmed(),
            e.provider.cyan(),
            model.yellow(),
            e.input_tokens,
            e.output_tokens,
            e.cost_usd,
            e.ms
        );
        if full {
            println!("  {}\n{}", "送信:".dimmed(), e.prompt);
            println!("  {}\n{}", "応答:".dimmed(), e.response);
            println!();
        }
    }
    Ok(())
}

/// `nuko dict …`: IME の user_dict.json を直接編集する (単語登録)。
///
/// IME が読む `~/Library/Application Support/nuko-ime/user_dict.json` に書くので、
/// `nuko-dict` (別パス user.dict) と違い IME に反映される。反映は NukoIME 再起動後。
fn cmd_dict(action: DictAction) -> Result<()> {
    use nuko_core::dictionary::{UserDictionary, UserEntry};
    let path = nuko_data_dir()?.join("user_dict.json");
    let mut dict = if path.exists() {
        UserDictionary::load(&path)?
    } else {
        let mut d = UserDictionary::new();
        d.set_path(&path);
        d
    };
    match action {
        DictAction::Add { reading, surface } => {
            dict.add(UserEntry::new(&surface, &reading))?;
            dict.save()?;
            println!(
                "{} 「{}」→「{}」を登録しました。{} NukoIME 再起動 (入力ソース切替) で反映。",
                "✅".green(),
                reading.yellow(),
                surface.white().bold(),
                "※".dimmed()
            );
        }
        DictAction::Remove { reading, surface } => {
            let removed = dict.remove(&reading, &surface)?;
            dict.save()?;
            if removed {
                println!(
                    "{} 「{}」→「{}」を削除しました。",
                    "✅".green(),
                    reading,
                    surface
                );
            } else {
                println!("{} 該当する登録がありません。", "—".dimmed());
            }
        }
        DictAction::List => {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "[]".to_string());
            let entries: Vec<UserEntry> = serde_json::from_str(&text).unwrap_or_default();
            if entries.is_empty() {
                println!("登録語はありません。");
            } else {
                println!("{} ({} 件):", "ユーザー辞書".cyan().bold(), entries.len());
                for e in &entries {
                    println!("  {} → {}", e.reading.yellow(), e.surface.white().bold());
                }
            }
        }
    }
    Ok(())
}

/// `nuko config …`: BYOK の API キーを Keychain に保存/確認/削除する。
/// `nuko update [--apply]`: GitHub Releases の最新 tag と比較し、更新を確認/適用する。
///
/// 終了コード: 0=最新 / 10=更新あり / 1=エラー (GUI がこのコードで分岐する)。
/// stdout には人間向けの現在/最新/リリース URL を出す。
fn cmd_update(apply: bool) -> Result<()> {
    let current = nuko_core::VERSION;
    let info = update::fetch_latest()?;
    let newer = nuko_core::update::is_newer(current, &info.tag);
    println!(
        "現在: v{current}　最新: {}　{}",
        info.tag,
        if newer {
            "→ 更新があります"
        } else {
            "(最新です)"
        }
    );
    if !info.html_url.is_empty() {
        println!("リリース: {}", info.html_url);
    }
    if !newer {
        return Ok(()); // exit 0 = 最新
    }
    let Some(zip) = info.app_zip_url.as_ref() else {
        println!("自動更新用の .zip が見つかりません。リリースページから手動で更新してください。");
        std::process::exit(10);
    };
    if !apply {
        println!("（メニューの「今すぐ更新」または `nuko update --apply` で自動更新できます）");
        std::process::exit(10); // 10 = 更新あり (未適用)
    }
    #[cfg(target_os = "macos")]
    {
        update::apply_macos(zip)?;
        println!("更新を適用中です。数秒後に NukoIME が再起動します。");
        println!("反映には【入力ソースを一度切替えて戻す】か再ログインが必要です。");
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = zip;
        println!("自動置換は現在 macOS のみ対応です。リリースページから手動で更新してください。");
    }
    Ok(())
}

/// `nuko models <provider>`: 実 API からモデル一覧を取得し id を 1 行ずつ出力する。
fn cmd_models(provider: &str) -> Result<()> {
    let provider = normalize_provider(provider)?;
    let ids = match provider.as_str() {
        "openrouter" => {
            // base_url は ai.toml があればそれ、無ければ既定。公開 API なのでキー不要。
            let base = nuko_data_dir()
                .ok()
                .map(|d| {
                    crate::openrouter::DreamConfig::load(d.join("ai.toml")).unwrap_or_default()
                })
                .map_or_else(
                    || "https://openrouter.ai/api/v1".to_string(),
                    |c| c.base_url,
                );
            models::fetch_openrouter(&base)?
        }
        "anthropic" => {
            let key = resolve_key("ANTHROPIC_API_KEY", "anthropic").ok_or_else(|| {
                anyhow::anyhow!(
                    "Anthropic のモデル取得にはキーが必要です (先に鍵を保存してください)"
                )
            })?;
            models::fetch_anthropic(&key)?
        }
        _ => unreachable!("normalize_provider が検証済み"),
    };
    // おすすめ (Claude 系・特に Sonnet) を上位に (400件超から選びやすく)。
    for id in models::recommended_first(ids) {
        println!("{id}");
    }
    Ok(())
}

fn cmd_config(action: ConfigAction) -> Result<()> {
    match action {
        ConfigAction::SetKey { provider } => {
            let provider = normalize_provider(&provider)?;
            // キーは標準入力から読む (シェル履歴や ps に残さないため)。
            use std::io::Read;
            let mut key = String::new();
            std::io::stdin().read_to_string(&mut key)?;
            let key = key.trim();
            if key.is_empty() {
                anyhow::bail!(
                    "キーが空です。標準入力で渡してください (例: echo -n $KEY | nuko config set-key anthropic)"
                );
            }
            nuko_platform::secret::set_api_key(&provider, key)?;
            println!(
                "{} {} の API キーを Keychain に保存しました。",
                "✅".green(),
                provider.cyan()
            );
        }
        ConfigAction::ShowKey { provider } => {
            let provider = normalize_provider(&provider)?;
            match nuko_platform::secret::get_api_key(&provider)? {
                Some(k) => println!(
                    "🔑 {}: 保存済み ({})",
                    provider.cyan(),
                    mask_key(&k).dimmed()
                ),
                None => println!("— {}: 未登録", provider.cyan()),
            }
        }
        ConfigAction::DeleteKey { provider } => {
            let provider = normalize_provider(&provider)?;
            nuko_platform::secret::delete_api_key(&provider)?;
            println!(
                "{} {} の API キーを削除しました。",
                "✅".green(),
                provider.cyan()
            );
        }
    }
    Ok(())
}

fn cmd_dream(
    max_readings: usize,
    provider: Option<&str>,
    model: Option<&str>,
    apply: bool,
) -> Result<()> {
    let dir = nuko_data_dir()?;
    let obs = ObservationLog::new(true, dir.join("observations.jsonl"));
    let events = obs.read_all().unwrap_or_default();
    let store = CorrectionStore::load(dir.join("corrections.toml")).unwrap_or_default();
    let digest = digest_observations(&events, max_readings);
    let prompt = build_prompt(&digest, &store);

    println!("{}", "ぬこIME dreaming (Layer 3)".cyan().bold());
    println!(
        "観察 {} 件 → 読み {} 件を要約 / 学習済み {} 件",
        events.len().to_string().yellow(),
        digest.readings.len().to_string().yellow(),
        store.len().to_string().yellow()
    );
    println!();

    // ai.toml (BYOK 設定) を 1 回だけ読む。--provider 未指定なら ai.toml を使う:
    // enabled かつ provider 設定済みなら実行、でなければ dry-run。
    // → IME メニューは引数なし `nuko dream --apply` で ai.toml 駆動の実 dream ができる。
    let cfg = crate::openrouter::DreamConfig::load(dir.join("ai.toml")).unwrap_or_default();
    let effective_provider: Option<String> = provider
        .map(String::from)
        .or_else(|| cfg.enabled.then(|| cfg.provider.clone()).flatten());
    let Some(provider_name) = effective_provider else {
        // dry-run: 送信されるプロンプトをそのまま見せる (API キー不要)。
        println!(
            "{}",
            "── プロバイダ未指定 / ai.toml 未設定: dry-run (以下が送信されるプロンプト) ──"
                .dimmed()
        );
        println!("{prompt}");
        println!();
        println!(
            "  {} 実推論は `nuko dream --provider anthropic --model <id>` (要キー)。\n  \
             または ai.toml に enabled=true / provider=\"anthropic\" / model=\"...\" を書けば \
             `nuko dream --apply` だけで動きます。キーは `nuko config set-key` で Keychain に。",
            "ヒント:".dimmed()
        );
        return Ok(());
    };
    let provider_name = provider_name.as_str();

    // プロバイダを解決 (mock / openrouter)。
    let provider: Box<dyn DreamProvider> = match provider_name {
        "mock" => Box::new(MockProvider::empty()),
        "openrouter" => {
            use crate::openrouter::{DreamConfig, OpenRouterProvider};
            let cfg = DreamConfig::load(dir.join("ai.toml"))?;
            let api_key = resolve_key("OPENROUTER_API_KEY", "openrouter").ok_or_else(|| {
                anyhow::anyhow!(
                    "OPENROUTER_API_KEY が未設定です。環境変数か \
                     `nuko config set-key openrouter` (Keychain) で設定してください。"
                )
            })?;
            let model = model.map(String::from).or(cfg.model).ok_or_else(|| {
                anyhow::anyhow!(
                    "モデル未指定です。--model か ai.toml の model を設定してください \
                         (例: anthropic/claude-3.5-sonnet)。"
                )
            })?;
            println!(
                "  {} openrouter / model={}",
                "プロバイダ:".dimmed(),
                model.cyan()
            );
            Box::new(OpenRouterProvider::new(api_key, model, cfg.base_url))
        }
        "anthropic" | "claude" => {
            use crate::anthropic::{model_hint, AnthropicProvider};
            let cfg = crate::openrouter::DreamConfig::load(dir.join("ai.toml"))?;
            let api_key = resolve_key("ANTHROPIC_API_KEY", "anthropic").ok_or_else(|| {
                anyhow::anyhow!(
                    "ANTHROPIC_API_KEY が未設定です。環境変数か \
                     `nuko config set-key anthropic` (Keychain) で設定してください。"
                )
            })?;
            let model = model.map(String::from).or(cfg.model).ok_or_else(|| {
                anyhow::anyhow!(
                    "モデル未指定です。--model か ai.toml の model を設定してください ({})。",
                    model_hint()
                )
            })?;
            println!(
                "  {} anthropic / model={}",
                "プロバイダ:".dimmed(),
                model.cyan()
            );
            Box::new(AnthropicProvider::new(api_key, model))
        }
        other => {
            anyhow::bail!("未知のプロバイダ: {other} (\"mock\" / \"openrouter\" / \"anthropic\")");
        }
    };

    let raw = provider.complete(&prompt)?;
    let proposal = parse_proposal(&raw)?;
    let (merged, report) = merge_proposals(&store, &proposal, 2);

    println!(
        "{} が {} 件提案 → 新規 {} 件 / 恒等除外 {} 件 / 既存重複 {} 件",
        provider.name().cyan(),
        proposal.proposals.len(),
        report.added.len().to_string().green(),
        report.skipped_identity.len(),
        report.skipped_existing.len()
    );
    for p in &report.added {
        println!(
            "  {} → {}  {}",
            p.reading.yellow(),
            p.prefer.white().bold(),
            "(AI提案)".dimmed()
        );
    }

    if apply && !report.added.is_empty() {
        merged.save(dir.join("corrections.toml"))?;
        println!(
            "{} corrections.toml に保存しました。NukoIME 再起動で反映されます。",
            "✅".green()
        );
    } else if !report.added.is_empty() {
        println!(
            "  {} 保存するには `--apply` を付けてください。",
            "注:".dimmed()
        );
    }
    Ok(())
}

/// 学習した選好を一覧表示する共通ヘルパ。
fn print_preferences(store: &CorrectionStore) {
    for p in &store.preferences {
        println!(
            "  {} → {}  {}",
            p.reading.yellow(),
            p.prefer.white().bold(),
            format!("(seen {})", p.seen).dimmed()
        );
    }
}

fn learn_show(dir: &Path) -> Result<()> {
    println!("{}", "ぬこIME 学習状況".cyan().bold());
    println!("データ: {}", dir.display().to_string().dimmed());
    println!();

    // オプトイン状態
    let enabled = dir.join("OBSERVE_ENABLED").exists();
    let status = if enabled {
        "ON".green().bold()
    } else {
        "OFF".red().bold()
    };
    println!("観察ログ (オプトイン): {status}");
    if !enabled {
        println!(
            "  {} `nuko learn on` で有効化 (デフォルトは収集しません)",
            "ヒント:".dimmed()
        );
    }

    // 観察イベント数
    let obs = ObservationLog::new(true, dir.join("observations.jsonl"));
    let count = obs.count().unwrap_or(0);
    println!("観察イベント: {} 件", count.to_string().yellow());

    // 学習した訂正選好
    let store = CorrectionStore::load(dir.join("corrections.toml")).unwrap_or_default();
    println!();
    println!(
        "{} ({} 件):",
        "学習した変換選好".green().bold(),
        store.len()
    );
    if store.is_empty() {
        println!(
            "  {}",
            "(まだありません。使い込んでから `nuko learn relearn`)".dimmed()
        );
    } else {
        print_preferences(&store);
    }

    Ok(())
}

fn learn_toggle(dir: &Path, on: bool) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let marker = dir.join("OBSERVE_ENABLED");
    if on {
        std::fs::write(&marker, "")?;
        println!(
            "{} 観察ログを {} にしました。",
            "✅".green(),
            "ON".green().bold()
        );
        println!(
            "  {} NukoIME を再起動すると反映されます (入力ソースを切替→戻す)。",
            "注:".dimmed()
        );
    } else {
        if marker.exists() {
            std::fs::remove_file(&marker)?;
        }
        println!(
            "{} 観察ログを {} にしました (以後、記録しません)。",
            "✅".green(),
            "OFF".red().bold()
        );
    }
    Ok(())
}

fn learn_relearn(dir: &Path, min_seen: u32) -> Result<()> {
    let obs = ObservationLog::new(true, dir.join("observations.jsonl"));
    let events = obs.read_all()?;
    let store = extract_corrections(&events, min_seen);
    let path = dir.join("corrections.toml");
    store.save(&path)?;

    println!(
        "{} 観察 {} 件から選好 {} 件を再学習しました。",
        "✅".green(),
        events.len(),
        store.len().to_string().yellow()
    );
    if !store.is_empty() {
        print_preferences(&store);
    }
    println!(
        "  {} NukoIME を再起動すると変換に反映されます。",
        "注:".dimmed()
    );
    Ok(())
}

fn learn_clear(dir: &Path) -> Result<()> {
    let mut removed = Vec::new();
    for name in ["observations.jsonl", "corrections.toml"] {
        let p = dir.join(name);
        if p.exists() {
            std::fs::remove_file(&p)?;
            removed.push(name);
        }
    }
    if removed.is_empty() {
        println!("{}", "削除するデータはありませんでした。".dimmed());
    } else {
        println!(
            "{} 学習データを削除しました: {}",
            "✅".green(),
            removed.join(", ")
        );
    }
    println!(
        "  {} オプトイン設定 (ON/OFF) は変更していません。",
        "注:".dimmed()
    );
    Ok(())
}
