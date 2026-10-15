---
id: never-open-same-file-for-write-before-read
trigger: "スクリプトでファイルを読んで書き換える時（Python の open 等）"
action: "同じパスを書き込みで開く前に内容を読み切って変数に入れる。open(p,'w').write(open(p).read()) の形は書き込み側が先に空にするので使わない"
severity: MEDIUM
severity_evidence: ""
promotion_override: false
confidence: 0.4
scope: global
status: active
evidence:
  - kind: initial
    ref: "2026-10-15 context-drop v0.1.9 実装"
    note: "照合語: truncat/open( — 該当なし。accelerator.ts が空になり git show HEAD で復元して編集をやり直した"
---
