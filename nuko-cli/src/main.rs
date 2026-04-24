//! ぬこIME CLI

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use nuko_core::prelude::*;
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
            format!("[{}]", reading).yellow(),
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
