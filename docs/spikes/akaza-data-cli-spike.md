# spike-4: akaza-data CLI ビルド・動作確認

**実行日**: 2026-06-03
**spike**: [`spikes/akaza-data-cli/`](../../spikes/akaza-data-cli/)
**libakaza/akaza-data rev**: [`akaza-im/akaza@8a40428`](https://github.com/akaza-im/akaza/commit/8a404281ece7ca51119127a96bdde8c153b0df61)
**目的**: Phase 2-A として `akaza-data` CLI が macOS でビルドでき、Phase 2-C で使う subcommand 表面 (特に `make-dict` / `learn-corpus`) を実機の `--help` で確定する。

## 結果サマリ

| 項目 | 結果 |
|---|---|
| ビルド | ✅ 成功 (release, 33.71s) |
| 場所 | `spikes/akaza-data-cli/install/bin/akaza-data` (~6.5 MB on macOS arm64) |
| バージョン | `akaza-data 2026.404.0` |
| Subcommand 数 | 16 |
| Phase 2-C 必須 subcommand | `make-dict`, `learn-corpus`, `check` (動作確認用) |

## インストールコマンド

```bash
mkdir -p spikes/akaza-data-cli/install
cargo install --git https://github.com/akaza-im/akaza \
  --rev 8a404281ece7ca51119127a96bdde8c153b0df61 \
  --root spikes/akaza-data-cli/install \
  akaza-data
```

`--root` でローカル install することで `~/.cargo/bin` を汚さない。`install/` は gitignore 対象 (`spikes/.gitignore` に登録)。

## トップレベル `--help` 出力

```
Usage: akaza-data [OPTIONS] <COMMAND>

Commands:
  tokenize                   コーパスを形態素解析機でトーカナイズする
  tokenize-line              一行の自然文をトーカナイズする
  wfreq                      トーカナイズされたコーパスから単語頻度ファイルを生成する
  vocab                      単語頻度ファイルから語彙リストを生成する
  make-dict                  システム辞書ファイルを作成する。
  wordcnt-unigram            ユニグラム言語モデルを作成する。
  wordcnt-bigram             システム言語モデルを生成する。
  wordcnt-skip-bigram        skip-bigram 言語モデルを生成する。
  learn-corpus               コーパスから言語モデルを学習する
  check                      かな漢字変換を実行する（CLI テスト用）
  evaluate                   変換精度を評価する
  bench                      インクリメンタル変換のベンチマーク
  dump-unigram-dict          ユニグラム辞書ファイルをダンプする
  dump-bigram-dict           バイグラム辞書ファイルをダンプする
  convert-skip-bigram-model  wordcnt skip-bigram trie → skip_bigram.model に変換
  model-info                 モデルファイルのメタデータを表示する
  help                       Print this message or the help of the given subcommand(s)
```

## Phase 2-C で使う subcommand の詳細

### `make-dict` — SKK-JISYO.akaza 生成

```
Usage: akaza-data make-dict [OPTIONS] --unidic <UNIDIC> --vocab <VOCAB> <TXT_FILE>

Arguments:
  <TXT_FILE>  デバッグのための中間テキストファイル

Options:
  -c, --corpus <CORPUS>            (複数指定可、Vec<String>)
  -u, --unidic <UNIDIC>            必須: UniDic lex CSV
      --vocab <VOCAB>              必須: vibrato-ipadic.vocab
      --sudachi-lex <SUDACHI_LEX>  Sudachi 辞書 CSV (固有名詞補完、複数指定可)
```

**注意**: `--unidic` と `--vocab` は **必須**。`--corpus` と `--sudachi-lex` は任意・複数指定可。
`<TXT_FILE>` は positional 引数 (デバッグ出力)。

Phase 2-C で最小モデル試作する場合は:
- UniDic: 必須なので `model-pipeline/Makefile` 内で download
- vocab: 上流 corpus-stats tarball 同梱の `vibrato-ipadic.vocab` を流用
- corpus: 上流 `training-corpus/{must,should,may}.txt` を流用 (seed 二段階で後で自前置換)
- sudachi-lex: 任意だが、Phase 2-C でも `small_lex.csv` だけは入れる方向

### `learn-corpus` — unigram/bigram/skip_bigram.model 生成

```
Usage: akaza-data learn-corpus [OPTIONS] --delta <DELTA>
       <MAY_CORPUS> <SHOULD_CORPUS> <MUST_CORPUS>
       <SRC_UNIGRAM> <SRC_BIGRAM>
       <DST_UNIGRAM> <DST_BIGRAM>

Arguments (positional, 順番固定):
  <MAY_CORPUS>     训练用コーパス (may, 弱い hint)
  <SHOULD_CORPUS>  训练用コーパス (should, 中の hint)
  <MUST_CORPUS>    训练用コーパス (must, 強い hint)
  <SRC_UNIGRAM>    入力: stats-vibrato-unigram.wordcnt.trie (上流 corpus-stats)
  <SRC_BIGRAM>     入力: stats-vibrato-bigram.wordcnt.trie  (上流 corpus-stats)
  <DST_UNIGRAM>    出力: unigram.model
  <DST_BIGRAM>     出力: bigram.model

Options:
  -d, --delta <DELTA>                      必須 (上流 Makefile では 2000)
      --may-epochs <MAY_EPOCHS>            [default: 10]
      --should-epochs <SHOULD_EPOCHS>      [default: 100]
      --must-epochs <MUST_EPOCHS>          [default: 1000]
      --src-skip-bigram <SRC_SKIP_BIGRAM>  任意: stats-vibrato-skip-bigram.wordcnt.trie
      --dst-skip-bigram <DST_SKIP_BIGRAM>  --src-skip-bigram 指定時は必須: skip_bigram.model
```

**重要**: positional 引数の **順番が固定** (may → should → must → src_unigram → src_bigram → dst_unigram → dst_bigram)。
上流 Makefile では `--must-epochs=10000` を渡しており、デフォルトの 1000 より大きい (上流側で経験的にチューニング済み)。

### `check` — 動作確認 (Phase 2-D の前段)

```
Usage: akaza-data check [OPTIONS] [YOMI] [EXPECTED]

Options (抜粋):
  -m, --model-dir <MODEL_DIR>   モデルデータ格納ディレクトリ
  -n, --candidates <CANDIDATES> 各文節の候補数 [default: 1]
  -k, --k-best <K_BEST>         k-best 分節パターン数
  -f, --format <FORMAT>         text|json [default: text]
```

Phase 2-C で最小モデルを作った後、ぬこIME を通さず直接:
```
akaza-data check -m model-pipeline/data/ にほんごをはなす
```
で **libakaza が読める形のモデルが生成できているか** を最速で確認できる。
Phase 2-D (nuko-ime 経由での実機検証) の前に、まずここで疎通する。

## Phase 2-B/2-C への含意

1. **`model-pipeline/Makefile` の必要要素** (上流 Makefile を nuko-ime 用に minimal 化):
   - `akaza-data` を `cargo install --root ./tools` 等でビルド (CI でもローカルでも)
   - 上流 `akaza-corpus-stats` tarball を `gh release download` でダウンロード (~270 MB)
   - UniDic, IPADIC vocab を必要に応じて取得
   - SudachiDict (`small_lex.csv` だけで十分かは Phase 2-C で確認)
   - `make-dict` → `learn-corpus` の順で実行
2. **Phase 2-C の最小モデル戦略**:
   - 上流 corpus-stats tarball を使う (60 GB の生テキストには触らない)
   - `--should-epochs=10`, `--must-epochs=100` 等で **学習を短縮** して試作品を高速生成
   - `akaza-data check` で疎通確認
   - スコープ外: 品質 (Phase 2-E の本番モデルでチューニング)
3. **CI runtime 見積もり** (Phase 2-E の本番モデル):
   - 上流 Makefile の epochs (must=10000) で `learn-corpus` を回すと、上流の CI 時間が参考になる
   - 単に「動く」ことだけなら epochs を下げて数分〜数十分で完了する見込み
4. **`check` の存在は嬉しい誤算**: nuko-ime 配線前に libakaza+モデルが動くことを CLI で証明できる

## 関連ドキュメント

- [`phase2-model-pipeline.md`](./phase2-model-pipeline.md) — Phase 2 全体段取り
- [`libakaza-api-survey.md`](./libakaza-api-survey.md) — libakaza 公開 API
- [`libakaza-no-model-spike.md`](./libakaza-no-model-spike.md) — モデル不在エラー (spike-2)
- [`libakaza-send-constraint.md`](./libakaza-send-constraint.md) — Send 制約 (spike-3)
