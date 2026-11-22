# Mozc 統合の是非 — 外部AI(Gemini/Carbon)レビューと nuko 実装に基づく比較検討

作成: 2026-11-22 / 対象: nuko-ime
関連: [`2026-11-22-curation-plan-gemini-review.md`](2026-11-22-curation-plan-gemini-review.md)（A/B/C/D 全体計画）

> 「ロングテール品質を上げるために Mozc へ統合（乗り換え・組み込み）すべきか？」という問いへの
> Gemini の回答を**一次入力としてそのまま保存**し、nuko-ime の**実コード・実パイプラインに照らして
> 過不足を検証**し、①②③ のどれが優れるかを**事実ベースで比較検討**する。
>
> ⚠️ 外部AIは nuko のエンジン内部（akaza = 単語 n-gram LM）を知らずに一般論で答えている。
> **「どのデータが architecture に乗るか」は実装を見て判断**する（推測禁止・グローバル規約）。

---

## 1. Gemini(Carbon) の回答（一次入力・原文保存）

結論: **Mozc の C++ エンジン本体を統合するのは非推奨だが、Mozc の『辞書・接続コストデータ』だけを
Rust 側（libakaza）に統合するのは強く推奨**。

### 選択肢の比較

| 方式 | 概要 | 変換精度(初期) | 開発・保守コスト | nuko 独自機能の維持 | 判定 |
|---|---|---|---|---|---|
| **① データのみ統合**（辞書・コスト借用） | Mozc の TSV 辞書と品詞接続表をビルド時に変換し libakaza で読む | ◎ 大幅改善 | 低（Pure Rust 維持） | ◎ 完全維持 | ★最推奨 |
| **② エンジンごと統合**（C++ FFI/IPC） | mozc_server や変換コアを裏で呼び出し UI だけ nuko | ◎ 商用級 | 極高（Bazel地獄・OS移植難） | × ほぼ捨てる | 非推奨 |
| **③ 完全自前維持**（現状延長） | 自前コーパス学習のみで libakaza を改善し続ける | △ 限界あり | 中（コーパス収集沼） | ◎ 完全維持 | 茨の道 |

### ② を薦めない理由
1. nuko のコア価値（4層学習・王冠モデル・explain）が死ぬ。Mozc は巨大ブラックボックス
   （ラティス〜リランク〜学習が C++ 内で密結合）でスコアに綺麗に介入できず「ガワだけ Mozc」になる。
2. ビルド/マルチプラットフォームの悪夢（Bazel / Abseil / Protobuf 依存）。Pure Rust のクロス設計が汚れる。

### ① が最強の現実解である理由
- Mozc の強さの 8 割は C++ でなく「辞書と接続コストの数字」。`src/data/dictionary_oss/` に **BSD-3-Clause**
  で公開：
  - `dictionary00.txt`〜`dictionary09.txt`: 約100万語の語彙＋大規模 Web コーパス由来の精緻な**単語コスト**
  - `connection_single_column.txt`: 約2,600×2,600 の**品詞 ID 間 接続コスト行列**
- これを入れると「ロングテールの語彙穴・頻度歪みが一瞬で埋まり Google 日本語入力(OSS)水準に近づく」。
  「エンジンコードは一切変えず、ビルドスクリプトで変換して食わせるだけ」。akaza や AzooKey も
  Mozc OSS 辞書 / SudachiDict を変換してベース辞書に使う先例がある。

### Gemini の逆質問
> nuko がベースにしている静的辞書と品詞体系は IPAdic / UniDic / Sudachi / Mozc のどれ？

---

## 2. nuko-ime の実態（コード・パイプラインで確認＝事実）

Gemini の逆質問への回答と、① の feasibility を決める前提。

### 2.1 使っているデータ（`model-pipeline/Makefile` 実測）
- **基礎統計（単語コストの源）**: `akaza-corpus-stats`（**Wikipedia + 青空文庫 + CC-100 + IPADIC** から
  生成した wordcnt trie `stats-vibrato-unigram/bigram/skip-bigram.wordcnt.trie` + `vibrato-ipadic.vocab`）。
- **語彙**: **UniDic 3.1.1 CSJ**（BSD-3、NINJAL）を主 lex、**SudachiDict**（Apache-2.0）で固有名詞・
  漢字普通名詞を補完（`scripts/sudachi_to_corpus.py` で `--corpus` 追加）。
- **辞書生成**: `akaza-data make-dict --corpus … --unidic … --vocab … --sudachi-lex …` で `SKK-JISYO.akaza`。
- **静的辞書**: `nuko-core/src/dictionary/system.rs` は**ハードコード約230語**（即効 fix 用）。

→ **答え: IPADIC(コーパス統計) + UniDic 3.1.1 + SudachiDict の混成。品詞体系は IPADIC/UniDic 由来。**

### 2.2 エンジンの型（= ① の可否を決める決定的事実）
- 実機モデルは **`unigram.model` / `bigram.model` / `skip_bigram.model`（+ `.scores`）= 単語 n-gram LM のみ**。
  **品詞接続コスト行列は存在しない。** akaza は Mozc と違い **POS ラティス方式ではなく単語 bigram 方式**。
- 単語コストは **make-dict の入力辞書のコスト列ではなく、`corpus-stats` の unigram trie（出現回数）から
  再計算**される。`--corpus` は**語彙（surface/yomi）追加**の口であって、コスト数値の注入口ではない。

---

## 3. Gemini 回答の過不足（実装照合）

### ✅ 妥当な点
- **② 却下は完全同意**。4層学習・王冠・explain・将来の AI dreaming という nuko のコア価値を殺し、
  Bazel/C++ でクロス設計も壊す。入れる意味がない。
