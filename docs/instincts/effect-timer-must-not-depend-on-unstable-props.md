---
id: effect-timer-must-not-depend-on-unstable-props
trigger: "React の useEffect でタイマーや購読を張り、props のコールバックを使う時"
action: "依存配列には開始・終了を決める状態だけを入れ、コールバックは最新値を ref に入れて呼ぶ。毎回作られる関数を依存に入れると再描画のたびにタイマーが張り直される"
severity: MEDIUM
severity_evidence: ""
promotion_override: false
confidence: 0.4
scope: global
status: active
evidence:
  - kind: initial
    ref: "2026-10-15 context-drop v0.1.9 PR #13 独立レビュー Blocking"
    note: "照合語: useEffect/依存配列/再描画 — 該当なし。ショートカット記録の 19 秒タイマーが再描画で張り直され、記録が終わらない・途中で終わる不具合になった"
---
