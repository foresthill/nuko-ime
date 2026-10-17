# 方式3 (handleEvent) 移行計画 — F キー対応の大改修

**目的**: F6-F10 の文字種変換（と将来の keyCode/修飾キーを要する機能）を可能にするため、
IMK の入力受信を **方式1 (`inputText:` + `didCommandBySelector:`) → 方式3 (`handleEvent:`)** に
全面移行する。方式3 は生 NSEvent を受け取るので keyCode・修飾キーが取れる。

> 背景: F キーは方式1 では `noop:` として届くが keyCode を伴わず、`NSApp.currentEvent` も
> IME では KeyDown を返さない（2026-10 実機で `keyCode=None` 確認）。識別には生 NSEvent =
> 方式3 が必須。mozc も方式3（`recognizedEvents` = NSKeyDown|NSFlagsChanged）。
> 詳細: memory `nuko-ime-imk-event-exclusive`。

## 重要な制約（なぜ big-bang か）

3 方式は**排他**（混在厳禁 = 落とし穴 #9：方式3 を方式1 に被せると打てない/クラッシュ）。
よって **部分移行・並行テスト不可**。handleEvent を**全キー分実装 → 方式1 を撤去 → 一括テスト**
という big-bang になる。**失敗しても v0.1.3 が安全網**（この作業は専用ブランチ、main は無傷）。

## 幸い: ロジックは既に純粋関数に分離済み

移行で書き直すのは **IMK 配線層だけ**。変換ロジックは移行対象外：
- ローマ字→かな: `nuko-core` `RomajiConverter`
- Space 分岐: `commit::decide_space_action`（純粋）
- コマンド分岐: `commit::decide_command`（純粋、**セレクタ名**キー → keyCode キーに作り替え）
- Backspace: `commit::decide_backspace` 等
- 確定: `commit::decide_commit`（純粋）

## 現行の方式1 ハンドラ（移行元）

| ハンドラ | 役割 | 方式3 での受け口 |
|---|---|---|
| `inputText:client:` → `_input_text_impl` | 文字（ローマ字/記号/数字/Space） | handleEvent 内: `event.characters()` が印字文字 |
| `didCommandBySelector:client:` → `_did_command_impl` | Enter/Esc/Backspace/矢印/Shift+矢印/Tab/noop | handleEvent 内: keyCode + modifiers → 同じ `CommandAction` |
| `setValue:forTag:` | かな/英数 モード切替 | 方式3 でも別途届く（要確認）。かなキー(keyCode 104)も handleEvent で取れる |
| `menu` (`method_id`) | 入力メニュー | **変更不要**（方式に依存しない） |

## 設計

### recognizedEvents
```
#[unsafe(method(recognizedEvents:))]
fn recognized_events(&self, _sender) -> NSEventMask {
    NSEventMask::KeyDown | NSEventMask::FlagsChanged
}
```

### handleEvent:client:
```
#[unsafe(method(handleEvent:client:))]
fn handle_event(&self, event: Option<&NSEvent>, sender) -> Bool {
    // 1. event.type() で分岐
    //    KeyDown: characters() / charactersIgnoringModifiers() / keyCode() / modifierFlags()
    //    FlagsChanged: 修飾キー状態 (必要なら)
    // 2. keyCode + modifiers → KeyAction (純粋関数 command_for_keycode)
    //    - 特殊キー (Return/Delete/Esc/矢印/Shift+矢印/Tab/F6-F10/Space) → コマンド/F キー変換
    //    - それ以外 → characters() を文字として _input_text_impl 相当へ
    // 3. 非 japanese_mode / 非 composing は従来どおりパススルー (Bool::NO)
}
```

### 純粋キーマッピング（テスト可能・de-risk の中心）
`commit::command_for_keycode(key_code: u16, modifiers, is_composing) -> KeyAction`
- macOS 仮想キーコード（kVK_*）: Return=0x24, Delete(BS)=0x33, Escape=0x35, Tab=0x30,
  Space=0x31, Left=0x7B, Right=0x7C, Down=0x7D, Up=0x7E, F6=0x61, F7=0x62, F8=0x64,
  F9=0x65, F10=0x6D。Shift 修飾で矢印→文節伸縮。
- `KeyAction` = `Command(CommandAction)` | `Fkey(FKeyConversion)` | `Char(String)` | `PassThrough`。
- **一次ソースで keyCode を確認**（Carbon `Events.h` kVK_*、または実機 handleEvent でログ）。推測禁止。

## 移行手順（ブランチ `feat/method3-handleevent`）

1. `command_for_keycode` + `KeyAction` を純粋関数で実装＋**rstest 網羅**（各キー・修飾・composing）
2. handleEvent / recognizedEvents を実装し、`_input_text_impl` / `_did_command_impl` の中身を
   KeyAction 経由で呼べるよう小さく refactor（ロジックは再利用）
3. `inputText:client:` / `didCommandBySelector:client:` を**撤去**
4. ビルド → install → **実機 smoke test（全項目）**:
   - ローマ字で「にほんご」が打てる / Space 変換 / Enter 確定 / Esc 取消 / Backspace
   - 矢印で文節移動 / Shift+矢印で文節伸縮 / Tab 単語登録
   - **F7→カタカナ / F8→半角カナ / F6→ひらがな**（本命）
   - かな/英数 切替 / 記号・数字 / crash しない
5. 全部緑なら PR → マージ。1 つでも不可なら原因特定（debug ビルドの panic_verify）して反復。
   直せなければブランチ破棄（v0.1.3 が安全網）。

## リスクと対策
- **最大リスク: 打てなくなる**。→ 専用ブランチ・main 無傷・各 smoke 項目を潰すまでマージしない。
- objc2 の `handleEvent:client:` シグネチャ不整合 → debug ビルド（panic=unwind）の `panic_verify` で特定。
- `characters()` と `charactersIgnoringModifiers()` の使い分け（ローマ字は modifiers 無視側が安全か要検証）。
- 「かな」キー Space leak ガード（現 thread_local）は handleEvent で keyCode 104 を直接見られるので
  むしろ正確になる（方式1 では死んでいた、controller.rs:237 の注記）。

## F キー対応後の副産物
keyCode/修飾キーの完全アクセスで、将来 ATOK/ESET 風のモード切替・任意ショートカットが可能になる。
