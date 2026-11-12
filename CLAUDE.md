# CLAUDE.md — nuko-ime プロジェクト開発ノート

> AI 開発エージェント (Claude 等) と人間の開発者が、コードを読むだけでは分からない
> プロジェクト固有の決定事項・落とし穴・運用手順を共有するためのドキュメント。
>
> 2026-05〜06 の集中開発で得た知見の集約。コードの WHAT は読めば分かる、ここには WHY と HOW を残す。

## プロジェクト概要

- macOS 向け日本語 IME (将来 Linux / Windows 対応予定)
- 変換エンジン: **libakaza** (Path B、MIT) + 静的辞書 + 個人頻度学習
- ライセンス: Apache-2.0 OR MIT
- 詳しくは [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) / [`docs/ROADMAP.md`](docs/ROADMAP.md)

## ビルド・テスト・デプロイの 4 つのコマンド

開発中に最も頻繁に走らせるもの:

```bash
# 1. ユニットテスト + lint (毎コミット前)
#    ★ clippy は必ず CI と同じ条件 (--workspace --all-targets) で回すこと。
#      --all-targets はテストコードも lint 対象にする。省くとローカルで通って CI で落ちる
#      (2026-09-19、observation テストの clippy でこれをやらかした)。
cargo test --workspace --features akaza                                     # CI: Test と同等
cargo clippy --workspace --all-targets -- -D warnings                       # CI: clippy-macos と同じ
cargo clippy --workspace --exclude nuko-macos --all-targets -- -D warnings  # CI: clippy (ubuntu) と同じ
cargo clippy --workspace --all-targets --features akaza -- -D warnings      # akaza gated も確認
cargo fmt --check

# 2. .app をビルドしてインストール (Rust コード変更時)
FEATURES=akaza ./nuko-macos/scripts/install.sh

# 3. libakaza モデルを再生成 (model-pipeline/ 変更時)
cd model-pipeline && make all PROFILE=min   # 数分
cd model-pipeline && make all PROFILE=full  # 数十分〜数時間 (epochs 10000)

# 4. 生成したモデルを反映 (= ファイルコピー + プロセス再起動)
MODEL_DIR="$HOME/Library/Application Support/nuko-ime/akaza-model"
cp -v model-pipeline/data/SKK-JISYO.akaza \
      model-pipeline/data/unigram.model \
      model-pipeline/data/bigram.model \
      model-pipeline/data/bigram.model.scores \
      model-pipeline/data/skip_bigram.model \
      model-pipeline/data/skip_bigram.model.scores \
      "$MODEL_DIR/"
killall NukoIME
```

### 2 種類の「更新」を混同しないこと (重要)

| 何を変えたか | 反映方法 |
|------------|---------|
| **Rust コード** (`nuko-core` / `nuko-macos` の `*.rs`) | `install.sh` で `.app` 再ビルド |
| **libakaza モデル** (`*.model` / `*.scores` / `SKK-JISYO.akaza`) | `cp` で配置 + `killall NukoIME` |

両者は別物。「静的辞書 (`system.rs`) を変えたのに反映されない」と思ったら、それは Rust コードなので **`install.sh` が要る**。逆に libakaza モデル更新時に `install.sh` を走らせる必要はない。

`*.scores` ファイル (`bigram.model.scores` / `skip_bigram.model.scores`) も**必須 6 ファイル**。漏らすと libakaza が silent fail して静的辞書フォールバックになる (= 「ガリガリした候補しか出ない」症状)。

## 実機 smoke test (`controller.rs` を触ったら**必須**)

