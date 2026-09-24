# Text adapter acceptance — task 17

Verified on Windows on 2026-09-23. Both clients select the same text adapter through
`ProjectStore`; canonical records are the only shared source of truth.

The focused Rust suite covers immutable snapshots, independent cloned additions
and edits, stale content despite unchanged stored counters, full SQLite fixture
preservation, typed mutations across domains, no-op cursors, receipts, local-only
computer mappings/intents, schema and reference rejection, prepared-journal
recovery, and recovery blocked by an unexpected external edit. Eight tests pass.
The application regression suite passes 193 tests (five existing tests remain ignored).

Two independent MCP processes passed disjoint writes, one winner for conflicting
guards, and concurrent QA retries with exactly one shell execution. Evidence:
`target/storage-parity/9136a17e-c609-4cef-a586-c2a64e5a8a0d/prepare-evidence.json`.
The resolver/read-stability checks continue to pass with text now available and
server SQL still unavailable.

All six native Tauri/WebView2 scenarios pass against text storage:
read/poll byte and mtime preservation, disjoint edits, stale task drafts, stale
completion drafts, Markdown conflicts, and all workspace views without JavaScript
errors. The test uses isolated settings/project/WebView and desktop/MCP processes.
Evidence: `target/native-storage/1fbc5e87-de98-46a3-ad5d-68911b59df3d/evidence.json`.
The first native run exposed numeric revision ordering in the UI; polling now
compares the complete opaque storage cursor and handles revisions moving backwards.

Reproduce after building the frontend and both native binaries:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib --offline
python -B scripts/check_storage_parity.py --binary src-tauri/target/debug/adashi-mcp.exe --backend text
python -B scripts/check_storage_core.py --binary src-tauri/target/debug/adashi-mcp.exe
$env:ADASHI_TEST_BACKEND='text'
node scripts/check_native_storage.mjs
```

Set `ADASHI_PLAYWRIGHT_PACKAGE` to the Playwright package directory when it is
provided by another runtime. The frontend build and native test may require
permission to launch their child processes in a restricted Windows sandbox.

Format details and limits are in [storage-text-format.md](storage-text-format.md).
The in-memory query projection still reuses schema-14 domain SQL. File inventories
are checked on access; unchanged snapshots avoid parsing and rebuilding again.
There is no filesystem-wide atomicity for uncooperative Git/editor processes and
no claim of universal power-loss durability. No Git commands run automatically.
Task 18 owns user-facing conversion and tracked/ignored file setup.
