const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(path.join(__dirname, "../src/layouts/dock/dockConfig.ts"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports });
const dock = output.exports;
const plain = (value) => JSON.parse(JSON.stringify(value));

test("missing or broken data falls back to the default dock", () => {
  for (const raw of [null, undefined, "x", [1], 42]) {
    assert.deepEqual(plain(dock.normalizeDockConfig(raw)), plain(dock.DEFAULT_DOCK_CONFIG));
  }
});

test("stored values are cleaned up", () => {
  const config = dock.normalizeDockConfig({
    enabled: false,
    position: "top",
    size: "huge",
    magnify: "yes",
    autoHide: true,
    items: [
      { path: "/admin/system/users", color: "#ABCDEF" },
      { path: "/admin/system/users", color: "#000000" },
      { path: "https://evil.example", color: "#111111" },
      { path: "/admin/system/logs", color: "red" },
      "junk",
    ],
  });
  assert.equal(config.enabled, false);
  assert.equal(config.position, "bottom", "unknown position → bottom");
  assert.equal(config.size, "medium", "unknown size → medium");
  assert.equal(config.magnify, true, "non-boolean → default");
  assert.equal(config.autoHide, true);
  assert.deepEqual(
    Array.from(config.items, (item) => item.path),
    ["/admin/system/users", "/admin/system/logs"],
    "duplicates and non-admin paths are dropped",
  );
  assert.equal(config.items[0].color, "#abcdef");
  assert.equal(config.items[1].color, dock.colorForPath("/admin/system/logs"), "invalid color → stable default");
});

test("item count is capped", () => {
  const items = Array.from({ length: 30 }, (_, index) => ({ path: `/admin/page-${index}` }));
  assert.equal(dock.normalizeDockConfig({ items }).items.length, dock.MAX_DOCK_ITEMS);
});

test("reordering moves one item and ignores bad indexes", () => {
  const items = ["a", "b", "c"].map((path) => ({ path, color: "#000000" }));
  assert.deepEqual(Array.from(dock.moveDockItem(items, 0, 2), (item) => item.path), ["b", "c", "a"]);
  assert.equal(dock.moveDockItem(items, 1, 5), items);
  assert.equal(dock.colorForPath("/admin/x"), dock.colorForPath("/admin/x"));
});

test("page rules override the default per page", () => {
  const config = dock.normalizeDockConfig({
    autoHide: false,
    pageRules: [
      { path: "/admin/system/ssh", mode: "autoHide" },
      { path: "/admin/system/vnc", mode: "hide" },
      { path: "/admin/system/ssh", mode: "show" },
      { path: "/admin/system/logs", mode: "sometimes" },
      { path: "javascript:alert(1)", mode: "hide" },
    ],
  });
  assert.deepEqual(
    plain(config.pageRules),
    [
      { path: "/admin/system/ssh", mode: "autoHide" },
      { path: "/admin/system/vnc", mode: "hide" },
    ],
    "duplicates, unknown modes and foreign paths are dropped",
  );
  assert.equal(dock.dockModeForPath(config, "/admin/system/ssh"), "autoHide");
  assert.equal(dock.dockModeForPath(config, "/admin/system/vnc"), "hide");
  assert.equal(dock.dockModeForPath(config, "/admin/dashboard"), "show");
  assert.equal(
    dock.dockModeForPath({ ...config, autoHide: true }, "/admin/dashboard"),
    "autoHide",
    "pages without a rule follow the default",
  );
});

test("rules can be set, replaced and removed", () => {
  let config = dock.normalizeDockConfig({});
  config = dock.withPageRule(config, "/admin/system/ssh", { mode: "hide" });
  config = dock.withPageRule(config, "/admin/system/ssh", { mode: "show" });
  assert.deepEqual(plain(config.pageRules), [{ path: "/admin/system/ssh", mode: "show" }]);
  config = dock.withPageRule(config, "/admin/system/ssh", { reserve: false });
  assert.deepEqual(
    plain(config.pageRules),
    [{ path: "/admin/system/ssh", mode: "show", reserve: false }],
    "patching one field keeps the other",
  );
  config = dock.withPageRule(config, "/admin/system/ssh", { mode: null });
  assert.deepEqual(plain(config.pageRules), [{ path: "/admin/system/ssh", reserve: false }]);
  config = dock.withPageRule(config, "/admin/system/ssh", { reserve: null });
  assert.deepEqual(plain(config.pageRules), [], "a rule with nothing left is dropped");
  config = dock.withPageRule(config, "/admin/system/vnc", { mode: "hide", reserve: true });
  config = dock.withPageRule(config, "/admin/system/vnc", null);
  assert.deepEqual(plain(config.pageRules), []);
  const many = Array.from({ length: 80 }, (_, i) => ({ path: `/admin/p${i}`, mode: "hide" }));
  assert.equal(dock.normalizeDockConfig({ pageRules: many }).pageRules.length, dock.MAX_DOCK_PAGE_RULES);
});

test("reserve space follows the default, page rules and display mode", () => {
  const config = dock.normalizeDockConfig({
    pageRules: [
      { path: "/admin/system/ssh", reserve: false },
      { path: "/admin/system/vnc", mode: "autoHide", reserve: true },
      { path: "/admin/system/logs", reserve: "yes" },
      { path: "/admin/home", mode: "show", reserve: true },
    ],
  });
  assert.equal(config.reserveSpace, true, "reserving is on by default");
  assert.deepEqual(
    plain(config.pageRules),
    [
      { path: "/admin/system/ssh", reserve: false },
      { path: "/admin/system/vnc", mode: "autoHide", reserve: true },
      { path: "/admin/home", mode: "show", reserve: true },
    ],
    "rules with neither a valid mode nor a boolean reserve are dropped",
  );
  assert.equal(dock.dockReservesSpaceForPath(config, "/admin/system/ssh"), false);
  assert.equal(dock.dockReservesSpaceForPath(config, "/admin/dashboard"), true);
  assert.equal(
    dock.dockReservesSpaceForPath(config, "/admin/system/vnc"),
    false,
    "an auto-hidden dock never reserves space",
  );
  const off = { ...config, reserveSpace: false };
  assert.equal(dock.dockReservesSpaceForPath(off, "/admin/dashboard"), false);
  assert.equal(dock.dockReservesSpaceForPath(off, "/admin/home"), true);
  assert.equal(
    dock.normalizeDockConfig({ reserveSpace: false }).reserveSpace,
    false,
  );
});
