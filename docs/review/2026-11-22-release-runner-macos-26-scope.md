# Scope: release runner macos-14 → macos-26

## 保証すること
- `.github/workflows/release.yml` の job が、GitHub が提供している Apple Silicon (arm64) の runner `macos-26` で動く。
- 成果物の arch (arm64)・名前 (`Context-Drop-vX.Y.Z-macos-arm64.*`)・更新の platform・署名と公証の手順は変わらない。
- 変更はこの 1 行だけ。他の step・secret・権限・trigger には触れない。

## 保証しないこと
- 将来 `macos-26` も廃止された時の自動追従 (その時はまた 1 行直す)。
- Intel (x86_64) 版の提供 (元から無い)。

## 前提 (実測)
- v0.1.11 の dry run (run 44922672353) は `The configuration 'macos-14-arm64-us-default' is not supported` で 0 秒で取り消し。
- CI の `macos-latest` は `Image: macos-26-arm64` (run 44920228180)。
- この branch の dry run (run 44922788194) は success: build・署名・公証 Accepted・確認まで通過。公開はしていない。

## 受け入れる限界
- 最低対応 macOS は tauri.conf.json の設定で決まり、runner の版では変わらない前提 (build 機の SDK が新しくなることによる差は dry run の成果物で確認する範囲に留める)。

## user の裁定
- 2026-11-22: v0.1.11 を「dry run → 本番 release」で出す。
