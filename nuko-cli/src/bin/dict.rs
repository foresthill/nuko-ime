//! 辞書管理ツール

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use nuko_core::dictionary::{DictionaryManager, UserDictionary};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "nuko-dict")]
#[command(author, version, about = "ぬこIME 辞書管理ツール")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// ユーザー辞書にエントリを追加
    Add {
        /// 表層形（変換結果）
        surface: String,
        /// 読み（ひらがな）
        reading: String,
        /// 品詞
        #[arg(short, long)]
        pos: Option<String>,
        /// 辞書ファイルのパス
        #[arg(short, long)]
        dict: Option<PathBuf>,
    },
    /// ユーザー辞書からエントリを削除
    Remove {
        /// 表層形
        surface: String,
        /// 読み
        reading: String,
        /// 辞書ファイルのパス
        #[arg(short, long)]
        dict: Option<PathBuf>,
    },
    /// ユーザー辞書の内容を表示
    List {
        /// 辞書ファイルのパス
        #[arg(short, long)]
        dict: Option<PathBuf>,
        /// フィルター（読みの前方一致）
        #[arg(short, long)]
        filter: Option<String>,
    },
    /// 単語を検索
    Search {
        /// 検索する読み
        reading: String,
    },
    /// 辞書をインポート（CSV形式）
    Import {
        /// インポートするCSVファイル
        file: PathBuf,
        /// 出力先辞書ファイル
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// 辞書をエクスポート（CSV形式）
    Export {
        /// 辞書ファイルのパス
        #[arg(short, long)]
        dict: Option<PathBuf>,
        /// 出力先CSVファイル
        output: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Add {
            surface,
            reading,
            pos,
            dict,
        } => cmd_add(&surface, &reading, pos.as_deref(), dict),
        Commands::Remove {
            surface,
            reading,
            dict,
        } => cmd_remove(&surface, &reading, dict),
        Commands::List { dict, filter } => cmd_list(dict, filter.as_deref()),
        Commands::Search { reading } => cmd_search(&reading),
        Commands::Import { file, output } => cmd_import(&file, output),
        Commands::Export { dict, output } => cmd_export(dict, &output),
    }
}

fn cmd_add(surface: &str, reading: &str, pos: Option<&str>, dict_path: Option<PathBuf>) -> Result<()> {
    println!("{}", "ユーザー辞書にエントリを追加".cyan().bold());

    let path = dict_path.unwrap_or_else(default_dict_path);
    let mut dict = UserDictionary::load(&path)?;

    use nuko_core::dictionary::UserEntry;
    let mut entry = UserEntry::new(surface, reading);
    if let Some(p) = pos {
        entry = entry.with_pos(p);
    }

    dict.add(entry)?;
    dict.save()?;

    println!("  表層形: {}", surface.green());
    println!("  読み: {}", reading.yellow());
    if let Some(p) = pos {
        println!("  品詞: {}", p.dimmed());
    }
    println!();
    println!("{}", "エントリを追加しました".green());

    Ok(())
}

fn cmd_remove(surface: &str, reading: &str, dict_path: Option<PathBuf>) -> Result<()> {
    println!("{}", "ユーザー辞書からエントリを削除".cyan().bold());

    let path = dict_path.unwrap_or_else(default_dict_path);
    let mut dict = UserDictionary::load(&path)?;

    let removed = dict.remove(reading, surface)?;
    if removed {
        dict.save()?;
        println!("  表層形: {}", surface.green());
        println!("  読み: {}", reading.yellow());
        println!();
        println!("{}", "エントリを削除しました".green());
    } else {
        println!("  表層形: {}", surface.yellow());
        println!("  読み: {}", reading.yellow());
        println!();
        println!("{}", "該当するエントリが見つかりませんでした".red());
    }

    Ok(())
}

fn cmd_list(dict_path: Option<PathBuf>, filter: Option<&str>) -> Result<()> {
    println!("{}", "ユーザー辞書の内容".cyan().bold());

    let path = dict_path.unwrap_or_else(default_dict_path);
    println!("辞書: {}", path.display().to_string().dimmed());
    println!();

    let dict = UserDictionary::load(&path)?;
    let entries = dict.all_entries();

    if entries.is_empty() {
        println!("{}", "辞書は空です".dimmed());
        return Ok(());
    }

    if let Some(f) = filter {
        println!("フィルター: {}", f.yellow());
        println!();
    }

    println!(
        "  {} {} {}",
        "読み".underline(),
        "表層形".underline(),
        "品詞".underline()
    );

    let mut count = 0;
    for entry in &entries {
        if filter.map_or(true, |f| entry.reading.starts_with(f)) {
            let pos = entry.pos.as_deref().unwrap_or("-");
            println!("  {} {} {}", entry.reading.yellow(), entry.surface.green(), pos.dimmed());
            count += 1;
        }
    }

    println!();
    println!("合計: {} 件", count);

    Ok(())
}

fn cmd_search(reading: &str) -> Result<()> {
    println!("{}", "辞書検索".cyan().bold());
    println!("読み: {}", reading.yellow());
    println!();

    let manager = DictionaryManager::new()?;
    let candidates = manager.lookup(reading)?;

    if candidates.is_empty() {
        println!("{}", "候補が見つかりませんでした".red());
    } else {
        println!("{}:", "検索結果".green());
        for candidate in &candidates {
            let source = match candidate.source {
                nuko_core::conversion::CandidateSource::System => "[システム]",
                nuko_core::conversion::CandidateSource::User => "[ユーザー]",
                nuko_core::conversion::CandidateSource::Learned => "[学習]",
            };
            println!(
                "  {} {} {}",
                candidate.surface.white().bold(),
                candidate.reading.dimmed(),
                source.dimmed()
            );
        }
    }

    Ok(())
}

fn cmd_import(file: &PathBuf, output: Option<PathBuf>) -> Result<()> {
    println!("{}", "辞書インポート".cyan().bold());
    println!("入力: {}", file.display());

    let out = output.unwrap_or_else(default_dict_path);
    println!("出力: {}", out.display());
    println!();
    println!("{}", "インポートが完了しました".green());
    println!("(注: デモ版では実際のインポートは行われません)");

    Ok(())
}

fn cmd_export(dict_path: Option<PathBuf>, output: &PathBuf) -> Result<()> {
    println!("{}", "辞書エクスポート".cyan().bold());

    let path = dict_path.unwrap_or_else(default_dict_path);
    println!("入力: {}", path.display());
    println!("出力: {}", output.display());
    println!();
    println!("{}", "エクスポートが完了しました".green());
    println!("(注: デモ版では実際のエクスポートは行われません)");

    Ok(())
}

fn default_dict_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("nuko-ime")
        .join("user.dict")
}

mod dirs {
    use std::path::PathBuf;

    pub fn data_local_dir() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            std::env::var("LOCALAPPDATA").ok().map(PathBuf::from)
        }

        #[cfg(target_os = "macos")]
        {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
        }

        #[cfg(target_os = "linux")]
        {
            std::env::var("XDG_DATA_HOME")
                .ok()
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var("HOME")
                        .ok()
                        .map(|h| PathBuf::from(h).join(".local").join("share"))
                })
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            None
        }
    }
}
