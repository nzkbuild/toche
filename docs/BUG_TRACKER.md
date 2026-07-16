# Bug Tracker — 1.0.2 Dogfood Session

Each entry: what the user saw, root cause, how/if it was fixed, and what class of bug
it reveals.

---

## Phase 1: Bugs found during live dogfooding (8 bugs)

### B1. `resolve_command` accessed wrong JSON path [FIXED]

**Symptom:** Bash tool calls (`cargo test`, `git diff`) were never reduced.

**Root cause:** `resolve_command` received the full `tool_use` block but read
`input.get("command")` directly. The correct path is `input.get("input")?.get("command")`.

**Fix:** `src/reduce/transform.rs:169` — c23bcd9.

**Bug class:** JSON structure assumption.

---

### B2. Cargo test fixture contained no strippable noise [FIXED]

**Symptom:** `bash_cargo_test_is_reduced` test failed on "tokens_reduced < tokens_raw".

**Root cause:** Fixture had clean output with nothing matching cargo filter strip patterns.

**Fix:** Added compilation noise — c23bcd9.

**Bug class:** Untestable test fixture.

---

### B3. `toche connect` only set `baseURL`, ignored `env.ANTHROPIC_BASE_URL` [FIXED]

**Symptom:** Connect reported success but traffic still went direct to upstream.

**Root cause:** Claude Code reads `env.ANTHROPIC_BASE_URL` which takes precedence over `baseURL`.

**Fix:** Connect now sets both. Uncommitted.

**Bug class:** Silent no-op.

---

### B4. No gateway health check before modifying settings [FIXED]

**Symptom:** Connect set `ANTHROPIC_BASE_URL` to `127.0.0.1:8743` while gateway was down.
User got "ConnectionRefused" and lost their session.

**Root cause:** Destructive config modification without verifying the replacement works.

**Fix:** Added `GET /health`, connect verifies before touching files. Uncommitted.

**Bug class:** Destructive action without precondition check.

---

### B5. Backup overwritten on every connect run [FIXED]

**Symptom:** Second connect destroyed the first (good) backup. Disconnect restored garbage.

**Root cause:** `std::fs::copy(settings_path, backup_path)` ran unconditionally.

**Fix:** Only create backup if one doesn't exist. Uncommitted.

**Bug class:** Backup corruption.

---

### B6. Disconnect didn't clean `env.ANTHROPIC_BASE_URL` [FIXED]

**Symptom:** After disconnect, `env.ANTHROPIC_BASE_URL` still pointed to Toche.

**Root cause:** Disconnect only removed `baseURL`, not the env-block URL.

**Fix:** Disconnect now also cleans `env.ANTHROPIC_BASE_URL`. Uncommitted.

**Bug class:** Incomplete reversal.

---

### B7. Wrong subcommand in error message [FIXED]

**Symptom:** "Start it first with: toche start" — `start` isn't a valid subcommand.

**Fix:** Corrected text. Uncommitted.

**Bug class:** Wrong user-facing instruction.

---

### B8. No `/health` endpoint [FIXED]

**Symptom:** No programmatic way to check if gateway is running.

**Fix:** Added `GET /health` → `200 "ok"`. Uncommitted.

**Bug class:** Missing observability.

---

## Phase 2: Multi-agent audit findings (7 new bugs)

### F1. `extract_model()` hand-parses JSON character-by-character [HIGH]

**File:** `src/gateway/routes.rs:23-52`

**What's wrong:** Walks raw JSON bytes looking for `"model"` without handling escape
sequences, nested objects, or the string appearing inside values. A request body with
`"message": "which model to use"` before the actual `"model"` key returns the wrong value
or `"unknown"`. Model name drives pricing and caching — a wrong parse cascades.

**Fix direction:** Replace with `serde_json::from_str` into a minimal struct.

---

### F2. `toche doctor` only checks `baseURL`, not `env.ANTHROPIC_BASE_URL` [MEDIUM]

**File:** `src/cli/doctor.rs:42-54`

