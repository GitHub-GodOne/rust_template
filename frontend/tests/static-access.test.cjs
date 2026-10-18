const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");
const { i18n, i18nRequire } = require("./support/i18n-stub.cjs");

const source = fs.readFileSync(
  path.join(__dirname, "../src/pages/system/static-hosting/staticAccess.ts"),
  "utf8",
);
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { require: i18nRequire, exports: output.exports });
const access = output.exports;

const rules = [
  { path: "", username: "site" },
  { path: "docs/private", username: "owner" },
];

test("the most specific rule wins, case-insensitively", () => {
  assert.equal(access.ruleForPath(rules, "index.html").username, "site");
  assert.equal(access.ruleForPath(rules, "Docs/PRIVATE/a.pdf").username, "owner");
  assert.equal(access.ruleForPath(rules, "docs/private-notes").username, "site");
  assert.equal(access.ruleForPath([{ path: "docs" }], "readme.md"), undefined);
});

test("locks are shown only where a new protected area starts", () => {
  assert.equal(access.entersProtectedArea(rules, "docs", "docs/private"), true);
  assert.equal(access.entersProtectedArea(rules, "docs/private", "docs/private/x"), false);
  assert.equal(access.entersProtectedArea([{ path: "a" }], "", "b"), false);
  assert.equal(access.entersProtectedArea([{ path: "a" }], "", "a"), true);
});

test("rules are found by exact directory and labelled", () => {
  assert.equal(access.hasRuleAt(rules, "/docs/private/"), true);
  assert.equal(access.hasRuleAt(rules, "docs"), false);
  assert.equal(access.accessRuleLabel(""), "整个站点");
  assert.equal(access.accessRuleLabel("docs/private"), "/docs/private");
});
