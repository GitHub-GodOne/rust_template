const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(path.join(__dirname, "../src/pages/system/ssh/sshSession.ts"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports });
const ssh = output.exports;

test("recognizes a session the server no longer has", () => {
  assert.equal(ssh.isSessionGone({ code: "SSH_SESSION_NOT_FOUND", status: 404 }), true);
  assert.equal(ssh.isSessionGone({ status: 404 }), true);
  assert.equal(ssh.isSessionGone(new Error("ssh session not found")), true);
  assert.equal(ssh.isSessionGone({ status: 400, message: "ssh target not found" }), false);
  assert.equal(ssh.isSessionGone(new Error("无法连接服务器")), false);
  assert.equal(ssh.isSessionGone(null), false);
});

test("reconnect backoff grows and is capped", () => {
  assert.deepEqual([0, 1, 2, 3, 10].map(ssh.reconnectDelay), [1000, 2000, 4000, 8000, 10000]);
});

test("overlay explains every non-connected state", () => {
  assert.equal(ssh.statusOverlay("connected"), null);
  assert.equal(ssh.statusOverlay("idle").action, "connect");
  assert.equal(ssh.statusOverlay("detached").action, "reconnect");
  assert.equal(ssh.statusOverlay("closed").action, "new");
  assert.equal(ssh.statusOverlay("connecting").action, null);
  assert.match(ssh.statusOverlay("connecting", { reconnectAttempt: 2 }).title, /第 2 次/);
  assert.equal(ssh.statusOverlay("error", { errorMessage: "认证失败" }).description, "认证失败");
});

test("stale server session ids are forgotten unless a connection is live", () => {
  const sessions = [
    { id: "a", serverSessionId: "gone", status: "detached" },
    { id: "b", serverSessionId: "kept", status: "detached" },
    { id: "c", serverSessionId: "gone-but-live", status: "connected" },
    { id: "d", serverSessionId: "pending", status: "connecting" },
    { id: "e", status: "idle" },
  ];
  const next = ssh.forgetMissingServerSessions(
    sessions,
    new Set(["kept"]),
    (session) => session.id === "c",
  );
  assert.equal(next[0].serverSessionId, undefined);
  assert.equal(next[0].status, "idle");
  assert.equal(next[1].serverSessionId, "kept");
  assert.equal(next[2].serverSessionId, "gone-but-live");
  assert.equal(next[3].serverSessionId, "pending");
  assert.equal(next[4], sessions[4]);
  const unchanged = ssh.forgetMissingServerSessions(sessions.slice(1, 2), new Set(["kept"]), () => false);
  assert.equal(unchanged.length, 1);
});
