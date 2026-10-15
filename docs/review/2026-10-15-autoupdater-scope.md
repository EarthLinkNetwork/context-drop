# アプリ内の自動更新（autoupdater）とダウンロード数の集計 (v0.1.10) — review scope

## 保証すること
- **更新の確認**: `tauri-plugin-updater` 2.13 を使い、`https://github.com/EarthLinkNetwork/context-drop/releases/latest/download/latest.json` を読んで、いまの版より新しい版があるかを調べる。
  - 確認はバックエンド（Rust）だけで行う。WebView の JS には updater の権限を渡さない（capabilities は変えない）。
  - **自動確認（既定 ON）**: 起動から約 10 秒後に 1 回確認する。その後は 1 時間ごとに起きて、前回の確認から 24 時間（壁時計）以上たっていれば確認する（Mac のスリープ中に止まる時計に頼らない）。Settings の「Check for updates automatically」で OFF にできる（`autoCheckUpdates`・DB に保存・既存の利用者も ON から始まる）。
  - **手動確認**: Settings → Advanced → Updates の「Check for Updates」で確認する。結果（最新です／新しい版があります／失敗した理由）をその場に出す。
  - 同時に動く確認・更新は 1 つだけ。更新の途中に確認を押しても何もしない。
- **更新の適用**: 新しい版があるときは、ウィンドウ上部に「Context Drop vX.Y.Z is available」と「Install & Restart」を出す。押すとダウンロード（進み具合を % で表示）→ 署名の検証 → `.app` の置き換え → 再起動。
  - **ダウンロードしたものは署名を検証してから置き換える**。署名の公開鍵はアプリに埋め込む（`plugins.updater.pubkey`）。`requireSignedVersion: true` で、署名に含まれる版と latest.json の版が違う更新は拒否する（古い正規の版へ戻される攻撃を防ぐ）。版が今より新しくないものは入れない（`allowDowngrades` は false のまま）。
  - 自分から更新を入れることはしない（利用者が「Install & Restart」を押したときだけ）。
  - 失敗したときは理由を表示し、利用者はやり直せる。置き換え前に失敗した場合、今のアプリはそのまま動く。
  - **更新できない場所では、置き換えを試さず理由を出す**: `.app` の中から動いていない（開発ビルド `tauri dev` など）／DMG から直接起動（`/Volumes/` 配下）／App Translocation 中（ダウンロードフォルダから直接起動）。`/Applications` に移すよう案内する。開発ビルドで `target/debug` を消す事故を防ぐ。
  - macOS 以外では更新機能を出さない（配布は macOS arm64 のみ）。
- **リリース（`release.yml`）**: タグの push で、これまでの DMG に加えて次を同じ Release に**1 回の `gh release create` でまとめて**上げる（latest.json だけが無い時間を作らない）。
  - `Context-Drop-vX.Y.Z-macos-arm64.app.tar.gz`（更新用の .app）と `.sig`（その署名）
  - `latest.json`（`version`・`notes`・`pub_date`・`platforms.darwin-aarch64.{url, signature}`）
  - 更新用の成果物は CI の設定ファイル `src-tauri/tauri.updater.conf.json`（`createUpdaterArtifacts: true`）でだけ作る。手元の `tauri build` は署名鍵なしでもこれまで通り動く。
  - 署名鍵 `TAURI_SIGNING_PRIVATE_KEY`（とパスワード）が無ければ、最初の検査で止める（既存の Apple 用の秘密と同じ扱い）。
  - **公開する前に CI で確かめる**: (1) タグの版と `tauri.conf.json` の版が一致する、(2) `.sig` が、アプリに埋め込んだ公開鍵で検証できる（minisign）、(3) tar.gz の中の `.app` が codesign と Gatekeeper（spctl）の検査を通る、(4) latest.json の url・signature・version が、上げるファイルと一致する。
  - 手動実行（workflow_dispatch）は今まで通り何も公開せず、DMG・tar.gz・sig・latest.json を workflow artifact に上げる。
- **ダウンロード数の集計**: `scripts/release-stats.sh` で、Release ごとに次の数を出す（GitHub API・費用 $0）。DMG（新規のダウンロード）／`.app.tar.gz`（自動更新で入れた回数）／`latest.json`（更新の確認の回数＝使われている台数の目安）。
- **文書**: README・SECURITY.md・docs/security.md・docs/architecture.md・docs/claude-code.md・アプリ内の Privacy information の「通信しない」約束を「更新の確認とダウンロードだけは GitHub に通信する（packet などのデータは送らない・Settings で自動確認を OFF にできる）」に書き換える。リリースの手順と署名鍵の保管場所を文書に残す。
- **署名鍵の保管**: 秘密鍵とパスワードを GitHub の repo secrets と dotvault に置く（鍵を失うと、既存の利用者に更新を届けられなくなるため、2 か所に置く）。
- 版を 0.1.10 にそろえる（tauri.conf / plugin / marketplace / CHANGELOG）。

## 保証しないこと
- v0.1.9 以前からの自動更新（updater が入っていないので、v0.1.10 だけは手で入れ直す必要がある）。
- 更新を入れたあとの Capture の状態の引き継ぎ（再起動すると、起動時の既定どおり Capture は OFF。packet の中身は SQLite に残る）。Install の前に画面でそのことを伝える。
- 再起動したあとにウィンドウを自動で開くこと（これまでの起動と同じく、メニューバーに常駐して始まる）。
- 1 人ずつの利用者数（DAU など）。GitHub のダウンロード数は同じ人の再取得や bot も数える。
- Windows・Linux・Intel Mac 向けの更新。
- 署名鍵が漏れた場合の対策（鍵の差し替えは、利用者に手で入れ直してもらう必要がある）。

## 前提
- `tauri` 2.12 / `@tauri-apps/cli` 2.12.0 / `tauri-plugin-updater` 2.13.1。CLI 2.12.0 は署名の trusted comment に版を書く（`requireSignedVersion` が使える）。
- tauri の bundler は、署名・公証（notarize）した `.app` から更新用の tar.gz を作る（CI の (3) で確かめる）。
- `releases/latest` は、draft と prerelease を除いた最新の Release を指す。
- updater がダウンロードしたファイルには quarantine 属性が付かない（Gatekeeper の初回確認は出ない）。署名の検証は updater 自身が行う。
- `/Applications` に書き込めない場合、updater は macOS の管理者パスワードの画面を出して置き換える（プラグインの既定の動き）。

## 受け入れる限界
- latest.json 自体には署名が無い。GitHub（TLS）を信頼する。偽の latest.json でできるのは「正規に署名された、より新しい版」を入れさせることだけ（`requireSignedVersion` と版の比較による）。
- オフラインのときの自動確認の失敗は、Settings → Updates に理由を出すだけで、バナーは出さない。
- 確認の間隔は「24 時間以上たったら、次の 1 時間ごとの見回りで確認」なので、最大で約 25 時間あく。

## user の裁定
- 2026-10-15 user: 「autoupdater の仕組みとダウンロード数を図るのは難しい?」→ 回答後「では実装して」。
- 2026-10-15 user が選択: 更新の確認は「起動時に自動確認（24 時間ごと・設定で OFF 可）」。README と SECURITY.md の「通信しない」約束は、これに合わせて書き換える。
