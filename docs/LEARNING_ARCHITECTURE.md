# 学習アーキテクチャ (かな→漢字の個人最適化)

ぬこIME の「使うほど自分に寄る / 寝てる間に賢くなる」学習機構の**実装できる具体アーキテクチャ**。

> **このドキュメントの位置づけ**
> - **なぜ / 倫理・戦略**: [`AI_AGENT_FOUNDATION.md`](AI_AGENT_FOUNDATION.md)(IME=個人AIエージェント論、諸刃の剣)
> - **フレームワーク(Stage 1-4)**: [`FUTURE_FEATURES.md` §8](FUTURE_FEATURES.md)
> - **本書**: 上記を「データモデル・データフロー・層の責務・ビルド順」に落としたもの
> - 合意した設計方針(2026-09-18、森岡さんと確認): 下から積む / churn-free dreaming / AI の役割は A→B 中心

## 0. 設計原則(不変・破ってはいけない)

1. **ローカル第一・デフォルト収集なし。** 何も集めないのが既定。外部送信は層ごとの明示オプトインのみ。
2. **完全オプトイン(層独立)。** Layer 1 を ON にしても Layer 3(AI)は別途同意が要る。デフォルト全 OFF(Layer 0 を除く)。
3. **透明性。** 学習データは**人間可読**(TOML/JSON)。ユーザーが閲覧・削除・エクスポート・インポートできる。
4. **churn-free dreaming。** dreaming は「提案を生成して人間がレビュー」ではなく、**履歴から個人重みを決定論的に再計算する**だけ。冪等・自動適用・溜まらない。
   → 過去の失敗(自動生成+高頻度human review が破綻)を繰り返さないための絶対条件。
5. **リアルタイム変換は既存の高速ロジック。AI は裏(バッチ)のみ。** 変換のたびに API を叩かない。

## 1. 層構成

```
Layer 0  頻度学習            確定候補の頻度を即時記録 → 変換順位に反映     ✅ 実装済
Layer 1  観察ログ            「入力→emit→(ユーザーが直した)」を記録        ⬜ 未 (次に着手)
Layer 2  訂正学習            観察ログから確定的な訂正/選好を抽出 → 変換へ   ⬜ 未
Layer 3  AI dreaming (BYOK)  Layer 1-2 のデータを AI が最適化(バッチ・オプトイン) ⬜ 未
```

**ボトムアップの理由**: Layer 3 の AI は「材料(Layer 1-2 のデータ)」が無いと何も学習できない。Layer 1-2 は AI 無し・プライバシーリスク無しで**単体で有用**。だから下から作る。

### 各層の責務

| 層 | 入力 | 出力 | データ | オプトイン | AI |
|---|---|---|---|---|---|
| **0 頻度** | 確定候補 | 頻度スコア | `learning.json` | (常時ON、既存) | ✗ |
| **1 観察** | 入力/emit/訂正イベント | 観察ログ | `observations.jsonl` | 要 (デフォOFF) | ✗ |
| **2 訂正学習** | 観察ログ | 訂正ルール/選好重み | `corrections.toml` | Layer 1 と連動 | ✗ |
| **3 AI dreaming** | Layer 1-2 データ | 再計算した重み | Layer 2 データを更新 | **別途要** (デフォOFF) | BYOK |

## 2. データモデル(人間可読・透明)

### Layer 0: 頻度 (既存 `learning.json`)

現状: `HashMap<reading, Vec<FrequencyEntry{ surface, reading, count, ... }>>`。
`LearningManager::record()` が確定時に `increment()`。**context は現在未使用**(`_context`)→ Layer 2 で活用する。

### Layer 1: 観察ログ `observations.jsonl` (追記のみ、1行1イベント)

```jsonc
{"ts":"2026-09-18T14:30:05Z","kind":"commit","reading":"はんい","surface":"範囲","cands":["繁位","範囲"],"picked":1}
{"ts":"2026-09-18T14:31:10Z","kind":"correction","reading":"こんばん","emitted":"今晩","corrected":"こんばん"}
```

- `kind`: `commit`(確定) / `correction`(直後に直した) / `romaji`(誤変換報告など)
- 追記のみ・ローカルのみ。ローテーション(サイズ上限)あり。
- **これが Layer 2/3 の唯一の入力源**。オプトインで初めて書き出す。

### Layer 2: 訂正・選好 `corrections.toml` (人間可読・編集可)

```toml
# 「いつも X を Y に直す」= 確定的な個人ルール (信頼度しきい値を超えたもの)
[[preference]]
reading = "こんばん"
prefer = "こんばん"     # 「今晩」より「こんばん」を上位に
weight = 8
seen = 12               # 観測回数 (透明性)

[[context_bias]]        # 文脈依存の選好 (Layer 0 の context 未使用分を補う)
reading = "きしゃ"
prefer = "記者"
after = "新聞"          # 「新聞」の後なら「記者」
weight = 5
```

- 頻度(Layer 0)だけでは拾えない「文脈」「訂正」を確定ルール化。
- **誤訂正の偶発学習を排除**: 信頼度しきい値(`seen` 回数・一貫性)を超えたものだけ昇格(FUTURE_FEATURES §5.4)。
- ユーザーが直接編集・削除可能(透明性)。

