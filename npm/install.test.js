"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const test = require("node:test");
const { extract, parseChecksum, releaseAsset } = require("./install");

test("maps every published platform to its release archive", () => {
  const { version } = require("../package.json");
  assert.equal(releaseAsset("linux", "x64").archive, `toche-${version}-x86_64-unknown-linux-gnu.tar.gz`);
  assert.equal(releaseAsset("win32", "x64").archive, `toche-${version}-x86_64-pc-windows-msvc.zip`);
  assert.equal(releaseAsset("darwin", "x64").archive, `toche-${version}-x86_64-apple-darwin.tar.gz`);
  assert.equal(releaseAsset("darwin", "arm64").archive, `toche-${version}-aarch64-apple-darwin.tar.gz`);
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

test("extracts a Windows zip when paths contain spaces", { skip: process.platform !== "win32" }, () => {
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "toche npm extraction "));
  const source = path.join(temporary, "source folder");
  const payload = path.join(source, "payload");
  const archive = path.join(temporary, "archive with spaces.zip");
  const destination = path.join(temporary, "destination folder");
  const powershell = process.env.SystemRoot
    ? path.join(process.env.SystemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe")
    : "powershell.exe";

  try {
    fs.mkdirSync(payload, { recursive: true });
    fs.writeFileSync(path.join(payload, "toche.exe"), "windows-test-binary");

    const compressed = spawnSync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Compress-Archive -LiteralPath $env:TOCHE_TEST_SOURCE " +
          "-DestinationPath $env:TOCHE_TEST_ARCHIVE -Force"
      ],
      {
        stdio: "inherit",
        env: {
          ...process.env,
          TOCHE_TEST_SOURCE: payload,
          TOCHE_TEST_ARCHIVE: archive
        }
      }
    );
    assert.equal(compressed.status, 0, "test zip creation should succeed");

    extract(archive, destination, "zip");
    assert.equal(
      fs.readFileSync(path.join(destination, "payload", "toche.exe"), "utf8"),
      "windows-test-binary"
    );
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
});
