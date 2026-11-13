#!/usr/bin/env bash
#
# regression-convert.sh — 変換エンジンのデグレ（回帰）試験
#
# 過去に壊れた/直したケース（ん+母音 境界・nn ルール・学習・複数文節 等）を
# まとめて変換し、期待どおりかを一括判定する。モデル/ロジックを変えたら叩く。
#
# 使い方:
#   ./nuko-macos/scripts/regression-convert.sh            # モデルのみ(決定的・推奨)
#   ./nuko-macos/scripts/regression-convert.sh --personal # 実機の個人学習込み
#   ./nuko-macos/scripts/regression-convert.sh --no-build  # ビルド省略(ビルド済み前提)
#
# 前提:
#   - libakaza モデルが配置済み
#     ($HOME/Library/Application Support/nuko-ime/akaza-model の 6 ファイル)。
#   - akaza feature 付きの nuko CLI が要る (本スクリプトが --release でビルドする)。
#     ※ install.sh は CLI を akaza 無しで上書きするので、install 後は再ビルドが要る。
#
# 判定の種類:
#   top  … 文全体の連結(最上位の変換)が期待文字列と完全一致すること。
#   cand … 期待文字列が候補のどこかに出ること(最上位である必要はない)。
#          例「しんに」の真意はモデル最上位でなくてよいが候補には必須。
#
# 終了コード: 失敗が 1 件でもあれば 1、全通過なら 0 (CI/手動ゲート両用)。

set -uo pipefail

cd "$(dirname "$0")/../.." || exit 2

PERSONAL=""
DO_BUILD=1
for arg in "$@"; do
  case "$arg" in
    --personal) PERSONAL="--personal" ;;
    --no-build) DO_BUILD=0 ;;
    -h | --help)
      sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "不明な引数: $arg" >&2
      exit 2
      ;;
  esac
done

NUKO="./target/release/nuko"

if [ "$DO_BUILD" -eq 1 ]; then
  echo "[build] cargo build --release --bin nuko --features akaza ..."
  if ! cargo build --release --bin nuko --features akaza; then
    echo "ビルド失敗" >&2
    exit 2
  fi
fi

if [ ! -x "$NUKO" ]; then
  echo "nuko CLI が見つからない: $NUKO (--no-build を外して再実行)" >&2
  exit 2
fi

# reading|expected|type|note
CASES=(
  "くりーんなほうで|クリーンな方で|top|★ん+な を阿呆にしない (#194 回帰)"
  "ごかくににただけますとさいわいです|ご確認いただけますと幸いです|top|単一n ん+母音(文) (#193)"
  "ごかくにんにただけますとさいわいです|ご確認いただけますと幸いです|top|nn ん+母音(文) (#192)"
  "しんに|真意|cand|ん+母音(単語) 候補に有 (#160)"
  "たんい|単位|top|ん+母音(単語)"
  "はんい|範囲|top|ん+母音(単語)"
  "せんえん|千円|top|ん+母音(単語)"
  "ざんねん|残念|top|ん+な を壊さない"
  "あんない|案内|top|ん+な を壊さない"
  "おんな|女|top|ん+な を壊さない"
  "こんにちは|こんにちは|top|ん+に を壊さない(モデルのみ)"
  "いんようします|引用します|top|nn+y ルール (#93)"
  "きんようび|金曜日|top|nn ルール"
  "しんようできる|信用できる|top|nn ルール"
  "わたしのなまえはなかの|私の名前は中野|top|複数文節/名前"
  "ぽけっとにいれる|ポケットに入れる|top|助詞 に"
  "にほんにいく|日本に行く|top|助詞 に"
  "このなつやすみ|この夏休み|top|複数文節"
)

mode="モデルのみ"
[ -n "$PERSONAL" ] && mode="個人学習込み(--personal)"
echo "=== 変換回帰試験 ($mode) ==="
printf '%-32s %-26s %-6s %s\n' "読み" "結果/判定" "種別" "備考"
printf '%s\n' "--------------------------------------------------------------------------------"

fail=0
pass=0
for c in "${CASES[@]}"; do
  IFS='|' read -r reading expected ctype note <<<"$c"
  out="$($NUKO convert "$reading" --libakaza $PERSONAL 2>/dev/null)"
  if [ "$ctype" = "cand" ]; then
    if printf '%s' "$out" | grep -qF "$expected"; then
      mark="✅ $expected"
      pass=$((pass + 1))
    else
      mark="❌ $expected 無し"
      fail=$((fail + 1))
    fi
  else # top
    got="$(printf '%s' "$out" | grep "連結:" | sed 's/.*連結: //')"
    if [ "$got" = "$expected" ]; then
      mark="✅ $got"
      pass=$((pass + 1))
    else
      mark="❌ $got (期待 $expected)"
      fail=$((fail + 1))
    fi
  fi
  printf '%-32s %-26s %-6s %s\n' "$reading" "$mark" "$ctype" "$note"
done

printf '%s\n' "--------------------------------------------------------------------------------"
echo "結果: $pass 通過 / $fail 失敗 (全 ${#CASES[@]} 件)"
[ "$fail" -eq 0 ]
