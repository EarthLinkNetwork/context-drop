# アプリ内の自動更新（autoupdater）とダウンロード数の集計 (v0.1.10) — review scope

## 保証すること
- **更新の確認**: `tauri-plugin-updater` 2.13 を使い、`https://github.com/EarthLinkNetwork/context-drop/releases/latest/download/latest.json` を読んで、いまの版より新しい版があるかを調べる。
  - 確認はバックエンド（Rust）だけで行う。WebView の JS には updater の権限を渡さない（capabilities は変えない）。
  - **自動確認（既定 ON）**: 起動から約 10 秒後に 1 回確認する。その後は 10 分ごとに起きて、**前回の成功した確認**から 24 時間（壁時計 `SystemTime`）以上たっていれば確認する。スリープ中に止まる時計で 24 時間を数えないので、スリープから戻った後も最長 10 分（起きている時間）で確認する。Settings の「Check for updates automatically」で OFF にできる（`autoCheckUpdates`・DB に保存・既存の利用者も ON から始まる）。
  - 「前回の確認」を更新するのは成功した確認（latest.json を取得して読めた）だけ。失敗したら 1 時間後に再試行する。前回の確認の時刻はメモリにだけ持つ（起動時には必ず確認する）。
  - **通信が止まっても固まらない**: latest.json の取得は 30 秒、ダウンロードは全体で 10 分のタイムアウト。確認・更新中の印（同時に 1 つだけ）は、成功・失敗・タイムアウトのどの経路でも外れる（drop guard）。
  - 置き換え（install）はメインスレッドで行わない（管理者パスワードが要るとき、プラグインはメインスレッドで AppleScript を動かして待つため）。
  - **手動確認**: Settings → Advanced → Updates の「Check for Updates」で確認する。結果（最新です／新しい版があります／失敗した理由）をその場に出す。
  - 同時に動く確認・更新は 1 つだけ。更新の途中に確認を押しても何もしない。
- **更新の知らせ**: 新しい版が見つかったら、トレイメニューの項目「Check for Updates…」を「Update to vX.Y.Z…」に変える（ふだんウィンドウは隠れているため、トレイから気付けるようにする）。押すとウィンドウを開く。新しい版が無いときに押すと、確認を行ってウィンドウを開く（結果は Settings → Advanced → Updates に出る）。
- **更新の適用**: 新しい版があるときは、ウィンドウ上部に「Context Drop vX.Y.Z is available」と「Install & Restart」を出す。押すとダウンロード（進み具合を % で表示）→ 署名の検証 → `.app` の置き換え → 再起動。
  - **ダウンロードしたものは署名を検証してから置き換える**。署名の公開鍵はアプリに埋め込む（`plugins.updater.pubkey`）。`requireSignedVersion: true` で、署名に含まれる版と latest.json の版が違う更新は拒否する（古い正規の版へ戻される攻撃を防ぐ）。版が今より新しくないものは入れない（`allowDowngrades` は false のまま）。
  - 自分から更新を入れることはしない（利用者が「Install & Restart」を押したときだけ）。
  - 失敗したときは理由を表示し、利用者はやり直せる。置き換え前に失敗した場合、今のアプリはそのまま動く。
  - **更新できない場所では、置き換えを試さず理由を出す**: `.app` の中から動いていない（開発ビルド `tauri dev` など）／DMG から直接起動（`/Volumes/` 配下）／App Translocation 中（ダウンロードフォルダから直接起動）。`/Applications` に移すよう案内する。開発ビルドで `target/debug` を消す事故を防ぐ。
  - macOS 以外では更新機能を出さない（配布は macOS arm64 のみ）。
- **リリース（`release.yml`）**: `vX.Y.Z`（数字だけ）のタグの push でだけ公開する（`v0.2.0-rc1` のようなタグでは動かない。スクリプトでも正規表現で確かめる）。DMG に加えて次を、**まず draft の Release にすべて上げ、ファイルがそろったことを確かめてから公開（`--draft=false --latest`）**する（draft は `releases/latest` に出ないので、latest.json が無い・古い時間を作らない）。同じタグの Release が既にあれば失敗させる（上書き `--clobber` をしない。ダウンロード数も消えない）。
  - `Context-Drop-vX.Y.Z-macos-arm64.app.tar.gz`（更新用の .app）と `.sig`（その署名）
  - `latest.json`（`version`・`notes`・`pub_date`・`platforms.darwin-aarch64.{url, signature}`）
  - 更新用の成果物は CI の設定ファイル `src-tauri/tauri.release.conf.json`（CI がその場で作る・`createUpdaterArtifacts: true` と版）でだけ作る。手元の `tauri build` は署名鍵なしでもこれまで通り動く。
  - 署名鍵 `TAURI_SIGNING_PRIVATE_KEY`（とパスワード）が無ければ、最初の検査で止める（既存の Apple 用の秘密と同じ扱い）。
  - **公開する前に CI で確かめる**: (1) タグの版と `tauri.conf.json` の版が一致する、(2) `.sig` と埋め込んだ公開鍵を base64 から戻し、`minisign -V` で tar.gz を検証し、**検証済みの trusted comment に `version:<版>` がある**ことを確かめる（`requireSignedVersion` で全員に拒否されるリリースを出さない）、(3) tar.gz を展開した写しの `.app` が codesign と Gatekeeper（spctl）の検査を通る、(4) latest.json を jq で確かめる（version は先頭の v 無しで (1) と同じ・`platforms["darwin-aarch64"].url` が上げるファイルの URL・signature が `.sig` と同じ・pub_date は RFC 3339）。tar.gz は bundler が作ったものを名前だけ変えて上げる（作り直さない）。
  - 手動実行（workflow_dispatch）は今まで通り何も公開せず、DMG・tar.gz・sig・latest.json を workflow artifact に上げる。入力 `version` を渡すと、その版でビルドする（E2E 用の `0.1.10-rc.1` など。公開はしない）。
