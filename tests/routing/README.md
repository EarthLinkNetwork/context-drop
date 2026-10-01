# Routing & concurrency test matrix

The spec's routing tests (§43) and the concurrency requirements (§9, §33) are
implemented as executable tests in this crate. Run them with:

```sh
cargo test -p context-drop-integration-tests
# or the whole workspace:
cargo test --workspace
```

| Spec case | What it asserts | Test (file · fn) |
|-----------|-----------------|------------------|
| A | 1 packet, 2 sessions → exactly one claim succeeds (sequential) | `routing.rs` · `test_a_one_packet_two_sequential_claims_exactly_one_succeeds` |
| A (concurrent) | 1 packet, N simultaneous claims → exactly one succeeds | `concurrency.rs` · `one_packet_n_simultaneous_claims_exactly_one_succeeds` |
| B | 2 packets, 2 sessions → distinct packet assignments | `routing.rs` · `test_b_two_packets_two_sessions_get_distinct_packets` |
| C | old READY + current DRAFT → DRAFT wins | `routing.rs` · `test_c_draft_with_items_wins_over_old_ready` |
| D | same repo, different session ids → distinguished | `routing.rs` · `test_d_same_repo_different_sessions_are_distinguished` |
| E | 20 unique sessions → deterministic routing | `routing.rs` · `test_e_twenty_sessions_route_deterministically` |
| E (concurrent) | 20 packets, 20 threads → all distinct, none double-claimed | `concurrency.rs` · `twenty_packets_twenty_threads_all_distinct` |
| F | different CLAUDE_CONFIG_DIR values → one global store | `routing.rs` · `test_f_different_config_dirs_share_one_global_store` |
| §33 | append-vs-claim race → append never lands on a CLAIMED packet | `concurrency.rs` · `append_never_wins_against_claim` |
| §16 | consecutive dedupe (A A A B A → A B A) | `routing.rs` · `consecutive_duplicates_are_deduped_but_repeats_are_kept` |
| §27 | missing session id → refuse, do not route by project | `routing.rs` · `missing_session_id_is_rejected_not_routed_by_project` |
| §29 | undo returns claim to READY within window; scoped to session | `routing.rs` · `undo_returns_claim_to_ready_within_window`, `undo_outside_window_is_not_eligible` |

Lifecycle, size limits, TTL cleanup, and manifest-no-leak live in
`../integration/lifecycle.rs`; fixture-driven capture/classification lives in
`../integration/fixtures.rs`.
