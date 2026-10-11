const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(path.join(__dirname, "../src/components/admin/finder/finderModel.ts"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports });
const finder = output.exports;
const plain = (value) => JSON.parse(JSON.stringify(value));

test("file kinds come from mime type, then extension", () => {
  assert.equal(finder.finderKindOf({ isFolder: true, name: "a.png" }), "folder");
  assert.equal(finder.finderKindOf({ name: "photo.JPG" }), "image");
  assert.equal(finder.finderKindOf({ name: "x", mimeType: "video/mp4" }), "video");
  assert.equal(finder.finderKindOf({ name: "report.pdf" }), "pdf");
  assert.equal(finder.finderKindOf({ name: "site.tar.gz" }), "archive");
  assert.equal(finder.finderKindOf({ name: "main.rs" }), "code");
  assert.equal(finder.finderKindOf({ name: "notes.md" }), "text");
  assert.equal(finder.finderKindOf({ name: "Makefile" }), "file");
});

test("sizes and dates read like Finder", () => {
  assert.equal(finder.formatBytes(512), "512 字节");
  assert.equal(finder.formatBytes(24_000), "24.0 KB");
  assert.equal(finder.formatBytes(1_500_000_000), "1.5 GB");
  assert.equal(finder.formatBytes(null), "--");
  const now = new Date(2026, 9, 11, 15, 0);
  assert.equal(finder.formatModified(new Date(2026, 9, 11, 9, 5).toISOString(), now), "今天 09:05");
  assert.equal(finder.formatModified(new Date(2026, 9, 10, 23, 59).toISOString(), now), "昨天 23:59");
  assert.equal(finder.formatModified(new Date(2026, 8, 1, 8, 30).toISOString(), now), "2026/09/01 08:30");
  assert.equal(finder.formatModified(undefined, now), "--");
});

test("paths split into crumbs and walk up to the root", () => {
  assert.deepEqual(plain(finder.pathCrumbs("/a/b/c/")), [
    { name: "a", path: "a" },
    { name: "b", path: "a/b" },
    { name: "c", path: "a/b/c" },
  ]);
  assert.deepEqual(plain(finder.pathCrumbs("")), []);
  assert.equal(finder.parentPathOf("a/b/c"), "a/b");
  assert.equal(finder.parentPathOf("a/"), "");
  assert.equal(finder.parentPathOf(""), null);
});

test("folders sort first and names compare naturally", () => {
  const items = [
    { key: "1", name: "file10.txt", kind: "text", sizeBytes: 10 },
    { key: "2", name: "file2.txt", kind: "text", sizeBytes: 300 },
    { key: "3", name: "zeta", kind: "folder" },
    { key: "4", name: "alpha", kind: "folder" },
  ];
  const byName = finder.sortFinderItems(items, { key: "name", order: "asc" });
  assert.deepEqual(Array.from(byName, (item) => item.name), ["alpha", "zeta", "file2.txt", "file10.txt"]);
  const bySizeDesc = finder.sortFinderItems(items, { key: "size", order: "desc" });
  assert.deepEqual(Array.from(bySizeDesc, (item) => item.name), ["zeta", "alpha", "file2.txt", "file10.txt"]);
});

test("history behaves like a browser", () => {
  let history = { entries: ["root"], index: 0 };
  history = finder.historyVisit(history, "a");
  history = finder.historyVisit(history, "a/b");
  assert.equal(finder.historyVisit(history, "a/b"), history, "same location is not duplicated");
  history = finder.historyMove(history, -1);
  assert.equal(history.entries[history.index], "a");
  history = finder.historyVisit(history, "c");
  assert.deepEqual(Array.from(history.entries), ["root", "a", "c"], "visiting drops forward entries");
  assert.equal(finder.historyMove(history, 1), history, "cannot go past the end");
});
