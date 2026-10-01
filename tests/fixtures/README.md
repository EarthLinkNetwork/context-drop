# Test fixtures

Small, checked-in inputs used by the integration tests in this crate.

- `unicode.txt` — multi-byte / emoji / full-width text, to prove capture and
  hashing are byte-exact for non-ASCII content.
- `sample.json` — a representative JSON error payload, to prove JSON
  classification and that raw content never leaks into the manifest.

Large inputs (e.g. the ">1 MB log" case) are generated in-memory by the tests
rather than committed, to keep the repository small.
