# macOS IME 実装調査レポート

**作成日**: 2026-04-24
**目的**: macOS InputMethodKit (IMK) で動作する日本語IMEを実装するにあたり、既存の動作実績のあるOSS IMEを調査し、我々の実装(nuko-macos)に足りないものを特定する。

## 背景: 遭遇した問題

nuko-ime の macOS 版 (`nuko-macos/`) を実装し、以下の段階まで進んだ:

- `.app` バンドル化 → `~/Library/Input Methods/` にインストール成功
- システム設定の入力ソースに「ぬこIME」が登場
- 選択すると `activateServer` が呼ばれる (ログで確認)
- `initWithServer:delegate:client:` のオーバーライドでクラッシュ (SIGABRT) は解決

**しかし**: テキストフィールドで文字キー (a, b, c...) を打っても `inputText:client:` が呼ばれない。スペースキーだけ一度届いた。モード表示 (かな/A/あ) も出ない。

## 調査対象

以下のOSS macOS IME のソースコードを `general-purpose` エージェント経由で調査:

| IME | 言語 | リポジトリ |
|-----|------|-----------|
| macOS_IMKitSample_2021 | Swift | ensan-hcl 他 (最小サンプル) |
| Squirrel | Swift | rime/squirrel (RIME の macOS フロントエンド) |
| KeyMagic-3 | Swift + Rust | thantthet/keymagic-3 (Rust コア + Swift IMK shim) |
| azooKey-Desktop | Swift | ensan-hcl/azooKey-Desktop (モダン日本語IME) |

## 調査結果

### 1. イベント受信メソッドの選択

| IME | 使用メソッド | `recognizedEvents:` | 備考 |
|-----|------|------|-----|
| minimal sample (Latnのみ) | `inputText:client:` | 未定義 | Latnのみなら最小構成で動く |
| Squirrel | `handle:client:` | `.keyDown \| .flagsChanged` | production RIME フロントエンド |
| KeyMagic-3 | `handle:client:` | 未定義 | Rust製、最も近い参考実装 |
| azooKey (日本語) | `handle:client:` | 未定義 | 最もモダンな日本語IME |

**所見**:
- 日本語IMEでは **`handle:client:` が主流** (3/3が採用)
- `handle:client:` は `NSEvent` を直接受け取るため、TSM のフィルタ層をバイパスできる
- `inputText:client:` は Latn のみの IME なら動くが、日本語の場合ルーティングが異なる

### 2. `IMKServerInput` プロトコルの明示的準拠

**どの実装も明示的な protocol 宣言をしていない** (`class X: IMKInputController` のみ)。
Swift では `IMKInputController` 経由で暗黙的に継承されるため、Rust の `define_class!` でも protocol 宣言は不要。

### 3. Info.plist の決定的な差分

#### 我々の初期 Info.plist に存在しなかったキー

```xml
<key>ComponentInputModeDict</key> <!-- ← 最重要 -->
<key>TISIntendedLanguage</key>    <!-- 推奨 -->
<key>InputMethodServerDelegateClass</key>  <!-- オプション -->
```

#### `ComponentInputModeDict` の役割

- **日本語IMEでは事実上必須**
- macOS の TSM (Text Services Manager) に対し「このIMEは複数の入力モード (かな / A) を持つ」と宣言
- 無いと TSM は IME を "headless" として登録 → **印字可能文字を IME にルーティングしない**
- スペースだけ届いていた理由: スペースは別の code path を通るため
- 「かな / A」のメニュー表示もこのキーから生成される

#### azooKey の `ComponentInputModeDict` 構造 (参考)

```xml
<key>ComponentInputModeDict</key>
<dict>
    <key>tsInputModeListKey</key>
    <dict>
        <key>{bundleID}.Japanese</key>
        <dict>
            <key>TISInputSourceID</key>
            <string>{bundleID}.Japanese</string>
            <key>TISIntendedLanguage</key>
            <string>ja</string>
            <key>tsInputModeScriptKey</key>
            <string>smJapanese</string>
            <key>tsInputModeCharacterRepertoireKey</key>
            <array>
                <string>Hira</string><string>Kana</string><string>Latn</string>
            </array>
            <key>tsInputModeDefaultStateKey</key><true/>
            <key>tsInputModeIsVisibleKey</key><true/>
            <!-- icon/keyEquivalent keys -->
        </dict>
        <key>{bundleID}.Roman</key>
        <dict>
            <key>tsInputModeScriptKey</key>
            <string>smRoman</string>
            <!-- ... -->
        </dict>
    </dict>
</dict>
```

