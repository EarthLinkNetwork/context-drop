# Scope: v0.1.12 — メニューの Check for Updates が結果を必ず出す

## 保証すること
- メニューバーのメニューの「Check for Updates…」を押すと、ウィンドウが開き、確認の結果がウィンドウの上部 (どのタブでも) に出る:
  確認中は「Checking for updates…」、新しい版が無ければ「Context Drop vX.Y.Z is the latest version (checked at HH:MM)」と OK、
  失敗なら理由と Try Again・Dismiss、新しい版があれば従来どおり Install & Restart。
- 版の表示と Check for Updates のボタンは Settings の先頭 (全タブ) にある。自動確認の設定は Settings → Advanced → Updates のまま。
- 自動確認 (起動時・1 日 1 回) は、新しい版を見つけた時以外は何も出さない (従来どおり)。
- 更新の取得・署名の検証・置き換え・自動確認の時期・通信の内容は変えない。

## 保証しないこと
- OS の通知・ダイアログでの知らせ (ウィンドウの中だけ)。
- メニューの項目名に結果を出すこと (「Update to vX.Y.Z…」への変化は従来どおり)。
- Windows / Linux の更新 (元から無い。メニューの項目は無効のまま)。

## 前提
(実測 2026-11-24・この Mac・v0.1.11)
- `/Applications/Context Drop.app` は 0.1.11、GitHub の最新 release と `latest.json` も 0.1.11。
- `updater::on_tray_item` はウィンドウを出して確認するだけ。`UpdateBanner` は `available`・`downloading`・`installing` の時しか描かず、
  結果の文は Settings → Advanced の `UpdatesPanel` にしか無い。そのため最新の時と失敗の時は何も起きないように見える。

## 方式
- バックエンドの `UpdateStatus` に `announce` を足す。メニューの項目が押されたら true。ユーザーが OK / Dismiss を押すか
  (`dismiss_update_notice`)、自動確認が始まると false。状態はバックエンドが持ち、画面は snapshot を読むだけ (既存の方式と同じ)。
- 確認が `busy` を取るのと最初の状態の変更は 1 つの手順 (`start` の lock)。メニューからの確認が、同時に始まった自動確認に
  `announce` を消されることは無い (断られた時も `announce` を立て、走っている確認の結果が出る)。
- 結果はページの先頭にあるので、メニューから頼まれるたび (`asked` が増えるたび) に画面を先頭へスクロールする。
- Settings の Check for Updates は、先に知らせを閉じてから確認する (結果は Settings の先頭にだけ出る)。
- 更新の無い platform では先頭のパネルを出さず、Advanced → Updates に「macOS のみ」と出す (従来の場所)。
- `UpdateBanner` は `announce` の時だけ、新しい版が無い結果も描く。Settings のタブを開いている間は先頭の `UpdatesPanel` が
  同じ結果を出すので、重ねて描かない。

## 受け入れる限界
- `announce` はアプリを終了すると消える (保存しない)。
- OK を押さずに置いた「最新です」は、次の自動確認 (最長で約 1 日後) が始まるまで残る。
- Settings の Check for Updates で確認した結果は、Settings の先頭にだけ出る (Capture のタブへは出さない)。
- updater.rs の変更は AppHandle が要るため単体試験が無い。画面の試験と、release 後のこの Mac での実機確認で確かめる。

## user の裁定
- 2026-11-24: 「メニューから確認した時は結果をメイン画面に必ず出す・Check for Updates を Settings の最初に見える位置にも置く」を v0.1.12 として進める。
