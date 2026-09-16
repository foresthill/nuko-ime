# リリース手順 (macOS プレビュー)

nuko-ime の macOS 向けプレビュー版を GitHub Release で配布する手順。

> **現状の位置づけ (正直に)**
> - **macOS 単独**。Windows(TSF)/Linux(IBus/Fcitx5)は `nuko-platform` に抽象層はあるが未実装。ROADMAP に残す。
> - **未署名・未 notarization**。Gatekeeper の警告が出る。導入に手作業が要る(下記)。
> - **学習は頻度学習(ローカル保存)のみ**。AI/BYOK 学習は未実装(将来)。
> - バージョンは **0.1.0**(1.0 は notarization + 実利用実績が揃ってから)。

## 1. 成果物を作る

```bash
FEATURES=akaza ./nuko-macos/scripts/package-release.sh
```

`dist/` に以下ができる:

| ファイル | 中身 | 目安 |
|---|---|---|
| `NukoIME-v<ver>-macos.zip` | .app 本体 | ~1 MB |
| `nuko-ime-model-v<ver>.tar.gz` | libakaza モデル6ファイル + NOTICE | ~158 MB |
| `SHA256SUMS.txt` | チェックサム | — |

**モデルのライセンス**: 生成モデルは CC BY-SA 4.0(Wikipedia)+ Public Domain(青空文庫)+ CC-100 の派生物。**再配布には NOTICE の同梱が必須**(CC BY-SA の帰属要件)。パッケージスクリプトが `model-pipeline/NOTICE` を tar に自動同梱する。

## 2. GitHub Release を作る (公開 = ユーザー確認後)

```bash
gh release create v0.1.0 \
  --title "ぬこIME v0.1.0 (macOS プレビュー)" \
  --notes-file docs/RELEASE_NOTES_v0.1.0.md \
  --prerelease \
  dist/NukoIME-v0.1.0-macos.zip \
  dist/nuko-ime-model-v0.1.0.tar.gz \
  dist/SHA256SUMS.txt
```

- `--prerelease` を付ける(プレビューであることを明示)。
- 公開は不可逆的にユーザーへ配布する操作なので、**中身を確認してから実行する**。

## 3. リリースノート (雛形)

`docs/RELEASE_NOTES_v0.1.0.md` を Release 本文に使う。導入手順は README「プレビュー版」節と揃える。

## 4. 将来 (B: 正式リリース)

- Apple Developer ID で **署名 + notarization**(Gatekeeper 警告を消す)。
  - 事前に Apple Developer Program License Agreement の承認が必要。
- DMG 化 / Homebrew cask 等のパッケージマネージャ対応。
- モデルの初回自動 DL(ROADMAP Phase 2-G)で 158MB 手動配置を不要にする。
