# spike-3: libakaza バックエンドが `!Send` で nuko-macos 配線がブロックされる

**発見日**: 2026-06-03
**libakaza rev**: [`akaza-im/akaza@8a40428`](https://github.com/akaza-im/akaza/commit/8a404281ece7ca51119127a96bdde8c153b0df61)
**前提**: PR #15 / #16 で `LibakazaBackend` + `ConversionEngine::with_libakaza` を導入済み。

## 何が起きたか

`nuko-macos/src/state.rs` の共有エンジンを `with_libakaza` 経由に切り替えようとした:

```rust
pub static ENGINE: LazyLock<Mutex<ConversionEngine>> =
    LazyLock::new(|| Mutex::new(build_engine().expect("ConversionEngine の初期化に失敗")));
```

`cargo build -p nuko-macos --features akaza` が以下で失敗:

```
error[E0277]: `Rc<libakaza::lm::system_unigram_lm::MarisaSystemUnigramLM>` cannot be sent between threads safely
error[E0277]: `Rc<libakaza::lm::system_bigram::MarisaSystemBigramLM>` cannot be sent between threads safely
error[E0277]: `Rc<libakaza::lm::system_skip_bigram::MarisaSystemSkipBigramLM>` cannot be sent between threads safely
error[E0277]: `Rc<(dyn libakaza::lm::base::SystemSkipBigramLM + 'static)>` cannot be sent between threads safely
error[E0277]: `(dyn libakaza::kana_trie::base::KanaTrie + 'static)` cannot be sent between threads safely
```

## 原因

`libakaza::engine::bigram_word_viterbi_engine::BigramWordViterbiEngine` の内部に `Rc<...>` が含まれている (`skip_bigram_lm: Option<Rc<MarisaSystemSkipBigramLM>>` ほか、`GraphBuilder` / `Segmenter` の構築物にも複数)。

`Rc<T>` は `!Send` なので、`LibakazaBackend` も `!Send`。結果として `ConversionEngine` に `Option<LibakazaBackend>` を持たせた瞬間、akaza feature 有効時は `ConversionEngine` 全体が `!Send` になる。

`LazyLock<Mutex<T>>: Sync` は `T: Send` を要求するため、コンパイル時にはじかれる。

## nuko-core 側で気づけなかった理由

nuko-core 内のテストは `LibakazaBackend` を threaded context (LazyLock / Arc<Mutex> 等) に置いていなかった。コンパイラは「使われていなければ Send 不要」と判断するため、`cargo test -p nuko-core --features akaza` は通っていた。問題は downstream (nuko-macos) で初めて表面化した。

## 取りうるアプローチ

| 案 | 内容 | 工数 | 副作用 |
|---|---|---|---|
| A | **`thread_local!` で per-thread エンジン** | 小 | モデルがスレッドごとにロード (メモリ増)。IMK callback がメインスレッド固定なら実用上 OK。 |
| B | **`unsafe impl Send`** で握り潰す | 小 | シリアル使用を呼び出し側で保証する必要。安全性の責任が広がる。 |
| C | **専用スレッド + チャネル** で libakaza を駆動 | 中 | 呼び出しコスト増。設計複雑化。 |
| D | **libakaza 上流に Rc → Arc 変更を PR** | 大 | 上流マージ待ち。長期戦。 |
| E | **`ConversionEngine` から `LibakazaBackend` を切り出し**、platform 層で別管理 | 中 | 公開 API 変更。`convert` の合成ロジック分散。 |

## 推奨

**案 A (thread_local) が当面の現実解**。理由:

1. macOS IMK の callback は概ねメインスレッド (NSApplication run loop) で動作する。serialize されている前提でメインスレッド 1 個に閉じれば問題なし。
2. nuko-core 側の API (`with_libakaza`) は変更不要。`nuko-macos/src/state.rs` の `LazyLock<Mutex<>>` を `thread_local!` パターンに置き換えるだけ。
3. 案 D (上流 PR) と平行で進めれば、上流が直ったら案 A から戻せばよい。

副次的に、**nuko-core にコンパイル時の Send 仮定を入れる** ことで再発防止:

```rust
// nuko-core/src/conversion/backend/libakaza.rs (test mod)
#[cfg(test)]
#[test]
fn libakaza_backend_is_not_send_by_design() {
    // libakaza 内部の Rc<> 由来で !Send。downstream (nuko-macos 等) は
    // thread_local 等で対処すること。詳細は docs/spikes/libakaza-send-constraint.md。
    fn assert_not_send<T: ?Sized>() where T: NotSendAssertion {}
    // 直接的な !Send アサートは Rust では書けないので、
    // ConversionEngine が akaza feature 時に Send かどうかの test を上位に書く。
}
```

→ 実際には `static_assertions::assert_not_impl_all!` が使える。本 PR の範囲外として将来課題。

## 次のステップ (別 PR)

1. **方針確定**: 案 A (thread_local) で進める旨をプロジェクトに記録 (本ドキュメント = 記録)。
2. **実装**: `nuko-macos/src/state.rs` を `thread_local!` パターンに移行。
3. **テスト**: 実機 (macOS) で `with_libakaza` が呼ばれていることをログで確認 (モデル不在でも fallback が動けば OK)。
4. **将来課題**: 上流 libakaza に Rc → Arc PR (アクセプトされたら案 A から戻す)。

## 関連

- [`docs/spikes/libakaza-api-survey.md`](./libakaza-api-survey.md) — API 全体像
- [`docs/spikes/libakaza-no-model-spike.md`](./libakaza-no-model-spike.md) — モデル不在エラー挙動 (spike-2)
- PR #15 / #16 — LibakazaBackend + ConversionEngine wire-up
