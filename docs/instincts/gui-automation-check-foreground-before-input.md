---
id: gui-automation-check-foreground-before-input
trigger: "user が使用中の Mac で GUI を自動操作して実機確認する時"
action: "座標クリックやキー入力の前に前面アプリと会議アプリの有無を確かめ、ボタンは AX の名前で押し、ホットキーは HID CGEvent で送る。押す→確かめるは 1 回の Bash で続けて行う"
severity: MEDIUM
severity_evidence: ""
promotion_override: false
confidence: 0.4
scope: global
status: active
evidence:
  - kind: initial
    ref: "2026-10-15 context-drop v0.1.8 実機確認"
    note: "照合語: GUI/座標/AXPress/CGEvent — 既存 instinct・skill に該当なし。会議アイコンでメニューバーの並びがずれ、右クリックとキー入力が別アプリのメニューへ、クリックが会議中の Zoom へ入った"
---

## 補足

System Events の keystroke は Carbon のグローバルホットキーに届かない。AppleScript の `entire contents` の並び順は画面上の順と一致しないので、どの行が変わったかの判断に使わない。
