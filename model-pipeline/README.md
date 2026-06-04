# model-pipeline/ — libakaza モデル生成パイプライン

ぬこIME (Path B = libakaza ベース) で使う統計的かな漢字変換モデル
(`unigram.model` / `bigram.model` / `skip_bigram.model` / `SKK-JISYO.akaza`) を
生成するためのビルドスクリプト群。

> **現状**: Phase 2-B (scaffold) 段階。本物のビルドは Phase 2-C 以降。
> 詳細は [`docs/spikes/phase2-model-pipeline.md`](../docs/spikes/phase2-model-pipeline.md)。

## 設計方針

- **モノレポ内に配置** (別 repo は作らない)。コード本体と独立した CI workflow で運用
- **生成物は GitHub Releases に添付** (リポジトリは軽いまま、`work/` `data/` は gitignore)
- **モデルのライセンス継承**: Wikipedia (CC BY-SA) / UniDic (BSD-3) / SudachiDict (Apache) / IPADIC (BSD-3) → [`NOTICE`](./NOTICE) 参照
- **ぬこIME 本体は MIT/Apache-2.0 維持** (モデルは「データ」なのでコード側ライセンスを汚染しない)

## 前提ツール

| ツール | 用途 | インストール |
|---|---|---|
| `akaza-data` | モデル生成 CLI (libakaza 同梱) | `cargo install --git https://github.com/akaza-im/akaza --rev <pin> --root tools akaza-data` |
| `gh` | GitHub Releases から corpus-stats 等を取得 | https://cli.github.com/ |
| `wget` / `unzip` / `zstd` | データ展開 | macOS: `brew install wget zstd` |
| Python 3 | 前処理 (将来) | システム同梱 |

詳細は [`Makefile`](./Makefile) のターゲット定義を参照。

## ビルド (Phase 2-C 以降で実装)

```bash
# Phase 2-C: 最小モデル (epochs を下げて高速試作)
make all PROFILE=min

# Phase 2-E: 本番モデル
make all PROFILE=full
```

実行後の生成物 (PROFILE=min での実測、2026-06-04):
```
data/
├── SKK-JISYO.akaza         42 MB
├── unigram.model           16 MB
├── bigram.model            53 MB
├── bigram.model.scores     35 MB  ← bigram.model と必ずペア
├── skip_bigram.model       94 MB
└── skip_bigram.model.scores 58 MB ← skip_bigram.model と必ずペア
                            合計 ~298 MB
```

⚠️ **`.scores` ファイルは bigram/skip_bigram の本体スコアデータ**。
コピー漏れすると libakaza が silent failure (load 時に `No such file or directory`、
nuko-ime は静的辞書フォールバックに落ちて気付かれない)。

これを tarball にして GitHub Releases に添付するのが Phase 2-E。
エンドユーザーは tarball を `~/Library/Application Support/nuko-ime/akaza-model/` に
展開して使う ([Phase 2-F: 手動配置の手順](#phase-2-f-手動配置の手順) 参照)。

## Phase 2-F: 手動配置の手順

PROFILE=min/full でローカルビルドしたモデルを nuko-ime IME 本体で使うまで:

### 1. モデルを期待される場所に配置

`data/` 配下の **全 6 ファイル** を `~/Library/Application Support/nuko-ime/akaza-model/` にコピーする (`.scores` 漏れ注意):

```bash
mkdir -p ~/Library/Application\ Support/nuko-ime/akaza-model
cp model-pipeline/data/* ~/Library/Application\ Support/nuko-ime/akaza-model/
```

`*` で全部コピーするのが確実。`unigram.model` `bigram.model` `bigram.model.scores`
`skip_bigram.model` `skip_bigram.model.scores` `SKK-JISYO.akaza` の 6 つが揃って
いることを確認:

```bash
ls -lh ~/Library/Application\ Support/nuko-ime/akaza-model/
# 6 ファイル、合計 ~300 MB
```

別のパスに置きたい場合は環境変数 `NUKO_AKAZA_MODEL_DIR=<path>` で上書き可能。

### 2. `--features akaza` でビルド・インストール

```bash
FEATURES=akaza ./nuko-macos/scripts/install.sh
```

`FEATURES` 環境変数は [PR #23](https://github.com/foresthill/nuko-ime/pull/23) で
追加。指定なしだと既存通り (静的辞書のみ) ビルド。

### 3. NukoIME プロセスを再起動

モデルファイル更新後は **必ずプロセス再起動** が必要。`LibakazaBackend::try_new`
は起動時 1 回だけ呼ばれるため (PR #16 設計)、走行中のプロセスは古い状態のまま:

```bash
killall NukoIME
```

macOS の入力ソースを別の IME に一旦切り替え → ぬこIME に戻すと自動再起動する。

### 4. 動作確認

任意のテキストフィールドで以下が漢字変換できることを確認:

| 入力 (ローマ字) | 期待される 1 番目の候補 |
|---|---|
| `nihongo` | 日本語 |
| `watashinonamae` | 私の名前 |
| `kyouhaiitenki` | 今日はいい天気 |
| `nihongowohanasu` | 日本語を話す |

うまく変換されない場合は `/tmp/nuko-ime-debug.log` に IMK callback のログが
蓄積されているので参照。「全てカタカナになる」「Space 何回押しても漢字にならない」
等は典型的に `.scores` 漏れ or プロセス未再起動が原因。

## ディレクトリ構造

```
model-pipeline/
├── README.md            # このファイル
├── NOTICE               # データソース別ライセンス継承
├── Makefile             # ビルドスクリプト (現状は骨格のみ)
├── dict/
│   └── SKK-JISYO.akaza  # 手書き seed 辞書 (上流流用、Phase 2-E で自前置換)
│                        # ※ akaza-data make-dict が dict/SKK-JISYO.akaza を
│                        #    ハードコードで探すため、ディレクトリ名は dict/ 固定
├── scripts/             # 前処理ヘルパー (Python/シェル、Phase 2-C で追加)
├── tools/               # cargo install 先 (gitignore)
├── work/                # ダウンロード/展開の中間ファイル (gitignore)
└── data/                # 最終生成モデル (gitignore、Release に添付)
```

## Phase 進行と本ディレクトリの対応

- **Phase 2-B**: scaffold (README / NOTICE / Makefile 骨格 / dict seed 上流流用)
- **Phase 2-C**: 最小モデル試作 (青空文庫 + SudachiDict 小規模、epochs を下げる)
- **Phase 2-D**: ぬこIME 本体で `NUKO_AKAZA_MODEL_DIR` 指定して実機検証
- **Phase 2-E**: 本番モデル (上流 corpus-stats + UniDic + dict/SKK-JISYO.akaza の自前置換)
- **Phase 2-F**: 手動配置のドキュメント (ぬこIME 本体 README)
- **Phase 2-G**: 自動ダウンローダ (将来)

## 関連ドキュメント

- [`docs/spikes/phase2-model-pipeline.md`](../docs/spikes/phase2-model-pipeline.md) — Phase 2 全体段取り
- [`docs/spikes/akaza-data-cli-spike.md`](../docs/spikes/akaza-data-cli-spike.md) — akaza-data CLI subcommand 仕様
- [`docs/spikes/libakaza-api-survey.md`](../docs/spikes/libakaza-api-survey.md) — libakaza 公開 API
- [`docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md) / [`docs/ROADMAP.md`](../docs/ROADMAP.md)
