//! ベンチマークツール

use anyhow::Result;
use clap::Parser;
use colored::*;
use nuko_core::prelude::*;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "nuko-bench")]
#[command(author, version, about = "ぬこIME ベンチマークツール")]
struct Cli {
    /// 反復回数
    #[arg(short, long, default_value = "1000")]
    iterations: usize,

    /// ウォームアップ回数
    #[arg(short, long, default_value = "100")]
    warmup: usize,

    /// テストデータ
    #[arg(short, long)]
    data: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("{}", "ぬこIME ベンチマーク".cyan().bold());
    println!("反復回数: {}", cli.iterations);
    println!("ウォームアップ: {}", cli.warmup);
    println!();

    // テストデータ
    let test_cases = if let Some(data) = cli.data {
        vec![data]
    } else {
        vec![
            "nihongo".to_string(),
            "henkan".to_string(),
            "toukyou".to_string(),
            "kyoutofu".to_string(),
            "puroguramu".to_string(),
        ]
    };

    // ローマ字変換ベンチマーク
    println!("{}", "=== ローマ字変換 ===".green().bold());
    bench_romaji(&test_cases, cli.warmup, cli.iterations)?;

    println!();

    // かな変換ベンチマーク
    println!("{}", "=== かな→漢字変換 ===".green().bold());
    bench_conversion(cli.warmup, cli.iterations)?;

    Ok(())
}

fn bench_romaji(test_cases: &[String], warmup: usize, iterations: usize) -> Result<()> {
    for input in test_cases {
        // ウォームアップ
        for _ in 0..warmup {
            let mut conv = RomajiConverter::new();
            let _ = conv.convert(input);
        }

        // 計測
        let mut durations = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let mut conv = RomajiConverter::new();
            let start = Instant::now();
            let _ = conv.convert(input);
            durations.push(start.elapsed());
        }

        // 統計
        let stats = calculate_stats(&durations);
        print_stats(input, &stats);
    }

    Ok(())
}

fn bench_conversion(warmup: usize, iterations: usize) -> Result<()> {
    let engine = ConversionEngine::new()?;
    let context = nuko_core::conversion::ConversionContext::new();

    let test_readings = vec!["にほん", "へんかん", "とうきょう", "きょうと", "ぷろぐらむ"];

    for reading in test_readings {
        // ウォームアップ
        for _ in 0..warmup {
            let _ = engine.convert(reading, &context);
        }

        // 計測
        let mut durations = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            let _ = engine.convert(reading, &context);
            durations.push(start.elapsed());
        }

        // 統計
        let stats = calculate_stats(&durations);
        print_stats(reading, &stats);
    }

    Ok(())
}

struct BenchStats {
    min: Duration,
    max: Duration,
    mean: Duration,
    median: Duration,
    p95: Duration,
    p99: Duration,
}

fn calculate_stats(durations: &[Duration]) -> BenchStats {
    let mut sorted: Vec<_> = durations.to_vec();
    sorted.sort();

    let total: Duration = sorted.iter().sum();
    let mean = total / sorted.len() as u32;
    let median = sorted[sorted.len() / 2];
    let p95 = sorted[(sorted.len() as f64 * 0.95) as usize];
    let p99 = sorted[(sorted.len() as f64 * 0.99) as usize];

    BenchStats {
        min: sorted[0],
        max: sorted[sorted.len() - 1],
        mean,
        median,
        p95,
        p99,
    }
}

fn print_stats(label: &str, stats: &BenchStats) {
    println!("  {}", label.yellow());
    println!(
        "    平均: {:>10.3}μs  中央: {:>10.3}μs",
        stats.mean.as_nanos() as f64 / 1000.0,
        stats.median.as_nanos() as f64 / 1000.0
    );
    println!(
        "    最小: {:>10.3}μs  最大: {:>10.3}μs",
        stats.min.as_nanos() as f64 / 1000.0,
        stats.max.as_nanos() as f64 / 1000.0
    );
    println!(
        "    P95:  {:>10.3}μs  P99:  {:>10.3}μs",
        stats.p95.as_nanos() as f64 / 1000.0,
        stats.p99.as_nanos() as f64 / 1000.0
    );

    // 目標判定
    let target_us = 10_000.0; // 10ms = 10000μs
    let mean_us = stats.mean.as_nanos() as f64 / 1000.0;
    if mean_us < target_us {
        println!("    → {} (目標: <10ms)", "PASS".green().bold());
    } else {
        println!("    → {} (目標: <10ms)", "FAIL".red().bold());
    }
}
