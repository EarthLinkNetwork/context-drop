# Recent Dispatches の内部スクロール (v0.1.7) — review scope

## 保証すること
- Recent Dispatches のリスト (.dispatch-list) だけが最大 220px で内部スクロールし、10 件あってもウィンドウ全体のスクロールを伸ばさない。
- 内部スクロールが端に達してもページ側へスクロールが連鎖しない (overscroll-behavior: contain)。
- 版を 0.1.7 に揃える (tauri.conf / plugin / marketplace / CHANGELOG)。

## 保証しないこと
- Current Packet 側の高さ制御 (既存のまま)。

## 前提
- CSS のみの変更で、挙動・データには触れない。

## 受け入れる限界
- 220px 固定 (ウィンドウの高さには追従しない)。
