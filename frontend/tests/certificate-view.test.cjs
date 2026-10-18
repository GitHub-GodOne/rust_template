const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(
  path.join(__dirname, "../src/pages/system/certificates/certificateView.ts"),
  "utf8",
);
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports });
const view = output.exports;
const plain = (value) => JSON.parse(JSON.stringify(value));

test("public and local URLs are previewed like the server builds them", () => {
  assert.equal(view.previewUrl("App.Example.com", "direct", 443), "https://app.example.com");
  assert.equal(view.previewUrl("app.example.com", "direct", 8443), "https://app.example.com:8443");
  assert.equal(view.previewUrl("app.example.com", "tunnel", 8443), "https://app.example.com");
  assert.equal(view.previewUrl("", "direct", 443), "https://example.com");
  assert.equal(view.upstreamLabel("::1", 3000), "http://[::1]:3000");
  assert.equal(view.upstreamLabel("", 8080), "http://127.0.0.1:8080");
});

test("tones follow status and expiry", () => {
  assert.equal(view.expiryTone(60), "success");
  assert.equal(view.expiryTone(7), "warning");
  assert.equal(view.expiryTone(-1), "error");
  assert.equal(view.expiryTone(null), "default");
  assert.equal(view.certificateTone("pending", true), "processing");
  assert.equal(view.certificateTone("failed"), "error");
  assert.equal(view.mappingTone("pending"), "warning");
  assert.equal(view.isPrivilegedPort(443), true);
  assert.equal(view.isPrivilegedPort(8443), false);
});

test("domain lists are split and de-duplicated", () => {
  assert.deepEqual(plain(view.parseDomainList(" *.Example.com, example.com，\nexample.com. ")), [
    "*.example.com",
    "example.com",
  ]);
});