**ユニットテストも CI も実機の IMK 経由入力を一切検証しない。** `commit.rs` の純粋関数テストが全部緑でも「打てるか」は保証されない。IMK の FFI 境界・方式違反 (落とし穴 #9) のバグは緑のまま main に潜伏し、実機投入で初めて発覚する (2026-06-20 の #51/#52 大デグレが実例)。

**ルール: `nuko-macos/src/controller.rs` (= IMK 境界) を変更した PR は、マージ前に実機 smoke test を必須とする。**

ヘルパースクリプトで build → install → 再起動まで自動化し、チェックリストを表示する:

```bash
./nuko-macos/scripts/smoke-test.sh        # release ビルドで install + 再起動 + チェックリスト
./nuko-macos/scripts/smoke-test.sh --debug # debug ビルド (panic=unwind+シンボル) で /tmp にログ採取
```

手で打って確認する最低項目 (スクリプトが表示):

0. **★ まず入力ソースを「ことえり等に切替 → NukoIME に戻す」。** 再インストール直後は、
   既に起動中のアプリが**古い IME 接続を掴んだまま**で新ビルドが反映されない。`killall`+`open`
   だけでは不十分で、入力ソースを切替→戻すと macOS が新バイナリに繋ぎ直す
   (2026-06-25 に「修正したのに反映されない」で 2 回ハマった)。
1. 「にほんご」打鍵 → **ローマ字がかなに変換される** (打てる)
2. Space → 変換、Enter → 確定、Backspace → 1 文字削除
3. 数キー打鍵しても **プロセスが crash しない** (`pgrep NukoIME` が生きている)
4. (segmented 変換時) ←→ で文節移動 / **Shift+←→ で文節の右端を伸縮** (右端だけ動く、下記仕様)
5. 候補が多い変換で**パネルが 1 ページ (9 候補) に収まり**、ページ送りできる

`--debug` モードは謎 crash の調査用。`panic=abort`+`strip` のリリースと違い、objc2 の `panic_verify` と `RUST_BACKTRACE=full` が panic 箇所を `file:line` で吐く。crash 後 `/tmp/nuko_smoke_debug.log` を読む。

## 確定した技術的落とし穴 (一次ソース確認済)

### 1. IMKCandidates は使えない

`IMKCandidates` (Apple 純正の IMK 候補ウィンドウ) は実機で動作不安定。Shiki Suen (vChewing 作者) が ["ancient rubbish, still stinking to this day"](https://shikisuen.medium.com/macos-input-method-development-guidelines-for-2026-5123461fa53b) と評価する程度の framework バグを抱える。

**採用したアプローチ**: 自前の `NSPanel` + `NSTextField` 描画 (= `CustomCandidatePanel`、`nuko-macos/src/candidate_panel.rs`)。

経緯詳細: PR #29 (per-controller IMKCandidates) → revert PR #30 → PR #31 (singleton IMKCandidates) → PR #32 (sync_panel_selection) → PR #33 (自前 NSPanel に移行) → PR #34 / #36 / #37 / #38 (NSPanel が出ない問題の段階的修正)。

### 2. NSPanel に必要な必須フラグ

`Borderless` だけで NSPanel を生成すると **IME プロセスは non-active なので表示されない**。必須:

```rust
let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;  // ★ NonactivatingPanel が必須
panel.setFloatingPanel(true);
panel.setBecomesKeyOnlyIfNeeded(true);
panel.setLevel(NSPopUpMenuWindowLevel);
panel.setHidesOnDeactivate(false);
panel.orderFrontRegardless();  // ★ orderFront ではダメ
```

### 3. `firstRectForCharacterRange:` は当てにならない

カーソル位置を取るための API だが、クライアントによって `(0,0,0,0)` を返す (= 「該当無し」のシグナルらしい)。`msg_send!` 経由の NSRect (32B struct) ABI 不整合の可能性もあり要再調査。

**暫定アプローチ**: マウスカーソル位置 (`NSEvent::mouseLocation()`) を使って、そのスクリーン内にパネルを置く。`controller.rs::caret_screen_point` 参照。マルチスクリーン対応のため全 NSScreen 走査でマウスのある screen を特定。

### 4. `IMKInputController` セッションが複数生成され得る

ブラウザのタブ切替・別アプリへのフォーカス移動などで `initWithServer:` が複数回呼ばれる。状態を per-controller ivars で持つと session 間で消失するため、`ConversionEngine` や `CustomCandidatePanel` は **thread_local singleton** で持つ。詳細は `state.rs` の方針コメント。

### 5. libakaza `convert` vs `convert_k_best`

旧コード (PR #28 ~ #39) は `convert()` で 1 候補 (最良パス連結) のみ返していた → 複数文節入力で代替が出ない。

PR #40 で `convert_k_best(reading, None, k=9)` に切替。各 `KBestPath` が「文全体の代替読み解き」を表し、「わたしのなまえ」→ 「私の名前 / 渡しの名前 / ワタシのなまえ」のような並びが手に入る。

### 6. `make-dict` の SudachiDict フィルタ

`akaza-data` の `make_sudachi_dict` は **名詞-固有名詞** OR **名詞-普通名詞-全カタカナ** しか拾わない。「既読 / 既得 / 危篤」のような **漢字の普通名詞** は dict に入らない。

**回避策** (PR #42): SudachiDict CSV から漢字普通名詞を抽出 → `surface/yomi` 形式の corpus にして make-dict の `--corpus` で渡す。`make_corpus_dict` は filter なしで全 word を追加する仕様を利用。

スクリプト: `model-pipeline/scripts/sudachi_to_corpus.py`。

### 7. 数字 `0-9` は IME 変換対象外にする

旧コードは ASCII graphic char を全部 romaji buffer に放り込んでいた → 「1tu」入力で buffer="1tu" → flush で composition に「1tu」注入 → engine.convert に「1tu」を渡して「統治体のに」のような変な変換になっていた (PR #35 で修正)。

**現在の挙動**: 候補表示なし & 数字 1 文字なら、composition があれば commit + 数字を直接 insertText でパススルー。

### 8. 候補リスト dedup は全パスで必要

`ConversionEngine::convert` には:
1. 学習データ
2. libakaza
3. 静的辞書
4. かなそのまま
5. カタカナ
6. 半角カタカナ

の 6 パスがあり、surface 一致 dedup を **全パスで**しないと「ニホンゴ」×3 のような重複が並ぶ (PR #39)。

### 9. IMK のイベント受信は 3 方式の排他 — `handleEvent:client:` を方式 1 に被せない

`IMKServerInput` のイベント受信には **3 方式**あり、入力メソッドは**そのうち 1 つだけ**を選んで実装する設計 (Apple 一次ソース: `IMKInputController.h` / [objc2-input-method-kit の doc コメント](https://docs.rs/objc2-input-method-kit) — *"This is not a formal protocol by choice. … An input method should choose one of those ways and implement the appropriate methods."*):

1. `inputText:client:` + `didCommandBySelector:client:` (キーバインド方式) ← **nuko はこれ**
2. `inputText:key:modifiers:client:`
3. `handleEvent:client:` (生 NSEvent を直接受信)

**方式を混ぜると壊れる:**

- 方式 1 を実装済みのところに方式 3 (`handleEvent:client:`) を**追加で被せる**と、IMK は `inputText:` を呼ばなくなり **日本語が全く打てなくなる** (PR #51 の大デグレ)。
- さらに `IMKInputController` は `handleEvent:client:` の**実装を持たない** (ヘッダに宣言はあるが実体は IMKServer 側)。なので「処理しないキーを super に流す」つもりで `msg_send![super(self), handleEvent:client:]` を呼ぶと **`invalid message send … method not found` で abort (crash)** する (PR #52)。

経緯: PR #51 (方式 3 を被せる) → PR #52 (super 呼出で直そうとして crash) → **PR #60 で `handleEvent:client:` override を完全撤去し方式 1 単独へ戻して根治** (2026-06-20、実機初投入で発覚)。矢印キーは方式 1 の `didCommandBySelector:` (`moveLeft:`/`moveRight:`) で受ける。

**デバッグのコツ**: リリースは `panic=abort` + `strip` で crash 箇所が見えない。`msg_send!` の型/セレクタ不整合は **デバッグビルド (`cargo build`、`panic=unwind` + シンボル付き) の objc2 `panic_verify`** が炙り出す。リリースで謎 crash したら、debug バイナリを `.app` に差し込んで `RUST_BACKTRACE=full ./NukoIME 2>log` で再現するのが最短 (詳細は下記「実機 smoke test」)。

### 10. `setMarkedText:` の `replacementRange` は Chromium/Electron で効く効かないが分かれる

未確定 (marked text) の更新は `setMarkedText:selectionRange:replacementRange:` で行うが、`replacementRange` の渡し方で **ネイティブ vs Chromium/Electron (Claude Code / VS Code / Slack 等)** の挙動が割れる (2026-09、実機ログで確認)。

- **`replacementRange = {NSNotFound, 0}`** (「現在のマーク範囲を置換」): ネイティブ (メモ/テキストエディット等) は正しく更新する。だが **Chromium 系は縮小更新 (Backspace で marked が短くなる) を画面に反映しない**ことがあり、「Backspace で消えない」症状になる。
  - 対策: **クライアントの `markedRange` を取得して明示的な `replacementRange` として渡す** (取得不能なら `NSNotFound` に fallback)。ネイティブでは両者同義なので回帰しない。`set_marked_text_with_selection` 参照。
- **未確定が空になる瞬間** (最後の 1 文字を Backspace で消す等) に `insertText("")` で終了すると、一部クライアントがマーク末尾を消さず **「最後の 1 文字だけ消えない (2 回押しが要る)」** になる。
  - 対策: 空になるケースは `insertText("")` でなく **`setMarkedText("")`** (実マーク範囲を空に置換) で確実にクリアする。`do_backspace` の `*EndComposing` 系参照。

**切り分け方**: `deleteBackward:` は Claude Code でも IME に届く (ログで確認済み)。nuko の `do_backspace` は composition を正しく削っている。問題は marked text の**反映側**。「Electron のせい」と断じる前に、ネイティブと Chromium 系の**両方で実機確認**し、`setMarkedText` の引数を詰めること。

> 注: 落とし穴 #7「数字 0-9 は変換対象外」は **PR #92/#94 で方針変更済み**。現在は数字も記号と同じ統一処理で、入力中は composition に足し (Space で全角/漢数字に変換)、先頭は composition に入れて未確定にする (即挿入をやめた)。「数字キーで候補選択 (digit-select)」は廃止。

## 文節伸縮の仕様 (Shift+←→) — **右端だけを動かす**

**確定した重要仕様 (ユーザー要望 2026-06-25)。変更するときは必ずこの方針を守ること。**

文節境界の手動調整 (`Shift+←` / `Shift+→`) は、**常に「focused 文節の右端だけ」を動かす**。左端は決して動かさない。

| キー | 操作 | 挙動 |
|------|------|------|
| **Shift+→** | focused の右端を 1 文字 **右** へ | focused を**伸ばす**。右隣から 1 文字もらう |
| **Shift+←** | focused の右端を 1 文字 **左** へ | focused を**縮める**。末尾 1 文字を右隣に渡す (末尾文節なら新文節を作る) |

これは Google 日本語入力等の標準。**「いま自分がどこの区切りを調整しているか」が直感的**になる。

**やってはいけないこと**: focused が 2 番目以降のとき `Shift+←` で**左端**を動かす (左隣を縮めて自分を左に伸ばす) のは禁止。「左が増えるとどこを調整しているか分からなくなって混乱する」とユーザーから明確に指摘されている。akaza 由来の `extend_clause.rs` の旧実装はこれをやっていたので、`extend_left` を「右端のみ動かす」に作り替えた。

実装: [`nuko-core/src/conversion/extend_clause.rs`](nuko-core/src/conversion/extend_clause.rs) の `extend_right` / `extend_left` (純粋関数、テスト付き) → `ConversionEngine::resize_segment` → controller の `handle_segment_resize`。

## 静的辞書とモデル辞書の補完関係

`nuko-core/src/dictionary/system.rs` は **ハードコードの静的辞書** (現在約 230 エントリ)。libakaza モデルの dict (SKK-JISYO.akaza、~1M エントリ) が大規模だが、まだ拾いきれない日常頻出語が出てくる。

- 即効性 fix: 静的辞書に追記して PR (例: PR #41, #43)
- 抜本対応: model-pipeline で再訓練 (PR #42 の corpus 拡張 + Phase 2-E)
- 個人最適: BYOK + persona + dreaming (将来、計画は [`memory`](#) 参照)

### ⚠️ エッジケースは「個別追記」でなく「クラスをどの層で直すか」で考える

誤変換を見たら、**この1件を静的辞書に足す**のではなく、**どの種類か**を判定して適切な層で直す。
個別ハードコードは無限に増えて破綻する（グローバル規約「個別対応は破綻する」）。

- **規則クラス**（ローマ字 nn、助数詞の音便、数の読み…）→ **ルールエンジン**で生成（`dictionary/counter.rs` / `input/romaji.rs`）。規則は数十でもインスタンスは数千、規則を直せば一網打尽
- **一般品質**（同音異義のランキング）→ **モデル**（`model-pipeline` の訓練コーパス）
- **個人・固有**（名前・専門語・区切り）→ **学習**（correction / segmentation / 複合語）
- **ロングテール** → **AI dreaming**

全体方針の表は [`docs/ARCHITECTURE.md` §3.4](docs/ARCHITECTURE.md)。規則/生成器を変えるときは
**壊してはいけない既存ケースを回帰テストで固定してから**直す。

### 落とし穴: nn 曖昧性の「代替読み候補」は学習キーを入力読みにする (#159 / PR #160)

`しんに⇄しんい(真意)` / `たんに⇄たんい(単位)` は **全IME共通で `shinni→しんに` が正しいローマ字**（ソース: シャープ ローマ字ガイド / Mozc `romanji-hiragana.tsv`。romaji 層では解決不可、`おんな=onna` と同型）。nuko は `nn_alternate_readings` で `しんに→しんい` の**代替読みも変換してマージ**し、真意 を候補に出す（＝Google/ATOK が変換層でやる曖昧一致と同方式）。

- **罠**: 代替読み由来候補の `candidate.reading` を代替読み（しんい）のままにすると、ユーザーが `しんに` と打って真意を選んでも **Layer0 頻度学習が しんい でキーされ**、次回 `しんに` で引けず「選んでも一生効かない」。観察ログ(Layer1/2)はオプトインなので未有効ユーザーでは訂正経路も働かない。
- **ルール**: 代替読み候補は `candidate.reading` を**入力(文節)読みに付け替える**（`candidate.reading` は表示に使われず学習キー専用。controller は `seg.reading` を使う）。`engine.rs` の `convert()` / `merge_nn_alternates_into_segments()` 参照。これで一度選べば Layer0 (score 200_010 > libakaza 100_000) で次回先頭。
- 初回ランキング（真意 を初手で上位）は未対応の follow-up（#159）。

### 落とし穴: 学習は flat / segmented の **両経路** に配線する (#178 / PR #178)

変換経路は 2 つある。**両方に同じ個人重みを効かせること。** 片方だけだと「単体では学習が効くのに、文中の語では永遠に勝てない」という再現しにくいバグになる。

- `convert()`（flat・単文節/静的辞書）は Layer0 頻度＋Layer2 訂正を合成する。
- `convert_segmented()`（libakaza の複数文節＝**文の大半**）は当初 **Layer0 頻度を未適用**で、Layer2（観察ログ **opt-in** 前提）しか効かせていなかった。→ 観察ログ未有効ユーザーでは文中の語に学習が一つも効かず、`ほう→法`・`よう→用` 等 libakaza 既定が永遠に 1 位に戻った（2026-11「砲→方 / 用→様 が何度直しても勝てない。勝って欲しい」）。
- **根治**: `engine.rs` に `apply_segment_frequency(segmented, learning)` を追加し `convert_segmented` の nn マージ後に適用（読み完全一致のみ・接尾/分割流用なし＝「学習 over-generalization」PR #106 の教訓）。opt-in 不要の常時 ON なので「一度選び直せば次回勝つ」が成立。
- **検証は `--release`**（debug は rsmarisa 再帰で try_new パニック）。一時 HOME に `learning.json` を仕込み、本番学習を汚さず `nuko convert --libakaza --personal` で before/after を実測した（ほうがいい 法→方 / このよう 用→様）。
- 残件: 間違った表層も確定済みだと Layer0 が重み投票になり tie で負け得る。「打ち直し＝間違い降格」の決定的機構は follow-up。思想は [`docs/LEARNING_ARCHITECTURE.md` の「個人重みの合成」](docs/LEARNING_ARCHITECTURE.md) を正とする。

### 落とし穴: segmented の Layer0 反映は「加算」でなく **`max(基礎, Layer0)`** (#184 / PR #184)

`apply_segment_frequency` が Layer0 頻度を候補へ効かせるとき、**既存候補も注入候補も「純 Layer0」に揃える**こと（`c.score = c.score.max(lc.score)`）。加算は禁止。

- **罠**: libakaza の文節候補スコアは**ログ確率ベースで多くは負値**。既存候補に `基礎 + Layer0` と加算すると、負の基礎に沈んだ高 count 表層が、候補に無く**純 Layer0** で注入される低 count 表層に**逆転**される。
- 実測（`--explain`・`--release`・実 `learning.json`、読み `ように`）: BEFORE 加算 → `用に(×4) 200040` が `ように(×36) −1408+200360=198952` に勝って 1位（＝ユーザー報告「文中で 用に になる」）。AFTER `max` → `ように 200360` が 1位、count 順（36>5>4）に整列。
- flat `convert()` は元々「置換」で正しく count 順。壊れていたのは **segmented だけ**。
- **検証手順の定石**: ① 誤変換を見たら `nuko convert <読み> --libakaza --personal --explain`（#183、`--release` 必須）で内訳を撮る → ② 失敗ケースをユニットテスト化（旧 `add` で落ちることを実測）→ ③ `max` 等の修正後に before/after を再度 `--explain` で実測。推測で辞書/スコアを弄らない。
- 残件（③ 降格 / 文脈）: `そこ(×28 底 > ×27 そこ)` は count どおりで `max` では覆らない。降格機構 or `context_hash` が要る。詳細と設計思想は [`docs/LEARNING_ARCHITECTURE.md` §1.6（スコア透明化）/ §個人重みの合成](docs/LEARNING_ARCHITECTURE.md) を正とする。

### 設計思想: 学習4層 ↔ 心の構造（意識/個人的無意識/集合的無意識/超意識）

個人学習は**共有しない**、集合知は**有益な分だけアップデートで配布**、AI(dreaming)は**意図推論**——この振り分けは心の構造のメタファで整理できる。長期の設計思想は [`docs/LEARNING_ARCHITECTURE.md` §1.5](docs/LEARNING_ARCHITECTURE.md)（長期記憶）。このCLAUDE.md（短期記憶）には運用規約だけ置き、思想は docs 側を正とする。

## モデル配布の戦略

- 現状: 各開発者が `make all` でローカル生成
- Phase 2-F (今): 手動配置の手順を README にまとめる
- Phase 2-G (将来): GitHub Releases から自動 DL ([`model-pipeline/README.md`](model-pipeline/README.md))

## 推奨開発フロー

1. ブランチを切る (`feat/...` / `fix/...` / `docs/...`)
2. 変更 → `cargo test` + `cargo clippy -D warnings` + `cargo fmt --check` 緑を確認
3. コミット (Conventional Commits、Co-Authored-By 任意)
4. push + PR 作成 (`gh pr create`)
5. **main 直 push は禁止** ([global CLAUDE.md ルール](https://github.com/foresthill/nuko-ime/blob/main/CLAUDE.md))
6. ユーザーがマージ
7. (Rust 変更なら) `install.sh` で実機反映
8. (モデル変更なら) `cp + killall` で実機反映

## 詳しいメモ

- 設計判断・思想: `docs/ARCHITECTURE.md` / `docs/VISION.md`
- Phase 別の段取り: `docs/ROADMAP.md`
- 将来機能: `docs/FUTURE_FEATURES.md`
- spike (探索的調査): `docs/spikes/`
