const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(path.join(__dirname, "../src/layouts/menuTrail.ts"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports });
const trail = output.exports;
const labels = (items) => JSON.parse(JSON.stringify(items.map(trail.menuLabel)));

const menus = [
  { key: "home", label: "首页", path: "/admin/dashboard" },
  {
    key: "system",
    label: "系统管理",
    children: [
      { key: "uploads", label: "素材库", path: "/admin/system/uploads" },
      { key: "ops", title: "运维", children: [{ key: "ssh", label: "SSH 管理", path: "/admin/system/ssh" }] },
    ],
  },
];

test("finds the chain from the top menu down to the page", () => {
  assert.deepEqual(labels(trail.findMenuTrail("/admin/system/uploads", menus)), ["系统管理", "素材库"]);
  assert.deepEqual(labels(trail.findMenuTrail("/admin/system/ssh", menus)), ["系统管理", "运维", "SSH 管理"]);
  assert.deepEqual(labels(trail.findMenuTrail("/admin/dashboard", menus)), ["首页"]);
});

test("unknown pages have no trail", () => {
  assert.equal(trail.findMenuTrail("/admin/nowhere", menus).length, 0);
  assert.equal(trail.findMenuTrail("/admin/system", menus).length, 0);
});
