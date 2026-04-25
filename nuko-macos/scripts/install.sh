#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APP_NAME="NukoIME"
APP_BUNDLE="$APP_NAME.app"
INSTALL_DIR="$HOME/Library/Input Methods"

echo "=== ぬこIME macOS ビルド＆インストール ==="
echo ""

echo "[1/4] ビルド中..."
cd "$PROJECT_ROOT"
cargo build --release -p nuko-macos

echo "[2/4] .appバンドル作成中..."
rm -rf "$PROJECT_ROOT/target/$APP_BUNDLE"
mkdir -p "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/MacOS"
mkdir -p "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/Resources"

# バイナリコピー
cp "$PROJECT_ROOT/target/release/NukoIME" \
   "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/MacOS/"

# Info.plist コピー
cp "$PROJECT_ROOT/nuko-macos/Info.plist" \
   "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/"

# アイコンファイル (TIFF) コピー
for icon in icon.tiff icon-japanese.tiff icon-roman.tiff; do
    if [ -f "$PROJECT_ROOT/nuko-macos/resources/$icon" ]; then
        cp "$PROJECT_ROOT/nuko-macos/resources/$icon" \
           "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/Resources/"
    fi
done

# ローカライズファイル (.lproj) コピー
for lang in ja en; do
    if [ -d "$PROJECT_ROOT/nuko-macos/resources/$lang.lproj" ]; then
        mkdir -p "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/Resources/$lang.lproj"
        cp "$PROJECT_ROOT/nuko-macos/resources/$lang.lproj/"*.strings \
           "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/Resources/$lang.lproj/" \
           2>/dev/null || true
    fi
done

echo "[3/4] インストール中..."
mkdir -p "$INSTALL_DIR"

# 既存プロセスを停止
killall "$APP_NAME" 2>/dev/null || true
sleep 1

# 既存バンドルを削除してコピー
rm -rf "$INSTALL_DIR/$APP_BUNDLE"
cp -R "$PROJECT_ROOT/target/$APP_BUNDLE" "$INSTALL_DIR/"

echo "[4/4] 完了！"
echo ""
echo "=== セットアップ手順 ==="
echo "1. システム設定 → キーボード → 入力ソース → 編集..."
echo "2. '+' をクリックして「日本語」の中から「ぬこIME」を追加"
echo "3. メニューバーの入力ソースからぬこIMEを選択"
echo ""
echo "※ 初回はログアウト→ログインが必要な場合があります"