- **ダウンロード数の集計**: `scripts/release-stats.sh` で、Release ごとに次の数を出す（GitHub API・費用 $0）。DMG（新規のダウンロード）／`.app.tar.gz`（自動更新で入れた回数）／`latest.json`（更新の確認の回数＝使われている台数の目安）。
- **文書**: README・SECURITY.md・CONTRIBUTING.md・docs/security.md・docs/architecture.md・docs/claude-code.md・docs/troubleshooting.md・アプリ内の Privacy information（`SettingsView` と `privacy_info` コマンド）の約束を次の内容に書き換える。
  - 「absolute」「設定で切れるものではない」の言い方をやめる。
  - 自動の更新確認が既定で ON で、最大で 1 日 1 回ほど GitHub から latest.json を読むこと。識別子も packet などのデータも送らないこと。GitHub には IP アドレスと updater の User-Agent が見えること。
  - 開発者は GitHub が公開しているファイルごとのダウンロード数（DMG・.app.tar.gz・latest.json）を見て、使われている台数の目安にしていること。自動確認を OFF にすれば、その数に入らないこと。
  - 「No telemetry, no analytics」は「上の匿名の更新確認の数のほかには、telemetry も analytics も無い」に変える。
  - 確認方法: `transmits nothing|no telemetry|local-only|No-network|absolute` を *.md・*.tsx・*.rs で grep し、残るのは書き換えた文か CHANGELOG の過去の記述だけにする。
  - リリースの手順と署名鍵の保管場所を文書に残す。
- **署名鍵の保管**: 鍵は repo の外（作業用の一時ディレクトリ）で作る。秘密鍵とパスワードを GitHub の repo secrets と dotvault（`context-drop/production`）に置く（鍵を失うと、既存の利用者に更新を届けられなくなるため、2 か所に置く）。リリースが通ったら手元の写しは消す。`.gitignore` に `*.key` を足す。
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
- `releases/latest` は、draft と prerelease を除いた最新の Release を指す（draft で上げてから公開するので、公開した時点で latest.json がそろっている）。
- E2E: `workflow_dispatch` の `version=0.1.10-rc.1` でビルドした（公開しない）アプリを /Applications に入れ、v0.1.10 を公開したあと、そのアプリから Check → Install & Restart で 0.1.10 に上がることを確かめる。
- updater がダウンロードしたファイルには quarantine 属性が付かない（Gatekeeper の初回確認は出ない）。署名の検証は updater 自身が行う。
- `/Applications` に書き込めない場合、updater は macOS の管理者パスワードの画面を出して置き換える（プラグインの既定の動き）。

## 受け入れる限界
- latest.json 自体には署名が無い。GitHub（TLS）を信頼する。偽の latest.json でできるのは「正規に署名された、より新しい版」を入れさせることだけ（`requireSignedVersion` と版の比較による）。
- オフラインのときの自動確認の失敗は、Settings → Updates に理由を出すだけで、バナーは出さない。
- 確認の間隔は「24 時間以上たったら、次の 10 分ごとの見回りで確認」なので、起きている時間で最大 24 時間 10 分あく。
- ダウンロードの途中で止まった（バイトが来ない）場合は、全体の 10 分のタイムアウトまで待つ。

## 独立レビュー（着手前・2026-10-15）
- 4 観点（security / macOS の動き / release pipeline / 製品の約束と UX）で読み、指摘ごとに別の agent が反証を試みた。残った 14 件をすべて上に反映した（prerelease タグ・署名の版の確認・鍵の置き場所・タイムアウト・スリープ中の時計・失敗した確認の扱い・rc での E2E・再実行の上書き・latest.json の検査・プライバシーの約束・書き換えの漏れ・トレイからの知らせ）。反証された 9 件は反映しない。

## user の裁定
- 2026-10-15 user: 「autoupdater の仕組みとダウンロード数を図るのは難しい?」→ 回答後「では実装して」。
- 2026-10-15 user が選択: 更新の確認は「起動時に自動確認（24 時間ごと・設定で OFF 可）」。README と SECURITY.md の「通信しない」約束は、これに合わせて書き換える。
