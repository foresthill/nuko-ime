# Mozc 語彙ブレンド spike — 実測結果 (B: ①語彙ブレンド)

作成: 2026-11-22 / 対象: nuko-ime / 前提: [`2026-11-22-mozc-integration-eval.md`](2026-11-22-mozc-integration-eval.md)

> Mozc 統合評価で「最有望」とした **①(a) 語彙ブレンド** を実際に試し、精度ベンチ (C) で
> before/after を**実測**した記録。推測でなく数値で採否を判断する（推測禁止・グローバル規約）。

## やったこと

1. `model-pipeline/scripts/mozc_to_corpus.py`: Mozc OSS 辞書 (`dictionary00.txt`〜`09.txt`、
   BSD-3-Clause / google/mozc) から**表層に漢字を含むエントリ**を抽出し `surface/yomi` corpus 化
   （既存 `sudachi_to_corpus.py` と同型）。→ **751,735 語**抽出。
2. `make-dict` に `--corpus work/mozc-kanji-corpus.txt` を追加して `SKK-JISYO.akaza` を再生成。
   辞書サイズ **47MB → 61MB (+30%)**、生成は +17 秒。エンジン (libakaza) は一切変更なし。
3. `nuko eval --libakaza`（C のベンチ）で before/after を測定。

## 実測結果（`--release`）

### (1) 常用語 Golden Set（`eval/golden.tsv` 31 件）— 回帰チェック

| metric | baseline | +Mozc blend | 差 |
|---|---|---|---|
| Top-1 | 80.65% | 80.65% | **±0（回帰なし）** |
| Top-3 | 90.32% | 90.32% | ±0 |
| Top-5 | 96.77% | 96.77% | ±0 |
| MRR | 0.869 | 0.869 | ±0 |
| Purity@5 | 85.81% | 87.10% | +1.3 |

→ 常用語はもともと UniDic+Sudachi で拾えているので**変化なし**。重要なのは**悪化ゼロ**（安全）。

### (2) rare な Mozc 語彙 300 件（coverage 測定）— ランダム抽出

| metric | baseline | +Mozc blend | 差 |
|---|---|---|---|
| Top-1 | 34.67% | **66.67%** | **+32.0** |
| Top-3 | 43.67% | 85.33% | +41.7 |
| Top-5 | 46.00% | **89.67%** | **+43.7** |
| MRR | 0.396 | 0.765 | +0.37 |

→ baseline は Mozc の rare 語彙の**半分以上を候補に出せていなかった**（Top-5 46%）。語彙ブレンドで
**Top-5 90% まで被覆**。これが「語彙穴が埋まる」の実体。

## 解釈（過大評価しない）

- **効くのは「rare な固有名詞・熟語の被覆」**。常用語の精度 (Golden) は動かない。日常文の体感が
  劇的に変わるわけではなく、「打った固有名詞・珍しい熟語が候補に出る率」が上がる。
- **Top-1 化には限界**（rare 語は blend 後も Top-1 66.67%）。追加語は corpus-stats の n-gram LM に
  無く **fallback スコア**になるため、**Top-5 には入るが Top-1 で勝ちにくい**。完全な順位改善には
  コスト補間（①(b)、未実装）が要る。これは評価文書の予測どおり。
- **コスト = 辞書 +30%・ビルド時 58MB DL**。desktop IME としては許容範囲。

## 判断：**採用（既定 ON、回帰なし・被覆が実測で改善）**

- `make all` の既定で Mozc 語彙ブレンドを有効化（`MOZC_BLEND=0` で無効化可）。
- 安全（Golden 回帰ゼロ）かつ rare 語彙の被覆が明確に改善するため、採用する。
- ただし「劇的な日常精度向上」ではない。日常精度は A(curation) と将来の順位改善（コスト補間/
  モデル再訓練）の領域。

## 残・次手

- **①(b) コスト補間**（corpus-stats への Mozc 頻度合流 or `engine.rs` スコア補間）で、blend した
  rare 語の Top-1 率を上げる。効果はこのベンチで測る。
- **Golden Set 拡充**（森岡さんの関心事）: 今回の測定で分かったとおり、**ranking 用 Golden だけだと
  coverage 改善が見えない**。coverage 用の rare 語セット（固有名詞・専門語）を別途持つと B 系の効果を
  追跡できる。Silver Set 自動生成（Gemini C-1）も将来。
- `MOZC_REF` は既定 `master`（再現性重視なら commit/tag 固定を推奨）。

## 再現手順

```bash
cd model-pipeline && make all PROFILE=min          # 既定で Mozc ブレンド込み
# 反映:
MODEL_DIR="$HOME/Library/Application Support/nuko-ime/akaza-model"
cp -v data/SKK-JISYO.akaza "$MODEL_DIR/" && killall NukoIME
# 測定:
cargo run --release --features akaza -p nuko-cli --bin nuko -- eval --libakaza --show-misses
```

---

（ソース: 実測 2026-11-22 `nuko eval --libakaza`、mozc_to_corpus.py 出力 751,735 語、
model-pipeline/Makefile の make-dict --corpus 追加。Mozc OSS 辞書 = google/mozc BSD-3-Clause。）
