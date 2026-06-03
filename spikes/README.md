# spikes/

短命の検証用 crate を置く場所。

メインの Cargo workspace からは **意図的に外している** (各 spike の `Cargo.toml` 末尾に `[workspace]` の空テーブルを置く)。
これにより spike 用の依存が本体の `Cargo.lock` を膨らませない。

## 既存の spike

- `libakaza-build/` — Path B 検証 (libakaza が macOS でビルドできるか) → 2026-05-26 ✅。詳細は [`docs/spikes/libakaza-api-survey.md`](../docs/spikes/libakaza-api-survey.md) の前段に該当。
- `libakaza-no-model/` — モデル不在時の `BigramWordViterbiEngineBuilder::build()` の Err 挙動検証 → 2026-06-02 ✅。詳細は [`docs/spikes/libakaza-no-model-spike.md`](../docs/spikes/libakaza-no-model-spike.md)。

## 運用ルール

- `**/target/` と `**/Cargo.lock` は `.gitignore` で除外する (`spikes/.gitignore` 参照)。
- spike が本実装に昇格した場合、`spikes/` 配下の crate は削除する (役目を終えたら証跡は `docs/spikes/` のメモに残し、コードは消す)。
- spike の `Cargo.toml` 末尾には必ず `[workspace]` の空テーブルを置いて、ルートの workspace から切り離す。