- **方向性 ① > ③ も同意**。自前コーパスだけの改善は漸減（③「茨の道」）。外部の良質語彙を借りるのは筋。
- **ライセンス**: Mozc OSS 辞書は BSD-3 で MIT/Apache と両立。既に UniDic(BSD-3)/Sudachi(Apache) を
  同様に取り込んでいるので**取り込み機構は既にある**（`sudachi_to_corpus.py` と同型で Mozc も追加可能）。

### ⚠️ 過大・不正確な点（nuko 内部を知らないゆえ）
1. **「接続コスト行列(`connection_single_column.txt`)を入れる」は nuko に非適用。** akaza に POS 接続
   ラティスが無いため、Mozc の 2600×2600 品詞接続表を食わせる口が無い。使うにはエンジンを
   POS ラティス方式へ作り替える必要があり、それは実質 ② に近い大改造。→ **この部分は却下 or 別研究**。
2. **「単語コストを食わせるだけ」は drop-in でない。** akaza のコストは `corpus-stats` の unigram trie
   から来る。Mozc の精緻なコスト数値を効かせるには、`--corpus` 追加（語彙）だけでは不十分で、
   **Mozc の頻度を corpus-stats の unigram 回数へ合流させる or スコア線形補間の層を足す**工程が要る。
   「ビルドスクリプトで変換して食わせるだけ」という軽さではない。
3. **純増分は未知（過大評価リスク）。** nuko は既に UniDic + Sudachi + IPADIC コーパスを使用済み。
   Mozc OSS 辞書は**語彙がかなり重複**する。「一瞬で Google 水準」は楽観で、**実測なしに効果は不明**。

### ➕ 不足していた観点（こちらで補う）
- **測定が先（C）**。「数千件のロングテール」を相手に、ベースの Top-1 がいま何%で、Mozc 語彙追加で
  何%上がるかを測る土台（Gemini の C 軸 = bench-eval）が無いと、① は**勘で大工事**になる。
- **①は 2 つに分解すべき**: (a)**語彙ブレンド**（Mozc OSS 辞書を `--corpus` 追加、容易・低リスク）と
  (b)**コスト/頻度補間**（corpus-stats への合流 or スコア補間、要工夫）。(a) だけでも「辞書に無い語で
  junk 分節になる」系は救えるので、まず (a) を bench で評価する。

---

## 4. 比較検討の結論（どれが優れているか）

**総合順位: ①(語彙ブレンド) > ③ > ①(コスト/接続) ≫ ②**

| 方式 | nuko での現実的評価 |
|---|---|
| **② エンジン統合** | **却下**。コア価値喪失＋Bazel/C++ でクロス設計崩壊。Gemini と同意。 |
| **① 接続コスト行列** | **非適用**。akaza に POS ラティスが無い。やるなら ② 相当の大改造＝却下寄り。 |
| **① 単語コスト補間** | **中期・要工夫**。corpus-stats への頻度合流 or スコア線形補間の層が要る。効果は bench 次第。 |
| **① 語彙ブレンド** | **最も有望・低リスク**。Mozc OSS 辞書(BSD-3)を既存 `--corpus` 機構で追加するだけ。語彙穴を埋める。 |
| **③ 自前維持** | **現状のベースライン**。漸減だが curation(A)＋学習で体感は上げられる。 |

**判断: 「エンジンは libakaza を維持（②却下）」は Gemini と一致。ただし ① は『辞書データを食わせるだけで
一瞬』ではなく、nuko では (a)語彙ブレンド＝易 / (b)コスト補間＝要工夫 / (c)接続行列＝非適用 に割れる。**
**よって順序は「まず C(bench) で現状 Top-1 と語彙穴を定量化 → ①(a) 語彙ブレンドを 1 本試して差分測定 →
効果があれば (b) コスト補間」。A(curation)＋C(bench) を先に、B は測ってから。**

→ 「どちらが優れているか」の答え: **方針としては ①（データ統合、特に語彙ブレンド）が ②③ より優れる。
ただし効果は未測定で、C（ベンチ）無しに大工事するのは避ける。** Gemini の結論（エンジンは手放さず
データだけ借りる）は正しいが、「接続コスト行列」「コストを食わせるだけ」の 2 点は nuko では過大。

---

## 5. 具体的な次手（本レポートの提言）

1. **C: bench-eval を先に作る**（別 PR、進行中）。reading→expected の Golden Set で Top-1/Top-3/MRR と
   **Lexical Purity@5**（上位5に実在語が占める割合＝curation の効果が測れる独自指標）を出す。
2. **①(a) 語彙ブレンドの spike**（C の後）: Mozc OSS `dictionaryNN.txt`(BSD-3) を `sudachi_to_corpus.py`
   と同型で `--corpus` 追加 → `make all PROFILE=min` → bench で Top-1 差分を測る。重複が多ければ中止、
   語彙穴が埋まるなら PROFILE=full で採用。
3. **①(b) コスト補間は (a) の結果を見てから**。corpus-stats 合流 or `engine.rs` でのスコア線形補間
   （ローカル・explain 可視化を保ったまま）を検討。
4. **接続コスト行列(①の一部)は見送り**（akaza アーキが POS ラティスでないため）。

---

（ソース: Gemini/Carbon 回答全文 §1、model-pipeline/Makefile（corpus-stats=Wikipedia+青空+CC-100+IPADIC /
UniDic 3.1.1 / SudachiDict / make-dict --corpus）、akaza-model の 6 ファイル（単語 n-gram のみ・POS 接続無し）、
nuko-core/src/dictionary/system.rs（静的辞書）、CLAUDE.md「どの層で直すか」）