### 個人重みの合成

変換時の候補スコア = `libakaza/辞書スコア` + `Layer0 頻度` + `Layer2 選好/文脈バイアス`。
すべてローカル・決定論的。AI はこの `Layer2` を**再計算する**だけで、変換パスに AI は挟まない。

## 3. データフロー

```mermaid
flowchart TD
  K[打鍵/確定] -->|即時| L0[Layer0 頻度 +1]
  K -->|オプトインON| L1[Layer1 観察ログ 追記]
  subgraph リアルタイム変換 (高速・AI無し)
    C[かな] --> S[候補スコア合成]
    L0 --> S
    L2[(Layer2 訂正/選好)] --> S
    S --> R[候補提示]
  end
  subgraph バッチ (裏・任意)
    L1 --> E[Layer2 抽出: しきい値超えを昇格]
    E --> L2
    L1 -.->|Layer3 ON時のみ| AI[AI dreaming: 再計算]
    L2 -.-> AI
    AI -.->|決定論的に上書き| L2
  end
```

要点: **リアルタイム経路に AI は無い**。AI は「バッチで Layer2 を研ぎ直す」だけ。だから速度・プライバシー・churn-free を同時に満たす。

## 4. Layer 3 — AI dreaming (BYOK) の詳細

### いつ走るか(§8.4 の論点)
- **手動 / 充電中 / 夜間バッチ**から選択。デフォルトは**手動 or 充電中**(バッテリー配慮)。
- 常駐しない。1回のバッチで完結する冪等処理。

### 何を渡し / 何を受け取り / どう適用するか(churn-free の担保)

| | 内容 |
|---|---|
| **入力(AIへ)** | Layer1 観察ログ + Layer2 現行ルール(**ローカルデータのみ**。オプトインで送信先=BYOK プロバイダ) |
| **AIの仕事** | 「訂正パターンの一般化・整理・重み再計算」= 役割 A→B。**新しい提案の生成ではない** |
| **出力(AIから)** | 再計算された Layer2(訂正/選好/重み)**そのもの** |
| **適用** | Layer2 を**丸ごと決定論的に置換**(差分レビュー無し)。前回結果との diff はユーザーが見られる(透明性)が、承認は不要 |

→ **提案キューを作らない**。出力は常に「完全な Layer2」であり、溜まらない。気に入らなければ Layer3 を OFF にすれば Layer2 は Layer1 からの決定論的抽出に戻る。

### BYOK(§8.5)
- プロバイダ抽象: Anthropic / Gemini / OpenAI / ローカル LLM(Ollama/llama.cpp) / **none**。
- 設定: `~/.config/nuko-ime/ai.toml`(プロバイダ・モデル・トリガ)。**API キーは環境変数経由**、本体に保存しない。
- デフォルト OFF。ON にする画面で「何を・どの AI に・いつ渡すか」を明示(AI_AGENT_FOUNDATION §4.5)。

## 5. プライバシー境界

```
[ローカルのみ・常時]        Layer 0 頻度
[ローカルのみ・オプトイン]  Layer 1 観察ログ / Layer 2 訂正学習
[外部送信・別オプトイン]    Layer 3 で BYOK プロバイダに Layer1-2 を渡すとき (ローカル LLM 選択なら外部送信ゼロ)
```

- 何も設定しなければ Layer 0 のみ = 従来通り、収集ゼロ。
- Layer 3 で**ローカル LLM(Ollama)を選べば、AI 学習でも外部送信ゼロ**。これが最もプライバシー厳格な構成。

## 6. ビルド順(bottom-up・各段で単体有用)

| Phase | 内容 | 完了判定 |
|---|---|---|
| **A. Layer 1 (観察ログ)** | オプトイン設定 + commit/correction イベントを `observations.jsonl` に追記 + 閲覧/削除 | ON にすると自分の入力履歴がローカルに貯まり、見られる |
| **B. Layer 2 (訂正学習)** | 観察ログ → しきい値抽出 → `corrections.toml` → 変換スコアに合成。record() の context 活用 | 「いつも直す変換」が次から最初から正しく出る(AI 無しで) |
| **C. Layer 3 (AI dreaming)** | `ai.toml` + プロバイダ抽象 + バッチ再計算 + 適用 + diff 可視化 | AI を設定すると dreaming 後に変換選好が研がれる。OFF で Layer2 決定論に戻る |

**まず Phase A から**。ここで初めて「AI に食わせるデータ」が生まれる。

## 7. 未決事項(実装前に詰める)

- 観察ログのローテーション方針(サイズ/期間上限)
- Layer 2 昇格の信頼度しきい値の具体値(誤訂正排除)
- Layer 3 の diff 可視化 UI の置き場(メニューバー? 設定アプリ?)
- persona/文体プロファイル(memory: BYOK+persona)を Layer 2/3 のどこに載せるか
- macOS 以外への移植時のデータ可搬性

## 参照
- 倫理・戦略: [`AI_AGENT_FOUNDATION.md`](AI_AGENT_FOUNDATION.md)
- フレームワーク: [`FUTURE_FEATURES.md` §8](FUTURE_FEATURES.md)、訂正イベント学習 §5.4
- 現行実装: `nuko-core/src/learning/`(`LearningManager` / `FrequencyEntry`)
