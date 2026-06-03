//! spike-2: モデル不在状態で BigramWordViterbiEngineBuilder::build() の挙動を確認する。
//!
//! ねらい: Phase 1.2 で実装するフォールバック契約 (= モデルなし時は libakaza 側で
//! Err を返し、nuko-core 側で既存 DictionaryManager にフォールバック) を裏付けるため、
//! 「どんな Error 型/メッセージが返るか」を実機で観察する。

use std::error::Error as StdError;

use libakaza::config::EngineConfig;
use libakaza::engine::bigram_word_viterbi_engine::BigramWordViterbiEngineBuilder;
use libakaza::graph::reranking::ReRankingWeights;

fn main() {
    let nonexistent_path = "/tmp/nuko-ime-spike2-no-such-model-dir-EXIST-IMPOSSIBLE";

    let config = EngineConfig {
        dicts: vec![],
        dict_cache: false,
        model: nonexistent_path.to_string(),
        reranking_weights: ReRankingWeights::default(),
    };

    println!("=== spike-2: BigramWordViterbiEngineBuilder::build() with missing model ===");
    println!("model path: {nonexistent_path}");
    println!();

    let builder = BigramWordViterbiEngineBuilder::new(config);
    match builder.build() {
        Ok(_engine) => {
            println!("UNEXPECTED: build() succeeded despite missing model files.");
            println!("→ Fallback policy needs reconsideration.");
            std::process::exit(2);
        }
        Err(e) => {
            println!("Expected Err (anyhow::Error).");
            println!();
            println!("Display:");
            println!("  {e}");
            println!();
            println!("Debug:");
            println!("  {e:?}");
            println!();
            println!("Error chain:");
            let mut current: Option<&dyn StdError> = Some(e.as_ref());
            let mut depth = 0;
            while let Some(err) = current {
                println!("  [{depth}] {err}");
                current = err.source();
                depth += 1;
            }
            println!();
            println!("=> Fallback policy validated: catch the Err and route to DictionaryManager.");
        }
    }
}
