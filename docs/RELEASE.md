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

## 4. 自動リリース (GitHub Actions, 署名 + 公証)

`.github/workflows/release.yml` が **タグ push (`v*`) で署名 + 公証済みの .app を作って
Release に添付**する。手動の `package-release.sh` と同じ組み立てを使い、Secrets が
設定されていれば `codesign`(Developer ID, hardened runtime) → `notarytool submit --wait`
→ `stapler staple` まで行う。Secrets 未設定なら未署名で通す (CI は失敗しない)。

### 必要な GitHub Secrets

Settings → Secrets and variables → Actions に登録:

| Secret | 中身 |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID Application 証明書 (.p12) の **base64** (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | .p12 書き出し時のパスワード |
| `APPLE_SIGNING_IDENTITY` | 例: `Developer ID Application: Your Name (TEAMID1234)` |
| `APPLE_ID` | Apple アカウントのメール |
| `APPLE_PASSWORD` | **App 用パスワード** (appleid.apple.com → サインインとセキュリティ → App 用パスワード) |
| `APPLE_TEAM_ID` | 10 桁の Team ID |

> 証明書は **Developer ID Application**(Mac 外配布用)。「Apple Development」や
> 「Mac App Distribution」ではない。`security find-identity -v -p codesigning` で確認。

### 流れ

```bash
# バージョンを上げて main へ (別 PR) → タグを打つ
git tag -a v0.1.5 -m "ぬこIME v0.1.5" && git push origin v0.1.5
# → release.yml が起動し、署名+公証済み NukoIME-v0.1.5-macos.zip + SHA256SUMS を
#   pre-release として添付する。
```

- Release 本文は `docs/RELEASE_NOTES_<tag>.md` があればそれを使う。
- **モデル tar (~166MB) は CI では作れない**(生成物でリポジトリに無い)ため、本ワークフローは
  **署名済み .app + SHA のみ**を添付する。モデルは別途手動添付(未変更なら前版を再利用):
  ```bash
  gh release upload v0.1.5 dist/nuko-ime-model-v0.1.5.tar.gz
  ```
- `workflow_dispatch` でも実行可(既存タグを input で指定)。

> **未検証**: 署名配線は入れたが、実際の署名/公証は Secrets 登録後にタグを 1 本流して
> 確認するまで未検証。初回は notarytool のログ(`xcrun notarytool log <id>`)で弾かれ要件
> (entitlements 等)が出たら調整する。arm64 単独ビルド(Intel/universal は将来)。

## 5. 将来 (さらに先)

- DMG 化 / Homebrew cask 等のパッケージマネージャ対応。
- モデルの初回自動 DL(ROADMAP Phase 2-G)で 166MB 手動配置を不要にする。
  - あるいはモデルを専用タグ(例 `model-vN`)の Release に 1 回だけ置き、各版から参照して
    166MB の毎回アップロードを無くす。
- universal binary (arm64 + x86_64) 対応。
