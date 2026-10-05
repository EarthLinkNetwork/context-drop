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

---

# 追加範囲（同 PR）: Recent Dispatches 10 件 + note/terminal + v0.1.6

## 保証すること
- Desktop の「Recent Dispatches」は claims の新しい順に最大 10 件を出し、各行に project・note（/cd の指示文）・terminal（iTerm2 の wNtNpN 等）・session id 末尾・account（config dir 名）・cwd・件数・時刻・状態を表示する。Undo は先頭（最新）の 1 件だけ、CLAIMED/PROCESSING の時だけ。
- DB は v2 へ移行（claims に nullable の note/terminal 列）。v1 DB は既存行を保ったまま移行し、同時オープンでも ALTER は 1 回だけ（IMMEDIATE + user_version 再確認）。旧版アプリ/CLI は v2 DB でも動く。
- `context-drop note --claim-id` は自 session の claim にだけ書き込む。note/terminal は制御文字を空白化・trim・500 文字上限、空は NULL。
- note はユーザーの指示文（メタデータ）であり、packet の内容は記録しない。

## 保証しないこと
- iTerm2 のタブ番号はタブ並び替えで変わる（env の値をそのまま記録）。
- モデルが Step 1b の note 実行を省略した場合 note は空（terminal は自動記録）。

## 前提
- SKILL.md の `!` injection に $ARGUMENTS を渡すのはシェルインジェクションになるため、指示文はモデルが claim 後に `note` で記録する。

## 受け入れる限界
- note 記録はモデルの Bash 呼び出しなので許可プロンプトが出ることがある（consume と同様）。
