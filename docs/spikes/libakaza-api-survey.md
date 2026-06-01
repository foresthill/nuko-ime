# libakaza 公開 API 調査メモ

**調査日**: 2026-06-02
**対象 rev**: [`akaza-im/akaza@8a40428`](https://github.com/akaza-im/akaza/commit/8a404281ece7ca51119127a96bdde8c153b0df61)
**目的**: Phase 1.2 「libakaza を `nuko-core::conversion::engine` に統合」の設計判断材料を集める。

> 注: 本ドキュメントは GitHub Contents API 経由で取得した libakaza 上記 rev のソース読みに基づく。引用箇所は当該 rev の `libakaza/src/` 配下の実ファイル。

---

## 1. ライブラリ全体像

### 1.1 ライセンス

- libakaza 本体: **MIT** ([LICENSE](https://github.com/akaza-im/akaza/blob/8a404281ece7ca51119127a96bdde8c153b0df61/LICENSE), Copyright 2023 Tokuhiro Matsuno) ✅ ぬこIME の MIT/Apache-2.0 dual と矛盾なし。
- `default-model/NOTICE` には **SKK-JISYO.L (GPL-2.0)** が含まれる旨明記 ❌ → デフォルトモデルをそのまま同梱・配布はできない。
- 結論: **ライブラリは git 依存で OK、モデルは自前で生成する** (Path B 既定方針通り)。

### 1.2 公開モジュール (`libakaza/src/lib.rs`)

```rust
#![allow(dead_code)]

pub mod config;
pub mod consonant;
pub mod corpus;
pub mod cost;
pub mod dict;
pub mod engine;
pub mod extend_clause;
pub mod graph;
pub mod kana_kanji;
pub mod kana_trie;
pub mod kansuji;
pub mod keymap;
pub mod lm;
pub mod numeric_counter;
pub(crate) mod resource;
pub mod romkan;
pub mod search_result;
pub mod user_side_data;
pub(crate) mod xdg_dirs;
```

統合に直接必要なのは `engine`, `graph::candidate`, `config` の 3 つ。`romkan` や `keymap` は libakaza 側にもあるが、ぬこIME 側に既存実装があるので **使わない** 想定。

---

## 2. コア API: `engine::base::HenkanEngine` トレイト

`libakaza/src/engine/base.rs` 全文:

```rust
use std::ops::Range;

use crate::graph::candidate::Candidate;
use crate::graph::graph_resolver::KBestPath;

pub trait HenkanEngine {
    fn learn(&mut self, candidates: &[Candidate]);

    fn convert(
        &self,
        yomi: &str,
        force_ranges: Option<&[Range<usize>]>,
    ) -> anyhow::Result<Vec<Vec<Candidate>>>;

    /// k-best ビタビで上位 k 個の分節パターンを返す。
    fn convert_k_best(
        &self,
        yomi: &str,
        force_ranges: Option<&[Range<usize>]>,
        k: usize,
    ) -> anyhow::Result<Vec<KBestPath>>;
}
```

ポイント:
- 入力は `yomi: &str` の **平仮名文字列**。ローマ字レベルの処理は libakaza の外で完結させる必要がある (= ぬこIME の `nuko-core::input` がそのまま使える)。
- 戻り値 `Vec<Vec<Candidate>>` は **文節 (segments) × 候補 (candidates)** の二段構造。例えば「わたしのなまえ」なら `[[私, 渡し, ...], [の, ...], [名前, ...]]`。
- `force_ranges` は文節区切りを強制したいときの byte range。通常時 `None` で良い。

### 2.1 `Candidate` 型 (`graph/candidate.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub surface: String,
    pub yomi: String,
    pub cost: f32,            // 低いほど良い (logprob ベース)
    pub compound_word: bool,  // 複合語: 学習時にユーザー辞書登録が必要
}
```

ぬこIME 既存の `nuko_core::conversion::Candidate` との差分:

| libakaza | nuko-core | 変換 |
|---|---|---|
| `surface: String` | `surface: String` | そのまま |
| `yomi: String` | `reading: String` | フィールド名のみ変更 |
| `cost: f32` (低=良) | `score: i32` (高=良) | `score = -(cost * 100.0) as i32` 程度で反転 |
| `compound_word: bool` | (なし) | 学習層に伝える必要あり |
| (なし) | `pos: Option<String>` | libakaza には品詞情報なし → `None` |
| (なし) | `source: CandidateSource` | `CandidateSource::System` 固定 (将来 `Engine` バリアントを追加してもよい) |

---

## 3. 実装本体: `BigramWordViterbiEngine`

`libakaza/src/engine/bigram_word_viterbi_engine.rs` 抜粋:

```rust
pub struct BigramWordViterbiEngine<U: SystemUnigramLM, B: SystemBigramLM, KD: KanaKanjiDict> {
    graph_builder: GraphBuilder<U, B, KD>,
    pub segmenter: Segmenter,
    pub graph_resolver: GraphResolver,
    pub user_data: Arc<Mutex<UserData>>,
    reranking_weights: ReRankingWeights,
    skip_bigram_lm: Option<Rc<MarisaSystemSkipBigramLM>>,
}
```

ぬこIME 側で使う具体型:

```rust
BigramWordViterbiEngine<MarisaSystemUnigramLM, MarisaSystemBigramLM, MarisaKanaKanjiDict>
```

### 3.1 Builder API

```rust
pub struct BigramWordViterbiEngineBuilder {
    user_data: Option<Arc<Mutex<UserData>>>,
    config: EngineConfig,
}

impl BigramWordViterbiEngineBuilder {
    pub fn new(config: EngineConfig) -> Self;
    pub fn user_data(&mut self, user_data: Arc<Mutex<UserData>>) -> &mut Self;
    pub fn build(&self) -> Result<BigramWordViterbiEngine<..., ..., MarisaKanaKanjiDict>>;
}
```

`build()` が読み込むファイル (`bigram_word_viterbi_engine.rs::build()`):

1. `<model_name>/unigram.model` (`MarisaSystemUnigramLM::load`)
2. `<model_name>/bigram.model` (`MarisaSystemBigramLM::load`)
3. `<model_name>/skip_bigram.model` (Optional; 無くても進む)
4. `<model_name>/SKK-JISYO.akaza` (必須; かな漢字辞書)

→ **ぬこIME 側で必要なもの**: 上記 4 ファイルを生成するパイプライン (Phase 2 の本体)。

### 3.2 `EngineConfig`

`config.rs` 抜粋:

```rust
pub struct EngineConfig {
    pub dicts: Vec<DictConfig>,
    pub dict_cache: bool,
    pub model: String,         // モデルディレクトリ名 (XDG data dir 配下を期待)
    pub reranking_weights: ReRankingWeights,
}
```

`default_engine_config()` は XDG 経由で SKK-JISYO.L を探しに行く。**ぬこIME では XDG に依存せず、明示的に絶対パスを渡す** 設計にしたい (macOS の Application Support 配下を使う想定)。

→ libakaza の `resource::detect_resource_path` は `pub(crate)` なので外から呼べない。代わりに `EngineConfig` を直接構築して、`model` フィールドにフルパス (もしくはモデルディレクトリの絶対パス) を渡せばよい。

---

## 4. 統合設計案

### 4.1 アーキテクチャ位置

```
┌─────────────────────┐
│  nuko-macos (IMK)   │
└──────┬──────────────┘
       │ key events
┌──────▼──────────────┐
│ nuko-core::input    │  ローマ字 → かな (ぬこIME 自前)
└──────┬──────────────┘
       │ "わたしのなまえ" (hiragana)
┌──────▼──────────────────────────────────────────┐
│ nuko-core::conversion::engine::ConversionEngine │
│ ┌─────────────────────────────────────────────┐ │
│ │ LibakazaBackend (NEW)                       │ │
│ │  - BigramWordViterbiEngine<Marisa..>        │ │  ← libakaza (MIT)
│ │  - フォールバック: SystemDictionary         │ │  ← 既存
│ └─────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────┘
```

### 4.2 feature flag 戦略

`nuko-core/Cargo.toml`:

```toml
[features]
default = []
lindera = ["dep:lindera"]              # 既存
akaza = ["dep:libakaza"]               # 新規 (Phase 1.2)

[dependencies]
libakaza = { git = "https://github.com/akaza-im/akaza", rev = "8a404281...", optional = true }
```

CI とローカル開発の標準ビルドは **当面 default**=`[]` のまま (= libakaza 非依存) にして、`cargo build -p nuko-core --features akaza` で有効化する。
理由: モデル生成パイプライン (Phase 2) が動くまでは、libakaza を有効化してもモデルファイル不在で起動不能 → デフォルト有効にする意味がない。

### 4.3 フォールバック契約

`ConversionEngine` の挙動:

| 状態 | 動作 |
|---|---|
| `akaza` feature 無効 | 既存通り `DictionaryManager` のみ |
| `akaza` feature 有効、モデルファイル不在 | `BigramWordViterbiEngineBuilder::build()` が `Err` → **警告ログを出して既存 `DictionaryManager` にフォールバック** (起動は成功させる) |
| `akaza` feature 有効、モデルファイル存在 | libakaza で変換 → segments を flatten して `CandidateList` に詰める |

学習データ (`LearningManager` / `UserData`) との橋渡しは Phase 1.2 のスコープ外 (Phase 3) とする。当面 libakaza の `user_data` は `Arc::new(Mutex::new(UserData::default()))` でデフォルト初期化。

### 4.4 segments flatten 戦略 (議論ポイント)

libakaza は `Vec<Vec<Candidate>>` (segments × candidates per segment) を返すので、現状のフラットな `CandidateList` API とは噛み合わない。
3 通りの案:

| 案 | 内容 | trade-off |
|---|---|---|
| A | segments を文字列連結して 1 候補に。文節別候補は `convert_k_best(k=N)` で取得 | 既存 API 互換だが、libakaza の文節別変換能力を活用できない |
| B | `CandidateList` を文節対応に拡張 (`Vec<SegmentCandidates>` 化) | libakaza の能力をフル活用できるが、`nuko-core` の API が大きく変わる |
| C | Phase 1.2 では A で動かし、Phase 1.3 で B にリファクタ | スコープを切れる |

**推奨: C**。Phase 1.2 は「libakaza が動いている」ことを最小限の改修で示すことを優先する。

---

## 5. 次のステップ (Phase 1.2 サブタスク)

1. **spike-2**: モデル不在状態で `BigramWordViterbiEngineBuilder::build()` がどのようなエラーを返すか実機検証 (現在の spike1 は link 検証のみ)
2. `nuko-core/Cargo.toml` に `akaza` feature と optional dep を追加
3. `nuko-core/src/conversion/backend/libakaza.rs` 新設 → `LibakazaBackend` 実装
4. `ConversionEngine::convert()` を改修: libakaza を試して失敗時は `DictionaryManager` にフォールバック
5. 統合テスト追加: 「モデル不在でも convert が空エラーにならない」「モデルあり (テスト用最小モデル) で日本語変換が返る」
6. **Phase 2 開始**: モデル生成パイプライン (SudachiDict + UniDic + Wikipedia → unigram/bigram/skip_bigram/SKK-JISYO.akaza)

---

## 6. 関連ドキュメント

- [ARCHITECTURE.md](../ARCHITECTURE.md) — 全体構造とライセンス境界
- [ROADMAP.md](../ROADMAP.md) — Phase 1〜5 の段取り
- [FUTURE_FEATURES.md](../FUTURE_FEATURES.md) §8 — 学習機構の設計 (Phase 3)
