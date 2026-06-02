# spike-2: libakaza モデル不在時のエラー挙動

**実行日**: 2026-06-02
**spike**: [`spikes/libakaza-no-model/`](../../spikes/libakaza-no-model/)
**libakaza rev**: [`akaza-im/akaza@8a40428`](https://github.com/akaza-im/akaza/commit/8a404281ece7ca51119127a96bdde8c153b0df61)
**目的**: Phase 1.2 フォールバック契約 (libakaza 失敗 → `DictionaryManager` フォールバック) のエラー型と粒度を実機で確認する。

## 検証コード

```rust
let config = EngineConfig {
    dicts: vec![],
    dict_cache: false,
    model: "/tmp/nuko-ime-spike2-no-such-model-dir-EXIST-IMPOSSIBLE".to_string(),
    reranking_weights: ReRankingWeights::default(),
};
let builder = BigramWordViterbiEngineBuilder::new(config);
builder.build()
```

## 観察結果

`cargo run --manifest-path spikes/libakaza-no-model/Cargo.toml` の出力:

```
Expected Err (anyhow::Error).

Display:
  No such file or directory (os error 2)

Debug:
  No such file or directory (os error 2)

Error chain:
  [0] No such file or directory (os error 2)
```

- **Err 型**: `anyhow::Error` (返り値の通り)
- **Display / Debug**: `No such file or directory (os error 2)` のみ
- **`source()` chain の深さ**: 1 (root のみ、wrap なし)
- **どのファイルで死んだかの情報**: なし

## 解釈

1. **fallback トリガー判定は単純で OK**: `Err(_)` を素直にキャッチして既存 `DictionaryManager` に流せばよい。エラー内容で分岐する必要なし。
2. **libakaza 側のエラーは不親切**: どのモデルファイル (`unigram.model` / `bigram.model` / `SKK-JISYO.akaza`) が無いかが Err からは取れない。`bigram_word_viterbi_engine::build()` 先頭の `MarisaSystemUnigramLM::load(.../unigram.model)` で死んでいるはず (推測 — 順序が unigram → bigram → skip → SKK)。
3. **nuko-core 側で context を補足する責任がある**: ユーザーに「モデル未配置だから静的辞書にフォールバックした」と分かるよう、`nuko-core` 側で `tracing::warn!` を出すべき。例:

   ```rust
   match LibakazaBackend::try_new(&model_dir) {
       Ok(backend) => Some(backend),
       Err(e) => {
           tracing::warn!(
               model_dir = %model_dir.display(),
               error = %e,
               "libakaza モデル読み込みに失敗。静的辞書にフォールバック"
           );
           None
       }
   }
   ```

## Phase 1.2 への含意

- `LibakazaBackend::try_new(model_dir)` は `Result<Self>` を返し、内部で `BigramWordViterbiEngineBuilder` を呼ぶ。
- `ConversionEngine` は **起動時に 1 度だけ** libakaza の load を試み、`Option<LibakazaBackend>` を保持する (毎回試すと無駄なファイル I/O)。
- `convert()` は `if let Some(backend)` で分岐し、`None` なら既存 `DictionaryManager` フローに落とす。

## 未調査 (Phase 1.2 本実装で確認)

- ディレクトリ自体は存在し、`unigram.model` だけ無い時のエラー
- 3 ファイル揃っているが SKK-JISYO.akaza だけ無い時のエラー (build 順の最後)
- 壊れたモデルファイル (zero byte, ランダムバイナリ等) の挙動
- → これらは「テスト用最小モデル」を Phase 2 で作るときにテストケースとして拾えばよい
