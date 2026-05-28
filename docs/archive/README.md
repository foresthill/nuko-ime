# docs/archive/

過去の設計ドキュメントを保存しています。

## アーカイブ方針

ぬこIME は OSS プロジェクトであり、設計判断の **文脈** (なぜそうしたか) を
将来の貢献者が辿れることに価値があると考えています。そのため、計画が大きく
変更された際にも、当時のドキュメントを削除せず本ディレクトリに保存します。

各ファイルの先頭にはアーカイブされた日付・経緯・現役ドキュメントへのリンク
が記載されています。

## 一覧

| ファイル | アーカイブ日 | 経緯 | 後継 |
|---|---|---|---|
| `SPECIFICATION-2025-12.md` | 2026-05 | lindera/vibrato 想定 + 同時クロスプラットフォーム想定 → Path B (libakaza) + macOS 一点突破に変更 | [`docs/ARCHITECTURE.md`](../ARCHITECTURE.md) |
| `PROJECT_PLAN-2025-12.md` | 2026-05 | 10 名規模のフルチーム想定 → 個人 OSS の現実に変更 | [`docs/ROADMAP.md`](../ROADMAP.md) |

## 参考: OSS でのアーカイブ慣習

「設計ドキュメントは削除より保存」は ADR (Architecture Decision Records) や
RFC (例: Rust RFCs, Python PEP) の慣習に近い考え方です。ぬこIME ではこの
慣習を緩く採用しています。
