# Toche 1.1.0 — Distribution Acceptance Report

**Author:** Astra (Release Director)
**Date:** 2026-07-20
**Classification:** Release Completion — Phase 1–2 Diagnosis and Upload
**Status:** ⚠️ PARTIALLY COMPLETE — Windows binary uploaded, npm verification pending

---

## Phase 1 — Packaging Failure Diagnosis

### Finding

**Path A: Missing assets only.** The npm installer (`npm/install.js`) is correct. The issue is that the GitHub Release `v1.1.0` was created with zero binary assets.

### Evidence

| Component | Status |
|-----------|--------|
| `npm/install.js` | ✅ Correct — targets, platform mapping, checksum verification all valid |
| `releaseAsset('win32','x64')` | ✅ Resolves to `toche-1.1.0-x86_64-pc-windows-msvc.zip` |
| Release URL | ✅ `https://github.com/nzkbuild/toche/releases/download/v1.1.0/toche-1.1.0-x86_64-pc-windows-msvc.zip` |
| GitHub Release `v1.1.0` on creation | ❌ 0 assets — `"assets":[]` |
| GitHub Release after fix | ✅ 2 assets uploaded |
| Source code at tag `v1.1.0` | ✅ Clean — no modifications needed |
| Installer logic | ✅ No changes required |

### Root Cause

The GitHub Release was created via `gh release create` without building and uploading platform binaries. The release was created as a metadata-only release (tag + notes + source archives). No CI/CD pipeline was triggered to build and attach platform-specific assets.

---

## Phase 2 — Path A: Upload Missing Assets

### Verification Gates Before Upload

| Gate | Result |
|------|--------|
| HEAD matches tag `v1.1.0` | ✅ `05da236` |
| Working tree clean | ✅ `git status --short` empty |
| Binary built from exact tagged commit | ✅ `toche 1.1.0` |
| `toche --version` | ✅ `toche 1.1.0` |
| `toche --help` | ✅ All 10 subcommands |

### Uploaded Assets

| Asset | SHA-256 |
|-------|---------|
| `toche-1.1.0-x86_64-pc-windows-msvc.zip` | `df26e56450e27e267e13c8c61fea3918e79a57962e02342cdc427fd8f85a5485` |
| `toche-1.1.0-x86_64-pc-windows-msvc.zip.sha256` | Uploaded |

### Verification

| URL | Status |
|-----|--------|
| `https://github.com/nzkbuild/toche/releases/download/v1.1.0/toche-1.1.0-x86_64-pc-windows-msvc.zip` | ✅ HTTP 200 |
| GitHub Release assets | ✅ 2 assets confirmed via `gh release view` |

---

## Phase 3 — Publish Verification (PENDING)

### Blocked

NPM publication requires authentication. Current state:

| Check | Status |
|-------|--------|
| `npm whoami` | ❌ `401 Unauthorized` — no active token for `registry.npmjs.org` |
| `~/.npmrc` | ⚠️ Token exists but for a different registry host, not npmjs.org |

### Required

1. Generate an npm token for the `@nzkbuild` scope on npmjs.org
2. Run `npm login` or set the token
3. `npm whoami` → `nzkbuild`
4. `npm publish --access public` from the tagged source
5. Fresh installation test from public registry

---

## Remaining Platform Binaries (Not Yet Built)

| Platform | Triple | Status |
|----------|--------|--------|
| Windows x64 | `x86_64-pc-windows-msvc` | ✅ Uploaded |
| macOS ARM | `aarch64-apple-darwin` | ❌ Not built (no macOS runner) |
| macOS x64 | `x86_64-apple-darwin` | ❌ Not built (no macOS runner) |
| Linux x64 | `x86_64-unknown-linux-gnu` | ❌ Not built (Windows host, can cross-compile) |

### Recommendation

For Linux x64: cross-compile from Windows (`rustup target add x86_64-unknown-linux-gnu`) or build on the VPS. For macOS: requires a macOS runner or CI/CD pipeline. The npm installer will fail on macOS and Linux until their binaries are uploaded.

---

## Recommendation

**READY TO PUBLISH 1.1.0 — Windows only, with npm auth required.**

For macOS and Linux support: either build cross-platform binaries or document that the npm package currently supports Windows only. A full cross-platform release requires CI/CD or manual builds on each platform.

---

## Next Steps

1. **You:** Generate npm token for `@nzkbuild` scope, provide it or run `npm login`
2. **Me:** Publish, verify `npm view @nzkbuild/toche version` → `1.1.0`
3. **Me:** Fresh `npm install -g @nzkbuild/toche` in sandbox, verify `toche --version`
4. **Decision:** Ship Windows-only now, or build Linux/macOS binaries first
