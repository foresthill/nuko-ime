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

実行後の生成物:
```
data/
├── SKK-JISYO.akaza
├── unigram.model
├── bigram.model
└── skip_bigram.model
```

これを tarball にして GitHub Releases に添付するのが Phase 2-E。
エンドユーザーは tarball を `~/Library/Application Support/nuko-ime/akaza-model/` に
展開して使う (Phase 2-F の手動配置パス)。

## ディレクトリ構造

```
model-pipeline/
├── README.md            # このファイル
├── NOTICE               # データソース別ライセンス継承
├── Makefile             # ビルドスクリプト (現状は骨格のみ)
├── seed/
│   └── SKK-JISYO.akaza  # 手書き seed 辞書 (上流流用、Phase 2-E で自前置換)
├── scripts/             # 前処理ヘルパー (Python/シェル、Phase 2-C で追加)
├── tools/               # cargo install 先 (gitignore)
├── work/                # ダウンロード/展開の中間ファイル (gitignore)
└── data/                # 最終生成モデル (gitignore、Release に添付)
```

## Phase 進行と本ディレクトリの対応

- **Phase 2-B (本コミット)**: scaffold (README / NOTICE / Makefile 骨格 / seed 流用)
- **Phase 2-C**: 最小モデル試作 (青空文庫 + SudachiDict 小規模、epochs を下げる)
- **Phase 2-D**: ぬこIME 本体で `NUKO_AKAZA_MODEL_DIR` 指定して実機検証
- **Phase 2-E**: 本番モデル (上流 corpus-stats + UniDic + 自前 seed への置換)
- **Phase 2-F**: 手動配置のドキュメント (ぬこIME 本体 README)
- **Phase 2-G**: 自動ダウンローダ (将来)

## 関連ドキュメント

- [`docs/spikes/phase2-model-pipeline.md`](../docs/spikes/phase2-model-pipeline.md) — Phase 2 全体段取り
- [`docs/spikes/akaza-data-cli-spike.md`](../docs/spikes/akaza-data-cli-spike.md) — akaza-data CLI subcommand 仕様
- [`docs/spikes/libakaza-api-survey.md`](../docs/spikes/libakaza-api-survey.md) — libakaza 公開 API
- [`docs/ARCHITECTURE.md`](../docs/ARCHITECTURE.md) / [`docs/ROADMAP.md`](../docs/ROADMAP.md)
