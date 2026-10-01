//! Live clipboard smoke test. `#[ignore]`d by default because it touches the
//! real system clipboard and needs a desktop/pasteboard session. Run manually:
//!
//! ```sh
//! cargo test -p context-drop-clipboard --test live -- --ignored
//! ```

use context_drop_clipboard::{system_provider, ClipItemKind, Payload, Watcher};

#[test]
#[ignore = "touches the real system clipboard; run with --ignored on a desktop session"]
fn live_text_roundtrip() {
    // Put a known value on the clipboard.
    let mut cb = arboard::Clipboard::new().expect("open clipboard");
    let marker = "context-drop-live-smoke-こんにちは";
    cb.set_text(marker.to_string()).expect("set text");

    let provider = system_provider().expect("system provider");
    let mut watcher = Watcher::new(provider);

    // The first poll after reset should surface the current clipboard.
    let cap = watcher
        .poll()
        .expect("poll ok")
        .expect("a capture on first poll");
    assert!(!cap.items.is_empty());
    let item = &cap.items[0];
    assert!(matches!(item.kind, ClipItemKind::Text | ClipItemKind::Url));
    match &item.payload {
        Payload::Inline(bytes) => {
            assert_eq!(String::from_utf8_lossy(bytes), marker);
        }
        Payload::FileRef { .. } => panic!("expected inline text, got a file ref"),
    }

    // A second poll with no change yields nothing.
    assert!(watcher.poll().expect("poll ok").is_none());
}
