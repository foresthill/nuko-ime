#!/bin/bash
set -euo pipefail

# 実機 smoke test ヘルパー。
#
# controller.rs (= IMK 境界) を触った PR はマージ前にこれで実機確認する。
# ユニットテストも CI も実機の IMK 経由入力を検証しないため、IMK の方式違反・
# FFI 型不整合 (CLAUDE.md 落とし穴 #9 参照) はこのゲートでしか捕まらない。
#
# 使い方:
#   ./nuko-macos/scripts/smoke-test.sh          # release ビルドで install + 再起動 + チェックリスト
#   ./nuko-macos/scripts/smoke-test.sh --debug  # debug ビルド(panic=unwind+シンボル)で panic 採取
#
# FEATURES は install.sh と同じく環境変数で上書き可 (default: akaza)。

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APP_NAME="NukoIME"
APP_BIN="$HOME/Library/Input Methods/$APP_NAME.app/Contents/MacOS/$APP_NAME"
FEATURES="${FEATURES:-akaza}"
DEBUG_LOG="/tmp/nuko_smoke_debug.log"

MODE="release"
if [ "${1:-}" = "--debug" ]; then
    MODE="debug"
fi

print_checklist() {
    cat <<'EOF'

=== 実機 smoke test チェックリスト (手で打って確認) ===
  ★ まず: 入力ソースを 一度ことえり等に切替 → NukoIME に戻す
     (再インストール直後は、既に起動中のアプリが古い IME 接続を掴んだままで、
      新ビルドが反映されない。切替→戻すと macOS が新バイナリに繋ぎ直す。
      killall+open だけでは不十分。2026-06-25 に 2 回ハマったので必須手順)

  1. 「にほんご」打鍵 → ローマ字がかなに変換される    (= 打てる)
  2. Space → 変換 / Enter → 確定 / Backspace → 1 文字削除
  3. 数キーを連打しても プロセスが crash しない
  4. (segmented 変換時) ←→ で文節フォーカス移動 / Shift+←→ で文節の右端を伸縮
  5. 候補が多い変換でパネルが 1 ページ (9 候補) に収まり、ページ送りできる

  プロセス生存確認:  pgrep -lf NukoIME
EOF
}

if [ "$MODE" = "release" ]; then
    echo "=== smoke-test: release モード ==="
    # 通常の install.sh をそのまま使う (build release + .app 作成 + install + killall)
    FEATURES="$FEATURES" "$SCRIPT_DIR/install.sh"

    echo "=== 新ビルドを起動 ==="
    open "$HOME/Library/Input Methods/$APP_NAME.app"
    sleep 2
    if pgrep -lf "$APP_NAME" >/dev/null; then
        echo "プロセス起動 OK: $(pgrep -lf "$APP_NAME" | head -1)"
    else
        echo "⚠️  プロセスが起動していません。クラッシュの可能性。--debug で再採取してください。"
    fi
    print_checklist
else
    echo "=== smoke-test: debug モード (panic 採取) ==="
    if [ ! -d "$HOME/Library/Input Methods/$APP_NAME.app" ]; then
        echo "⚠️  .app が未インストールです。先に release モードか install.sh を実行してください。"
        exit 1
    fi

    echo "[1/3] debug ビルド (panic=unwind + シンボル付き)..."
    cd "$PROJECT_ROOT"
    cargo build -p nuko-macos --features "$FEATURES"

    echo "[2/3] 既存プロセス停止 + debug バイナリを .app に差し込み..."
    killall "$APP_NAME" 2>/dev/null || true
    sleep 1
    cp -v "$PROJECT_ROOT/target/debug/$APP_NAME" "$APP_BIN"

    echo "[3/3] RUST_BACKTRACE=full で起動 (stderr → $DEBUG_LOG)..."
    : > "$DEBUG_LOG"
    RUST_BACKTRACE=full "$APP_BIN" >"$DEBUG_LOG" 2>&1 &
    sleep 2
    if pgrep -lf "$APP_NAME" >/dev/null; then
        echo "プロセス起動 OK (debug)。"
    else
        echo "⚠️  起動直後にクラッシュ。$DEBUG_LOG を確認してください。"
    fi
    print_checklist
    cat <<EOF

  ※ debug モードでは .app のバイナリが debug ビルドに差し替わっています。
    smoke test 後は release を入れ直してください:
      FEATURES=$FEATURES ./nuko-macos/scripts/install.sh
  ※ クラッシュしたら panic 箇所はここ:
      grep -A3 "panicked at" $DEBUG_LOG
EOF
fi
