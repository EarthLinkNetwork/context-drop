# Recent Dispatches の内部スクロール (v0.1.7) — review scope

## 保証すること
- Recent Dispatches のリスト (.dispatch-list) だけが最大 220px で内部スクロールし、10 件あってもウィンドウ全体のスクロールを伸ばさない。
- 内部スクロールが端に達してもページ側へスクロールが連鎖しない (overscroll-behavior: contain)。
- 版を 0.1.7 に揃える (tauri.conf / plugin / marketplace / CHANGELOG)。

## 保証しないこと
- Current Packet 側の高さ制御 (既存のまま)。
- スクロール連鎖の抑止は overscroll-behavior 対応 WebView (Safari 16+ 相当 = macOS 13 以降、または WebKit 更新済みの macOS 12) のみ。それ未満では履歴の端でページ側へスクロールが連鎖するが、内部スクロール自体は動き、全履歴に到達できる (見た目の劣化のみ・代替 JS は入れない)。

## 前提
- CSS のみの変更で、挙動・データには触れない。

## 受け入れる限界
- 220px 固定 (ウィンドウの高さには追従しない)。
- jsdom は CSS を評価しないため、高さ上限と内部スクロールは単体テストでなく実機検証 (v0.1.7 を /Applications に入れ、10 件の履歴を内部スクロールで最下部まで表示しページ位置が動かないことをスクリーンショットで確認) で担保する。
