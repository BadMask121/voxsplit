import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import { fillScript, megabytes, pkgPath, publicOrigin, rangeOf } from "../src/worker.js";

const release = { version: "1.2.3", file: "VoxSplit-1.2.3.pkg", sha256: "ab".repeat(32), bytes: 320_955_082, date: "2026-10-28" };

test("install script gets the release filled in", async () => {
  const template = await readFile(new URL("../public/install.template.sh", import.meta.url), "utf8");
  const script = fillScript(template, "https://voxsplit.triplejmusic.art", release);
  assert.ok(!script.includes("__"), "no placeholders left");
  assert.match(script, /local version="1\.2\.3"/);
  assert.match(script, /local url="https:\/\/voxsplit\.triplejmusic\.art\/download\/VoxSplit-1\.2\.3\.pkg"/);
  assert.match(script, new RegExp(`local sha256="${"ab".repeat(32)}"`));
  assert.match(script, /local size="321 MB"/);
});

test("download paths and sizes", () => {
  assert.equal(pkgPath(release), "/download/VoxSplit-1.2.3.pkg");
  assert.equal(megabytes(320_955_082), "321 MB");
});

test("R2 ranges normalise to offset and length", () => {
  assert.deepEqual(rangeOf({ offset: 100, length: 50 }, 1000), { offset: 100, length: 50 });
  assert.deepEqual(rangeOf({ offset: 900 }, 1000), { offset: 900, length: 100 });
  assert.deepEqual(rangeOf({ suffix: 200 }, 1000), { offset: 800, length: 200 });
  assert.deepEqual(rangeOf({ suffix: 5000 }, 1000), { offset: 0, length: 1000 });
});

test("download links are https outside local dev", () => {
  assert.equal(publicOrigin(new URL("http://voxsplit.triplejmusic.art/install.sh")), "https://voxsplit.triplejmusic.art");
  assert.equal(publicOrigin(new URL("http://localhost:8799/install.sh")), "http://localhost:8799");
});