**What's wrong:** Same bug class as B3. The shared `points_to_toche()` utility checks
both URL fields, but doctor manually inspects only `baseURL`. A user whose
`env.ANTHROPIC_BASE_URL` still points to Toche (but `baseURL` was cleared) gets a
false negative — doctor says "not connected" while traffic IS routing through Toche.

**Fix direction:** Reuse `points_to_toche()` in doctor.

---

### F3. `toche cache clear` orphans CAS blob files [HIGH]

**File:** `src/cli/cache.rs:95-97` + `src/safe_cache/cache_db.rs:194-213`

**What's wrong:** Cache clear deletes SQL rows but leaves the content-addressed storage
blobs on disk. Each safe_cache row has a `response_hash` pointing to `~/.toche/cas/`.
With no reverse index (hash → cache row), orphaned blobs accumulate silently. A user
running `toche cache clear` daily sees "Removed N entries" while disk usage keeps growing.

**Fix direction:** Collect `response_hash` values during clear and delete CAS files, or
add a `prune` command that garbage-collects unreferenced blobs.

---

### F4. No-backup disconnect path permanently loses original upstream URL [MEDIUM]

**File:** `src/cli/disconnect.rs:39-48`

**What's wrong:** When the `.toche-backup` file is missing (user deleted it, or it was
never created), disconnect surgically removes the Toche-prefixed URL. But it has no
record of what the URL was before Toche overrode it. The user's original
`ANTHROPIC_BASE_URL` is permanently gone.

**Fix direction:** On connect, save the original URL value before overwriting. The backup
file should be stored inside `~/.toche/` (not alongside settings.json) to reduce
accidental deletion.

---

### F5. Disconnect leaves empty `"env": {}` in settings [LOW]

**File:** `src/cli/disconnect.rs:39-48`

**What's wrong:** After removing `ANTHROPIC_BASE_URL` from the env object, disconnect
doesn't check whether `env` is now empty and should itself be removed. Leaves a
vestigial key.

**Fix direction:** Remove the `env` key if it becomes empty.

---

### F6. `toche setup` silently destroys existing `profiles.toml` [HIGH]

**File:** `src/cli/setup.rs:40-42`

**What's wrong:** Setup writes a fresh `profiles.toml` via `atomic_write_secure` with
zero checks for whether the file already exists. Unlike `connect` which creates a
backup, `setup` destroys all user customizations — custom profiles, API keys, model
mappings, filter configs — on every run. No confirmation, no backup, no `--force` flag.

**Fix direction:** Check if `profiles.toml` exists, refuse with an error unless `--force`
is passed, and create a `.bak` before overwriting.

---

### F7. `/health` endpoint returns hardcoded "ok" — no dependency verification [HIGH]

**File:** `src/gateway/server.rs:12-14`

**What's wrong:** `/health` returns `200 "ok"` unconditionally. It doesn't verify:
profiles load, upstream is reachable, ledger DB opens, CAS dir is writable. `toche connect`
uses this as a readiness gate — a user can "connect" to a gateway that is listening
but completely non-functional (bad profiles.toml, dead upstream, etc.). All Claude Code
API calls then fail with 500 errors and the user is dead in the water.

**Fix direction:** Add a `/ready` endpoint that exercises the critical path. `/health`
remains for liveness (process alive). `connect` calls `/ready` instead.

---

## Summary

| Status | Count | IDs |
|--------|-------|-----|
| Fixed (committed) | 2 | B1, B2 |
| Fixed (uncommitted) | 6 | B3, B4, B5, B6, B7, B8 |
| Not yet fixed | 7 | F1, F2, F3, F4, F5, F6, F7 |

**15 bugs total — 8 from live use, 7 from proactive audit.**

By severity of unfixed:
- **HIGH (4):** F1 — hand-rolled JSON parser, F3 — orphaned CAS blobs, F6 — setup destroys profiles, F7 — fake health check
- **MEDIUM (2):** F2 — doctor false negative, F4 — lost upstream URL on no-backup disconnect
- **LOW (1):** F5 — empty env object left behind
