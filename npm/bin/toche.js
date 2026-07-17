#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawn } = require("node:child_process");

const executable = process.platform === "win32" ? "toche-bin.exe" : "toche-bin";
const binary = path.join(__dirname, executable);

if (!fs.existsSync(binary)) {
  console.error("Toche's native binary is missing.");
  console.error("Reinstall without --ignore-scripts: npm install -g toche");
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), {
  argv0: "toche",
  stdio: "inherit"
});

child.on("error", (error) => {
  console.error(`Could not start Toche: ${error.message}`);
  process.exit(1);
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code === null ? 1 : code);
});
