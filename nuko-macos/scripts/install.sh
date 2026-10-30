#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APP_NAME="NukoIME"
APP_BUNDLE="$APP_NAME.app"
INSTALL_DIR="$HOME/Library/Input Methods"

# 使用方法:
#   ./install.sh                  # default features (静的辞書のみ)
#   FEATURES=akaza ./install.sh   # libakaza バックエンドを有効化
#                                 # モデルを ~/Library/Application\ Support/nuko-ime/akaza-model/
#                                 # に配置していない場合は静的辞書フォールバックで起動
FEATURES="${FEATURES:-}"

echo "=== ぬこIME macOS ビルド＆インストール ==="
if [ -n "$FEATURES" ]; then
    echo "Features: $FEATURES"
fi
echo ""

echo "[1/4] ビルド中..."
cd "$PROJECT_ROOT"
if [ -n "$FEATURES" ]; then
    cargo build --release -p nuko-macos --features "$FEATURES"
else
    cargo build --release -p nuko-macos
fi

# BYOK/AI dreaming 用の nuko CLI も同梱する。
# メニュー「🤖 AIで学習を研ぎ直す」がこれを subprocess 起動する。
cargo build --release --bin nuko -p nuko-cli

echo "[2/4] .appバンドル作成中..."
rm -rf "$PROJECT_ROOT/target/$APP_BUNDLE"
mkdir -p "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/MacOS"
mkdir -p "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/Resources"

# バイナリコピー
cp "$PROJECT_ROOT/target/release/NukoIME" \
   "$PROJECT_ROOT/target/$APP_BUNDLE/Contents/MacOS/"

# nuko CLI を同梱 (IME から current_exe の隣 = Contents/MacOS/nuko として解決)
cp "$PROJECT_ROOT/target/release/nuko" \
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

# バンドルを「ほぼアトミック」に差し替える。
# rm -rf → cp -R は数秒バンドルが消える窓を作り、その隙に TIS が有効入力ソース一覧
# (AppleEnabledInputSources) から外して「ぬこIMEが選べない」状態になることがある
# (2026-10 実機)。新バンドルを隣に用意してから mv で一瞬置換し、窓を最小化する。
STAGING="${INSTALL_DIR:?}/${APP_BUNDLE:?}.new"
rm -rf "$STAGING"
cp -R "$PROJECT_ROOT/target/$APP_BUNDLE" "$STAGING"
rm -rf "${INSTALL_DIR:?}/${APP_BUNDLE:?}"
mv "$STAGING" "$INSTALL_DIR/$APP_BUNDLE"

# 新バンドルを TIS に再登録する。
# ★ 以前は `open` で起動させて再登録させていた (366ac0d) が、`open` は
#   (1) 入力ソース再登録の正規手段ではなく (Apple DTS: ログアウト無し反映の公式手段は
#       無い / FB23026482)、(2) 単体の設定ウィンドウを開き、(3) 直前の killall 後に
#       TIS が自動 respawn するのと競合して 1 本しかない IMK 接続
#       (com.nuko.inputmethod.Nuko_Connection) を奪い合い「追加できるのに打てない」状態を
#       招く (2026-10 実機、デバッグ中の手動 open で発症)。
#   正規の再登録 API は Carbon HIToolbox の TISRegisterInputSource。swift があればそれで
#   再登録する (無ければスキップ→入力ソース切替かログインで反映)。open はもう使わない。
if command -v swift >/dev/null 2>&1; then
    NUKO_APP="$INSTALL_DIR/$APP_BUNDLE" swift - <<'SWIFT' 2>/dev/null || true
import Carbon
import Foundation
let path = ProcessInfo.processInfo.environment["NUKO_APP"] ?? ""
let url = URL(fileURLWithPath: path) as CFURL
let status = TISRegisterInputSource(url)
print("TISRegisterInputSource -> \(status)")
SWIFT
else
    echo "  (swift が無いため TISRegisterInputSource をスキップ。入力ソース切替かログインで反映されます)"
fi

# ★ ここで `open` は絶対にしない。killall で止めた旧プロセスは、ユーザーが入力ソースを
#   選択した時点で TIS がクリーンに起動し直し、そのプロセスが接続を単独で握る。

echo "[4/4] 完了！"
echo ""
echo "=== セットアップ手順 ==="
echo "1. システム設定 → キーボード → 入力ソース → 編集..."
echo "2. '+' をクリックして「日本語」の中から「ぬこIME」を追加"
echo "3. メニューバーの入力ソースからぬこIMEを選択"
echo ""
echo "※ 反映されない時の順序:"
echo "   (1) 入力ソースを別IME(ことえり等)に切替→ぬこIMEに戻す"
echo "   (2) それでもダメならログアウト→ログイン"
echo "   Apple DTS 公式回答(FB23026482): 入力ソース更新をログアウト無しで反映する"
echo "   公式手段は無く、ログインが必要な場合があります。"
