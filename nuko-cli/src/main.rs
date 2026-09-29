//! ぬこIME CLI

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
        /// 推論プロバイダ (現状 "mock" のみ。未指定なら dry-run でプロンプトを表示)
        #[arg(long)]
        provider: Option<String>,
        /// 提案を corrections.toml にマージして保存する (プロバイダ指定時のみ有効)
        #[arg(long)]
        apply: bool,
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
            apply,
        } => cmd_dream(max_readings, provider.as_deref(), apply),
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
fn cmd_dream(max_readings: usize, provider: Option<&str>, apply: bool) -> Result<()> {
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

    let Some(provider_name) = provider else {
        // dry-run: 送信されるプロンプトをそのまま見せる (API キー不要)。
        println!(
            "{}",
            "── プロバイダ未指定: dry-run (以下が送信されるプロンプト) ──".dimmed()
        );
        println!("{prompt}");
        println!();
        println!(
            "  {} 実推論は `nuko dream --provider mock` (雛形)。実 LLM 連携は今後。",
            "ヒント:".dimmed()
        );
        return Ok(());
    };

    // プロバイダを解決 (現状 mock のみ)。
    let provider: Box<dyn DreamProvider> = match provider_name {
        "mock" => Box::new(MockProvider::empty()),
        other => {
            anyhow::bail!("未知のプロバイダ: {other} (現状 \"mock\" のみ対応)");
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
