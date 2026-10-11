const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(
  path.join(__dirname, "../src/pages/system/logs/auditRules.ts"),
  "utf8",
);
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports, Object, Set });
const audit = output.exports;
const plain = (value) => JSON.parse(JSON.stringify(value));

const defaults = { enabled: true, log_reads: false, log_body: true };
const feature = (key, label, group, prefixes, rule = defaults) => ({
  key,
  label,
  group,
  prefixes,
  ...rule,
  defaults,
});
const settings = {
  enabled: true,
  features: [
    feature("users", "用户管理", "权限管理", ["/api/admin/users"]),
    feature("roles", "角色管理", "权限管理", ["/api/admin/roles"], {
      enabled: false,
      log_reads: false,
      log_body: true,
    }),
    feature("ssh", "SSH 管理", "运维工具", ["/api/admin/ssh"]),
  ],
};

test("drafts mirror the saved settings and detect changes", () => {
  const saved = audit.draftFrom(settings);
  assert.equal(saved.rules.roles.enabled, false);
  const changed = audit.setRule(saved, "users", { log_reads: true });
  assert.equal(audit.sameDraft(saved, saved), true);
  assert.equal(audit.sameDraft(saved, changed), false);
  assert.equal(saved.rules.users.log_reads, false, "drafts are not mutated");
  assert.equal(audit.countRecording(saved), 2);
});

test("bulk changes only touch the given features", () => {
  const saved = audit.draftFrom(settings);
  const off = audit.setAll(saved, { enabled: false }, ["users"]);
  assert.equal(off.rules.users.enabled, false);
  assert.equal(off.rules.ssh.enabled, true);
  const all = audit.setAll(saved, { enabled: true });
  assert.equal(audit.countRecording(all), 3);
  const reset = audit.resetToDefaults({ ...off, enabled: false }, settings.features);
  assert.equal(reset.enabled, true);
  assert.deepEqual(plain(reset.rules.roles), defaults);
});

test("features are grouped and searchable by name or path", () => {
  assert.deepEqual(
    plain(audit.groupFeatures(settings.features).map((group) => group.group)),
    ["权限管理", "运维工具"],
  );
  const found = audit.groupFeatures(settings.features, "ROLES");
  assert.deepEqual(plain(found.map((group) => group.features.map((f) => f.key))), [["roles"]]);
  assert.equal(audit.groupFeatures(settings.features, "ssh 管理").length, 1);
  assert.equal(audit.groupFeatures(settings.features, "nothing").length, 0);
});

test("the payload lists every feature rule", () => {
  const payload = audit.toPayload(audit.draftFrom(settings));
  assert.equal(payload.enabled, true);
  assert.deepEqual(plain(payload.features[1]), {
    key: "roles",
    enabled: false,
    log_reads: false,
    log_body: true,
  });
});
