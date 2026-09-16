# ぬこIME v0.1.0 (macOS プレビュー)

libakaza ベースの macOS 向け日本語 IME。最初の公開プレビューです。
**開発者・早期に試したい方向け**の未署名ビルドです。

## これは何か

- ローマ字 → かな → 漢字変換ができる macOS 入力メソッド
- 変換エンジン: libakaza + 静的辞書 + **ローカル頻度学習**(使うほど自分の変換に寄る)
- 文節伸縮(Shift+←→)、候補パネル(9候補ページング)対応

## プライバシー方針

- **学習データはすべてローカル保存**(`~/Library/Application Support/nuko-ime/`)。外部送信しない。
- **テレメトリ・利用データ収集は一切なし。しないのが既定**。
- 改善への協力(変換ログ共有等)は将来つけるとしても**完全オプトイン**。同意した人だけ。

## 対応状況(正直に)

- ✅ **macOS のみ**(Apple Silicon / Intel。macOS 13+ を想定)
- ⏳ Windows / Linux は設計上の想定はあるが**未実装**
- ⏳ AI/BYOK 学習は**未実装**(頻度学習のみ)
- ⚠️ **未署名**。Gatekeeper の警告が出るので下記手順で許可が必要

## インストール

**1. アプリを入れる**

1. `NukoIME-v0.1.0-macos.zip` を展開し、`NukoIME.app` を `~/Library/Input Methods/` に置く
2. 未署名のため、Finder で `NukoIME.app` を**右クリック →「開く」**、または
   **システム設定 → プライバシーとセキュリティ**で「このまま開く」を許可
3. ログアウト → ログイン(初回のみ、入力メソッド登録のため)

**2. 変換モデルを入れる**(これが無いと変換品質が大きく落ちます)

```bash
mkdir -p "$HOME/Library/Application Support/nuko-ime"
tar -xzf nuko-ime-model-v0.1.0.tar.gz -C "$HOME/Library/Application Support/nuko-ime/"
# → ~/Library/Application Support/nuko-ime/akaza-model/ に6ファイル + NOTICE が展開される
```

**3. 入力ソースに追加**

- システム設定 → キーボード → 入力ソース → 編集 → 「+」→ 日本語 →「ぬこIME」を追加
- メニューバーの入力メニューから「ぬこIME」を選択

## 使い方(基本)

- ローマ字入力 → Space で変換 → Enter で確定
- `←→` で文節移動、**`Shift+←→` で文節の区切りを伸縮**(右端が動く)
- 候補は数字キー 1-9、Space/↑↓ でページ送り

## 既知の制限

- 未署名のため初回導入に手作業が要る(署名+notarization は次版で対応予定)
- モデルが 158MB と大きい(初回自動 DL は将来対応)
- ローマ字の一部エッジケース: 「範囲」は `han'i`(`hanni` は「はんに」)

## チェックサム

`SHA256SUMS.txt` を参照。

## ライセンス

- コード: Apache-2.0 OR MIT
- 変換モデル: CC BY-SA 4.0(Wikipedia)/ Public Domain(青空文庫)/ CC-100 の派生物。
  同梱の `NOTICE` を参照。
