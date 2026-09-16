#!/bin/bash
set -euo pipefail

# ぬこIME macOS プレビュー版のリリース成果物を作る (未署名)。
#
# 生成物 (dist/):
#   - NukoIME-v<version>-macos.zip        … .app 本体 (2.5MB 程度)
#   - nuko-ime-model-v<version>.tar.gz     … libakaza モデル + NOTICE (~300MB)
#   - SHA256SUMS.txt                       … 各成果物のチェックサム
#
# モデルは CC BY-SA 4.0 (Wikipedia) 等の派生物なので、再配布には NOTICE の同梱が
# 必須。本スクリプトは model-pipeline/NOTICE をモデル tar に必ず含める。
#
# 使い方:
#   ./nuko-macos/scripts/package-release.sh
#   MODEL_DIR=/path/to/akaza-model ./nuko-macos/scripts/package-release.sh
#
# 公開 (GitHub Release への添付) はこのスクリプトでは行わない。成果物を dist/ に
# 用意するだけ。公開手順は docs/RELEASE.md 参照。

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APP_NAME="NukoIME"
APP_BUNDLE="$APP_NAME.app"
FEATURES="${FEATURES:-akaza}"
MODEL_DIR="${MODEL_DIR:-$HOME/Library/Application Support/nuko-ime/akaza-model}"
DIST="$PROJECT_ROOT/dist"

# Info.plist から version を取得
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' \
  "$PROJECT_ROOT/nuko-macos/Info.plist" 2>/dev/null || echo "0.0.0")"

echo "=== ぬこIME リリースパッケージ作成 v$VERSION (未署名プレビュー) ==="
rm -rf "$DIST"
mkdir -p "$DIST"

# --- 1. .app をビルドして dist/stage に組み立てる (インストールはしない) ---
echo "[1/4] release ビルド..."
cd "$PROJECT_ROOT"
cargo build --release -p nuko-macos --features "$FEATURES"

echo "[2/4] .app バンドル組み立て..."
STAGE="$DIST/stage"
APP="$STAGE/$APP_BUNDLE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$PROJECT_ROOT/target/release/$APP_NAME" "$APP/Contents/MacOS/"
cp "$PROJECT_ROOT/nuko-macos/Info.plist" "$APP/Contents/"
for icon in icon.tiff icon-japanese.tiff icon-roman.tiff; do
  [ -f "$PROJECT_ROOT/nuko-macos/resources/$icon" ] &&
    cp "$PROJECT_ROOT/nuko-macos/resources/$icon" "$APP/Contents/Resources/"
done
for lang in ja en; do
  if [ -d "$PROJECT_ROOT/nuko-macos/resources/$lang.lproj" ]; then
    mkdir -p "$APP/Contents/Resources/$lang.lproj"
    cp "$PROJECT_ROOT/nuko-macos/resources/$lang.lproj/"*.strings \
      "$APP/Contents/Resources/$lang.lproj/" 2>/dev/null || true
  fi
done

APP_ZIP="$DIST/${APP_NAME}-v${VERSION}-macos.zip"
# ditto はバンドルの属性/権限を保ったまま zip 化する macOS 標準手段
ditto -c -k --sequesterRsrc --keepParent "$APP" "$APP_ZIP"

# --- 3. モデル tar (NOTICE 同梱、CC BY-SA 4.0 帰属要件) ---
echo "[3/4] モデル tar (NOTICE 同梱)..."
if [ -d "$MODEL_DIR" ]; then
  MODEL_STAGE="$DIST/model-stage/akaza-model"
  mkdir -p "$MODEL_STAGE"
  cp "$MODEL_DIR"/* "$MODEL_STAGE/"
  # 再配布ライセンス表示 (必須)
  cp "$PROJECT_ROOT/model-pipeline/NOTICE" "$MODEL_STAGE/NOTICE"
  MODEL_TAR="$DIST/nuko-ime-model-v${VERSION}.tar.gz"
  tar -czf "$MODEL_TAR" -C "$DIST/model-stage" akaza-model
else
  echo "⚠️  モデルが見つかりません: $MODEL_DIR"
  echo "    モデル tar はスキップします (アプリのみ配布 = 静的辞書フォールバック)。"
  MODEL_TAR=""
fi

# --- 4. チェックサム + 後片付け ---
echo "[4/4] チェックサム..."
( cd "$DIST" && shasum -a 256 "$(basename "$APP_ZIP")" \
    ${MODEL_TAR:+"$(basename "$MODEL_TAR")"} > SHA256SUMS.txt )
rm -rf "$STAGE" "$DIST/model-stage"

echo ""
echo "=== 完成 (dist/) ==="
du -h "$DIST"/*
echo ""
echo "SHA256:"
cat "$DIST/SHA256SUMS.txt"
echo ""
echo "次: docs/RELEASE.md の手順で GitHub Release に添付 (公開はユーザー確認後)。"
