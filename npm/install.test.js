"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const { parseChecksum, releaseAsset } = require("./install");

test("maps every published platform to its release archive", () => {
  assert.equal(releaseAsset("linux", "x64").archive, "toche-1.0.8-x86_64-unknown-linux-gnu.tar.gz");
  assert.equal(releaseAsset("win32", "x64").archive, "toche-1.0.8-x86_64-pc-windows-msvc.zip");
  assert.equal(releaseAsset("darwin", "x64").archive, "toche-1.0.8-x86_64-apple-darwin.tar.gz");
  assert.equal(releaseAsset("darwin", "arm64").archive, "toche-1.0.8-aarch64-apple-darwin.tar.gz");
});

test("rejects a platform without a published binary", () => {
  assert.throws(() => releaseAsset("linux", "arm64"), /Unsupported platform/);
});

test("parses sha256sum output", () => {
  const hash = "a".repeat(64);
  assert.equal(parseChecksum(`${hash}  toche.tar.gz\n`), hash);
});

test("rejects malformed checksums", () => {
  assert.throws(() => parseChecksum("not-a-checksum"), /malformed/);
});
