//! Phase 2-C 疎通確認: model-pipeline/data/ のモデルを LibakazaBackend で読み込む。

use std::path::PathBuf;

use nuko_core::conversion::backend::LibakazaBackend;

fn main() {
    let model_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // デフォルト: model-pipeline/data/
            std::env::current_dir()
                .unwrap()
                .join("../../model-pipeline/data")
        });

    println!("=== Phase 2-C smoke test ===");
    println!("model_dir: {}", model_dir.display());
    println!();

    let backend = match LibakazaBackend::try_new(&model_dir) {
        Ok(b) => {
            println!("✅ LibakazaBackend::try_new succeeded");
            b
        }
        Err(e) => {
            println!("❌ LibakazaBackend::try_new failed: {e}");
            std::process::exit(1);
        }
    };

    let test_inputs = ["にほん", "にほんご", "わたしのなまえ", "きょうはいいてんき"];

    for input in test_inputs {
        match backend.convert(input) {
            Ok(candidates) => {
                if candidates.is_empty() {
                    println!("⚠️  '{input}' → (empty candidates)");
                } else {
                    let preview: Vec<String> = candidates
                        .iter()
                        .take(3)
                        .map(|c| format!("'{}' (score={})", c.surface, c.score))
                        .collect();
                    println!("✅ '{input}' → {}", preview.join(", "));
                }
            }
            Err(e) => {
                println!("❌ '{input}' → Err: {e}");
            }
        }
    }

    println!();
    println!("Smoke test complete.");
}
