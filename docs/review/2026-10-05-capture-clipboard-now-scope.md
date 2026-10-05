# Capture Clipboard Now + app version — review scope

## 保証すること
- 「Capture Clipboard Now」ボタンは、押した時点のクリップボード内容を 1 回だけ現在の DRAFT に追加する。Capture セッションは開始しない（capturing フラグ・capture loop に触れない）。
- Capture ON/OFF どちらでも動き、ドロップと同じ経路（size 上限・dedupe・lifecycle lock 下の atomic append・claim 済み packet を idle 時に無視）を通る。
- 空/未対応のクリップボードは notice で知らせ、エラーにしない。
- UI ヘッダに実行中アプリの版（tauri.conf.json の version）を表示する。

## 保証しないこと
- Windows/Linux での実機動作（macOS のみ実機確認）。
- Capture ON 中に同じ内容を Now で取り込んだ後、capture loop が同内容を再取得しないこと（既存 dedupe に任せる）。

## 前提
- `do_drop` の append 部分を `append_once` に切り出しただけで、drop の挙動は不変。
- Tauri の同期 command は main thread で動く（NSPasteboard 読み取りは main thread で可）。

## 受け入れる限界
- 巨大な画像のクリップボード読み取り中は UI が一瞬 busy になる。
