# Toche 1.1.1 Hotfix Implementation Plan

> **For Hermes:** Dispatch to Claude Code via `claude -p "...plan below..." --allowedTools "Bash,Read,Write,Edit,Grep,Glob,Task"`

**Goal:** Fix schema version mismatch crash — `"version 11 > 9"` error in checkpoint and cache — caused by three modules sharing `~/.toche/ledger.db` with different EXPECTED_VERSION constants.

**Architecture:** Single SQLite database (`~/.toche/ledger.db`) holding three logical "databases" (ledger, safe_cache, checkpoints) sharing a single `schema_version` table. The ledger owns the version counter. Cache and checkpoint must raise their EXPECTED_VERSION to 11 and make their table creation unconditional (just check `IF NOT EXISTS`).

**Tech Stack:** Rust, rusqlite

---

## Root Cause

| Module | File | EXPECTED_VERSION | Table |
|--------|------|-----------------|-------|
| Meter | `src/meter/db.rs:77` | **11** | `ledger` |
| Cache | `src/safe_cache/cache_db.rs:74` | **9** | `safe_cache` |
| Checkpoint | `src/continuity/checkpoint.rs:78` | **9** | `checkpoints` |

All three open `ledger.db` and share the `schema_version` table. Ledger writes version 11 first (it's always the first to open). Then checkpoint/cache open the same DB, read 11, see `11 > 9`, and crash.

**Fix:** Raise cache and checkpoint to `EXPECTED_VERSION = 11`. Make their `CREATE TABLE` blocks use `IF NOT EXISTS` unconditionally (no version gate). Remove the `if current_version < 9` gate.

---

## Tasks

### Task 1: Fix `src/safe_cache/cache_db.rs`

**Files:**
- Modify: `src/safe_cache/cache_db.rs:74`

**Step 1: Change EXPECTED_VERSION from 9 to 11**

Line 74:
```rust
const EXPECTED_VERSION: i32 = 9;
```
→
```rust
const EXPECTED_VERSION: i32 = 11;
```

**Step 2: Remove version gate from safe_cache table creation**

Lines 88-106 — the `if current_version < 9 { CREATE TABLE safe_cache ... }` block.

The `CREATE TABLE IF NOT EXISTS` on line 89 is safe to run unconditionally. Remove the `if` wrapper so it becomes:

```rust
// safe_cache table (may already exist)
conn.execute(
    "CREATE TABLE IF NOT EXISTS safe_cache (
        id INTEGER PRIMARY KEY,
        fingerprint TEXT NOT NULL,
        project_path TEXT NOT NULL DEFAULT '',
        provider TEXT NOT NULL,
        model TEXT NOT NULL,
        total_tokens INTEGER NOT NULL,
        input_tokens INTEGER NOT NULL DEFAULT 0,
        output_tokens INTEGER NOT NULL DEFAULT 0,
        content_hash TEXT NOT NULL DEFAULT '',
        created_at TEXT NOT NULL,
        last_hit_at TEXT NOT NULL
    )",
    [],
)?;
```

Also remove the `INSERT INTO schema_version (version) VALUES (9)` on the line after CREATE TABLE (it's redundant — the version is already 11 from ledger).

**Step 3: Verify the rejection guard still works**

Test: create a `schema_version` table with version 99, open CacheDb — should reject with "99 > 11."

---

### Task 2: Fix `src/continuity/checkpoint.rs`

**Files:**
- Modify: `src/continuity/checkpoint.rs:78`

**Step 1: Change EXPECTED_VERSION from 9 to 11**

Line 78:
```rust
const EXPECTED_VERSION: i32 = 9;
```
→
```rust
const EXPECTED_VERSION: i32 = 11;
```

**Step 2: Remove version gate from checkpoints table creation**

Lines 91-112 — the `if current_version < 9 { CREATE TABLE checkpoints ... }` block. Remove the `if` wrapper:

```rust
// checkpoints table (may already exist)
conn.execute(
    "CREATE TABLE IF NOT EXISTS checkpoints (
        id INTEGER PRIMARY KEY,
        project_path TEXT NOT NULL,
        git_head TEXT NOT NULL DEFAULT '',
        workspace_fingerprint TEXT NOT NULL DEFAULT '',
        task TEXT NOT NULL DEFAULT '',
        completed TEXT NOT NULL DEFAULT '',
        changed_files TEXT NOT NULL DEFAULT '',
        verification TEXT NOT NULL DEFAULT '',
        open_risks TEXT NOT NULL DEFAULT '',
        next_action TEXT NOT NULL DEFAULT '',
        facts_json TEXT NOT NULL DEFAULT '{}',
        model_assisted INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    )",
    [],
)?;
```

Remove `INSERT INTO schema_version (version) VALUES (9)` on line 111.

**Step 3: Verify the rejection guard still works**

Test: create a DB with version 99, open CheckpointDb — should reject.

---

### Task 3: Run existing tests

```bash
cd ~/Coding/toche && cargo test --all 2>&1 | tail -10
```

Expected: all 377+ tests pass.

---

### Task 4: Add regression test

**Create:** `tests/m24_schema_version_sync.rs`

Test that all three DB owners agree on EXPECTED_VERSION 11:

```rust
#[test]
fn all_schema_versions_agree() {
    // Verify meter/cache/checkpoint all expect version 11
    // Use compile-time checks or just assert_eq! on the constants
    // (If constants are private, open test-only access)
}

#[test]
fn checkpoint_opens_after_ledger_writes_v11() {
    // Simulate the actual failure scenario:
    // 1. Open LedgerDb → writes schema_version 11
    // 2. Open CheckpointDb on same file → should succeed
}

#[test]
fn cache_opens_after_ledger_writes_v11() {
    // Same for CacheDb
}
```

---

### Task 5: Version bump

**Modify:**
- `Cargo.toml` — `version = "1.1.1"`
- `package.json` — `"version": "1.1.1"`

---

### Task 6: clippy, fmt, cargo test

```bash
cd ~/Coding/toche && cargo fmt && cargo clippy -- -D warnings && cargo test --all
```

Expected: all green.

---

### Task 7: Commit

```bash
git add -A
git commit -m "fix: sync shared schema_version — cache and checkpoint expect v11 not v9

All three modules (meter, cache, checkpoint) share ~/.toche/ledger.db and
the schema_version table. Meter's v11 (protocol column, M11) broke cache
and checkpoint which still expected v9.

- Bump cache_db EXPECTED_VERSION 9 → 11
- Bump checkpoint_db EXPECTED_VERSION 9 → 11
- Remove version gates on CREATE TABLE (use IF NOT EXISTS unconditionally)
- Add regression tests

Fixes: 'Database was created by a newer version of Toche (schema version 11 > 9)'"
```

---

### Verification

After the hotfix:
```bash
# Test on machine with existing v11 DB:
toche checkpoint save --task "test" --completed "done" --next "verify" --model-assisted
# Expected: works, shows checkpoint saved
toche cache inspect
# Expected: works, shows cache entries
```
