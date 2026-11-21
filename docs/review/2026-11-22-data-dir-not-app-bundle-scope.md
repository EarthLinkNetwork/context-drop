# v0.1.11 — macOS data dir off the `.app` name (scope)

## 保証すること

- macOS: the data dir is `~/Library/Application Support/com.contextdrop.desktop`. The desktop app, once at startup before anything opens the DB, renames a pre-0.1.11 `com.contextdrop.app` there and puts `com.contextdrop.app` back as a relative symlink to it. Old paths (v0.1.10 plugin `claim.sh`, `~/.local/bin/context-drop`, a CLI that resolved the old path) keep reaching the same data.
- Never deletes or merges data. Both real dirs present: the new one is used, the old one is left untouched.
- Idempotent: every start re-creates a missing old-name link and removes a stale tmp link; our own link is recognised.
- A user's own symlink at the old name (reachable or offline) moves as a link; the data it points to stays put.
- A process that resolved the old location never creates a real `com.contextdrop.app` again after the move (it fails with a retry message).
- Windows and Linux: nothing moves; leaf stays `com.contextdrop.app`.
- Bundle identifier unchanged (`com.contextdrop.app`): macOS permissions and the updater are unaffected.

## 保証しないこと

- `~/Library/WebKit/com.contextdrop.app` (named by the bundle id; written rarely, not part of the measured problem).
- Merging data when both real directories exist.

## 前提

- Old and new dirs are siblings in the same directory, so `rename` is atomic.
- Measured on this Mac (macOS 27): a real `.app` dir whose contents change every 0.5 s caused 92 failed `lsd` registrations / 40 s; a `.app`-named symlink to such a dir caused 0, also when written through the link. Quitting the 0.1.10 app took `pbcopy` from 2.6 s to 0.03 s and `lsd` from ~100% to 0% CPU.
- Only the desktop app migrates; it is a single instance and the CLI copy is installed by the desktop app.

## 受け入れる限界

- An old (≤0.1.10) desktop app running at the same time as the new one.
- A CLI command that resolved the old path and runs into the instant between the two renames fails once with a retry message.

## user の裁定

- 2026-11-21: 直し方は「データフォルダ名だけ変える」(bundle ID は変えない) を user が選択。版は v0.1.11。
