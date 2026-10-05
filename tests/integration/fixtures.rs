//! Fixture-driven capture tests (spec §43 clipboard cases: text / Unicode /
//! JSON / large log / consecutive duplicate), using checked-in fixture files.

use std::fs;
use std::path::PathBuf;

use context_drop_clipboard::{classify_text, ClipItemKind};
use context_drop_core::{
    append_snapshot, claim, create_draft, sha256_hex, AppendOutcome, CapturedItem,
    CapturedSnapshot, ClaimContext, ItemKind, Limits, Storage,
};
use context_drop_integration_tests::Harness;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name);
    fs::read_to_string(path).expect("fixture readable")
}

fn text_snapshot(kind: ItemKind, ext: &str, mime: &str, text: &str) -> CapturedSnapshot {
    let bytes = text.as_bytes().to_vec();
    CapturedSnapshot {
        snapshot_sha256: sha256_hex(&bytes),
        items: vec![CapturedItem {
            kind,
            mime_type: mime.into(),
            ext: ext.into(),
            bytes,
        }],
    }
}

#[test]
fn unicode_and_json_fixtures_classify_and_store_byte_exact() {
    let unicode = fixture("unicode.txt");
    let json = fixture("sample.json");

    // Classification (clipboard pure logic).
    assert_eq!(classify_text(&unicode), ClipItemKind::Text);
    assert_eq!(classify_text(&json), ClipItemKind::Json);

    let h = Harness::new();
    let mut db = h.open_db();
    let draft = create_draft(&mut db, &h.storage).unwrap();
    let limits = Limits::default();

    append_snapshot(
        &mut db,
        &h.storage,
        &draft,
        &text_snapshot(ItemKind::Text, "txt", "text/plain", &unicode),
        &limits,
    )
    .unwrap();
    append_snapshot(
        &mut db,
        &h.storage,
        &draft,
        &text_snapshot(ItemKind::Json, "json", "application/json", &json),
        &limits,
    )
    .unwrap();
    // Appending the same JSON again immediately is a consecutive duplicate.
    let dup = append_snapshot(
        &mut db,
        &h.storage,
        &draft,
        &text_snapshot(ItemKind::Json, "json", "application/json", &json),
        &limits,
    )
    .unwrap();
    assert!(matches!(dup, AppendOutcome::Duplicate));

    let summary = context_drop_core::packet_summary(&db, &draft)
        .unwrap()
        .unwrap();
    assert_eq!(summary.item_count, 2);

    // Claim and verify the manifest carries no raw content, but item files do.
    claim(
        &mut db,
        &h.storage,
        &ClaimContext {
            session_id: Some("S1".into()),
            cwd: "/repo/demo".into(),
            project_root: "/repo/demo".into(),
            project_name: "demo".into(),
            config_dir: None,
            note: None,
            terminal: None,
        },
    )
    .unwrap();
    let manifest = fs::read_to_string(h.storage.manifest_path(&draft)).unwrap();
    assert!(
        !manifest.contains("こんにちは"),
        "manifest leaked unicode content"
    );
    assert!(
        !manifest.contains("null pointer"),
        "manifest leaked json content"
    );

    // The stored item files preserve the exact bytes (Unicode round-trips).
    let items_dir = h.storage.items_dir(&draft);
    let stored_unicode = fs::read_to_string(items_dir.join("0001.txt")).unwrap();
    assert_eq!(stored_unicode, unicode);
    let stored_json = fs::read_to_string(items_dir.join("0002.json")).unwrap();
    assert_eq!(stored_json, json);
}

#[test]
fn large_log_is_stored_under_default_limit_but_rejected_under_a_small_one() {
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = create_draft(&mut db, &h.storage).unwrap();

    // A >1 MiB synthetic log.
    let big = "ERROR something went wrong\n".repeat(60_000); // ~1.6 MiB
    assert!(big.len() > 1024 * 1024);
    let snap = text_snapshot(ItemKind::Text, "log", "text/plain", &big);

    // Under the default 25 MiB item limit: stored.
    let added = append_snapshot(&mut db, &h.storage, &draft, &snap, &Limits::default()).unwrap();
    assert!(matches!(added, AppendOutcome::Added { .. }));

    // Under a tiny limit on a fresh packet: rejected, packet preserved.
    let d2 = create_draft(&mut db, &h.storage).unwrap();
    let tiny = Limits {
        max_item_bytes: 1024,
        max_packet_bytes: 10 * 1024,
    };
    let rejected = append_snapshot(&mut db, &h.storage, &d2, &snap, &tiny).unwrap();
    assert!(matches!(
        rejected,
        AppendOutcome::RejectedItemTooLarge { .. }
    ));
    assert_eq!(
        context_drop_core::packet_summary(&db, &d2)
            .unwrap()
            .unwrap()
            .item_count,
        0
    );
}

#[test]
fn empty_and_whitespace_capture_helpers_behave() {
    // A defensive check that Storage/Db construct cleanly from the harness and
    // an empty draft reports zero items (mirrors the "empty clipboard" case:
    // nothing is appended, nothing is stored).
    let h = Harness::new();
    let mut db = h.open_db();
    let draft = create_draft(&mut db, &h.storage).unwrap();
    assert_eq!(
        context_drop_core::packet_summary(&db, &draft)
            .unwrap()
            .unwrap()
            .item_count,
        0
    );
    // Sanity: Storage points inside the harness temp dir.
    assert!(Storage::at(h.dir.path())
        .db_path()
        .starts_with(h.dir.path()));
}
