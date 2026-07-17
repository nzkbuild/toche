"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const packageJson = require("../package.json");
const binDir = path.join(__dirname, "bin");

const targets = {
  "darwin-arm64": { triple: "aarch64-apple-darwin", format: "tar.gz" },
  "darwin-x64": { triple: "x86_64-apple-darwin", format: "tar.gz" },
  "linux-x64": { triple: "x86_64-unknown-linux-gnu", format: "tar.gz" },
  "win32-x64": { triple: "x86_64-pc-windows-msvc", format: "zip" }
};

function releaseAsset(platform = process.platform, arch = process.arch) {
  const key = `${platform}-${arch}`;
  const target = targets[key];
  if (!target) {
    const supported = Object.keys(targets).join(", ");
    throw new Error(`Unsupported platform ${key}. Supported platforms: ${supported}.`);
  }

  const stem = `toche-${packageJson.version}-${target.triple}`;
  return { ...target, archive: `${stem}.${target.format}`, folder: stem };
}

function parseChecksum(text) {
  const match = text.trim().match(/^([a-fA-F0-9]{64})\s+/);
  if (!match) throw new Error("Release checksum file is malformed.");
  return match[1].toLowerCase();
}

function sha256(file) {
  const hash = crypto.createHash("sha256");
  hash.update(fs.readFileSync(file));
  return hash.digest("hex");
}

function download(url, destination, redirects = 0) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { "user-agent": "toche-npm-installer" } }, (response) => {
        if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
          response.resume();
          if (redirects >= 5) {
            reject(new Error(`Too many redirects while downloading ${url}.`));
            return;
          }
          const next = new URL(response.headers.location, url).toString();
          download(next, destination, redirects + 1).then(resolve, reject);
          return;
        }

        if (response.statusCode !== 200) {
          response.resume();
          reject(new Error(`Download failed with HTTP ${response.statusCode}: ${url}`));
          return;
        }

        const file = fs.createWriteStream(destination, { mode: 0o600 });
        response.pipe(file);
        file.on("finish", () => file.close(resolve));
        file.on("error", reject);
      })
      .on("error", reject);
  });
}

function extract(archive, destination, format) {
  fs.mkdirSync(destination, { recursive: true });
  if (format === "tar.gz") {
    const result = spawnSync("tar", ["-xzf", archive, "-C", destination], { stdio: "inherit" });
    if (result.status !== 0) throw new Error("Could not extract the Toche tar archive.");
    return;
  }

  const powershell = process.env.SystemRoot
    ? path.join(process.env.SystemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe")
    : "powershell.exe";
  const command =
    "Expand-Archive -LiteralPath $env:TOCHE_ARCHIVE_PATH " +
    "-DestinationPath $env:TOCHE_EXTRACT_DESTINATION -Force";
  const result = spawnSync(
    powershell,
    ["-NoProfile", "-NonInteractive", "-Command", command],
    {
      stdio: "inherit",
      env: {
        ...process.env,
        TOCHE_ARCHIVE_PATH: archive,
        TOCHE_EXTRACT_DESTINATION: destination
      }
    }
  );
  if (result.status !== 0) throw new Error("Could not extract the Toche zip archive.");
}

async function install() {
  const executable = process.platform === "win32" ? "toche-bin.exe" : "toche-bin";
  const destination = path.join(binDir, executable);
  fs.mkdirSync(binDir, { recursive: true });

  if (process.env.TOCHE_BINARY_PATH) {
    fs.copyFileSync(path.resolve(process.env.TOCHE_BINARY_PATH), destination);
    if (process.platform !== "win32") fs.chmodSync(destination, 0o755);
    console.log(`Toche ${packageJson.version} installed from TOCHE_BINARY_PATH.`);
    return;
  }

  const asset = releaseAsset();
  const tag = `v${packageJson.version}`;
  const base = process.env.TOCHE_DOWNLOAD_BASE ||
    `https://github.com/nzkbuild/toche/releases/download/${tag}`;
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "toche-install-"));

  try {
    const archive = path.join(temporary, asset.archive);
    const checksumFile = `${archive}.sha256`;
    console.log(`Downloading Toche ${packageJson.version} for ${process.platform}-${process.arch}...`);
    await download(`${base}/${asset.archive}`, archive);
    await download(`${base}/${asset.archive}.sha256`, checksumFile);

    const expected = parseChecksum(fs.readFileSync(checksumFile, "utf8"));
    const actual = sha256(archive);
    if (actual !== expected) throw new Error(`Checksum mismatch for ${asset.archive}.`);

    const extracted = path.join(temporary, "extracted");
    extract(archive, extracted, asset.format);
    const binaryName = process.platform === "win32" ? "toche.exe" : "toche";
    const source = path.join(extracted, asset.folder, binaryName);
    if (!fs.existsSync(source)) {
      throw new Error(`Release archive does not contain ${asset.folder}/${binaryName}.`);
    }

    fs.copyFileSync(source, destination);
    if (process.platform !== "win32") fs.chmodSync(destination, 0o755);
    console.log(`Toche ${packageJson.version} installed. Run: toche setup`);
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

if (require.main === module) {
  install().catch((error) => {
    console.error(`Toche installation failed: ${error.message}`);
    console.error("See https://github.com/nzkbuild/toche#install");
    process.exit(1);
  });
}

module.exports = { extract, install, parseChecksum, releaseAsset };