### 4. モード表示 (かな/A/あ) の仕組み

- `ComponentInputModeDict` で定義された各サブモードが、入力ソースピッカーに独立した項目として現れる
- `tsInputModeScriptKey` で `smJapanese` (かな) / `smRoman` (A) などを指定
- メニューバーの切替アイコンは `tsInputModeMenuIconFileKey` / `tsInputModePaletteIconFileKey` で指定

### 5. IMKServer 起動コード (KeyMagic-3)

```swift
// main.swift — ほぼ最小
IMKServer(name: kConnectionName, bundleIdentifier: Bundle.main.bundleIdentifier)
NSApplication.shared.run()
```

我々の Rust 実装も同等のことをしているため、**起動コード自体は問題なし**。

## 我々の実装に対する診断

### 優先度順の不足事項

1. **🔴 `ComponentInputModeDict` の欠如** — 最有力の原因
2. **🟡 `TISIntendedLanguage=ja` の欠如** — トップレベル推奨
3. **🟡 `inputText:client:` ではなく `handle:client:` を使うべき** — 日本語IMEの主流
4. **🟢 `InputMethodServerDelegateClass`** — オプション (同じクラスを指定)
5. **🟢 `recognizedEvents:`** — `handle:` 移行後に `.keyDown | .flagsChanged` を返す

## 段階的修正プラン

### Step 1: Info.plist 修正 (実施中)

`ComponentInputModeDict` + `TISIntendedLanguage` を追加し、Japanese / Roman サブモードを定義。

→ これで letter key が `inputText:client:` に届けば純Rust路線継続。

### Step 2: 届かない場合の対処 (A案: Rust継続)

`inputText:client:` を廃止し、`handle:client:` を実装:

```rust
#[unsafe(method(handleEvent:client:))]
fn handle_event(
    &self,
    event: Option<&NSEvent>,
    client: Option<&AnyObject>,
) -> bool {
    // event.characters() / event.keyCode() / event.modifierFlags() を参照
    // 消費したら true、透過なら false
}
```

併せて:

```rust
#[unsafe(method(recognizedEvents:))]
fn recognized_events(&self, _sender: Option<&AnyObject>) -> NSUInteger {
    // NSEventMaskKeyDown | NSEventMaskFlagsChanged
    (1 << 10) | (1 << 12)
}
```

### Step 3: それでもダメな場合 (B案: Swift shim ハイブリッド)

nuko-core を C FFI で公開し、macOS frontend のみ Swift で 50〜100 行書く。
参考: KeyMagic-3 (Rust core + Swift IMK shim の構成が最も近い)。

## TSM キャッシュの扱い

新しい Info.plist を反映させるには、macOS 側のキャッシュを破棄する必要がある:

```sh
# 既存バンドル削除
killall NukoIME 2>/dev/null
rm -rf ~/Library/Input\ Methods/NukoIME.app

# TSM キャッシュフラッシュ
killall -HUP cfprefsd
killall SystemUIServer
/System/Library/Frameworks/Carbon.framework/Frameworks/HIToolbox.framework/Resources/kickstart -u

# 再インストール
bash nuko-macos/scripts/install.sh

# 入力ソースから削除 → 再追加
```

頑固な場合はログアウト→ログインが確実。

## 参考リンク

- macOS_IMKitSample_2021: 最小Latn IMEサンプル
- Squirrel (rime/squirrel): https://github.com/rime/squirrel
- KeyMagic-3 (thantthet/keymagic-3): Rust core + Swift shim の構成参考
- azooKey-Desktop (ensan-hcl/azooKey-Desktop): モダン日本語IMEの最良参考実装

## 結論

**`ComponentInputModeDict` の欠如が文字キー未配信の最有力原因**。
Step 1 で Info.plist を修正し、TSM キャッシュをフラッシュして再検証。
ダメなら Step 2 (`handle:client:` 採用) → それでもダメなら Step 3 (Swift shim) の順で段階的に対応する。
