const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(path.join(__dirname, "../src/pages/system/ddns/ipv6.ts"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports, URL });
const { normalizeIpv6, isValidManualIpv6 } = output.exports;

test("normalizes valid IPv6 text and rejects everything else", () => {
  assert.equal(normalizeIpv6(" 2409:8A1E:0:0::0001 "), "2409:8a1e::1");
  assert.equal(normalizeIpv6("1.2.3.4"), null);
  assert.equal(normalizeIpv6("2409::1::2"), null);
  assert.equal(normalizeIpv6("[2409::1]"), null);
  assert.equal(normalizeIpv6("fe80::1%en0"), null);
  assert.equal(normalizeIpv6(""), null);
});

test("manual addresses follow the backend rules", () => {
  assert.equal(isValidManualIpv6("2409:8a1e:1234::1"), true);
  assert.equal(isValidManualIpv6("fd12:3456::1"), true);
  for (const value of ["::", "::1", "fe80::1", "febf::1", "ff02::1", "::ffff:1.2.3.4", "abc"]) {
    assert.equal(isValidManualIpv6(value), false, value);
  }
});
