# Mozc データ vendoring NOTICE

このディレクトリは Google Mozc プロジェクトから派生したデータファイルを含みます。

## ライセンス

**BSD 3-Clause License**
Copyright 2010-2018, Google Inc. All rights reserved.

完全なライセンス文は同ディレクトリの `LICENSE` を参照。

## 取得元

- リポジトリ: https://github.com/google/mozc
- パス: `src/data/preedit/romanji-hiragana.tsv`
- 取得時の固定コミット SHA: `60af02ff797275f2ba1b7fddccdec916798d112e`
- 取得日: 2026-04-25

## 含まれるファイル

| ファイル | 内容 | 行数 |
|---------|------|------|
| `romanji-hiragana.tsv` | ローマ字 → ひらがな変換テーブル | 323 |
| `LICENSE` | Mozc プロジェクトの BSD-3 ライセンス全文 | 111 |

## TSV のフォーマット

タブ区切り、以下の2形式が混在:

```
romaji<TAB>hiragana                  # 通常変換 (例: nn<TAB>ん)
romaji<TAB>hiragana<TAB>next_state   # 促音 (例: kk<TAB>っ<TAB>k)
```

`next_state` 列は促音処理で使われ、変換後にバッファを保持する文字を指定する。

## 改変について

このリポジトリ内では、上記ファイルを改変せずそのままコピーしている。
ぬこIME のローマ字パーサーが起動時にこの TSV を読み込んで使用する。

## ライセンス互換性

- ぬこIME 本体: MIT OR Apache-2.0
- このディレクトリのデータ: BSD-3-Clause (Google Inc.)

BSD-3 は MIT / Apache-2.0 と互換。BSD-3 の義務 (著作権表示・ライセンス文の保持・
非推奨条項) は本 NOTICE と LICENSE ファイルで満たされている。

## 上流の更新方針

ローマ字 → ひらがな変換テーブルは事実上ほぼ不変 (日本語ローマ字入力規則は
何十年も安定) のため、自動追従はせず手動更新する。
更新時は本 NOTICE のコミット SHA と取得日を更新すること。
