const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");
const axios = require("axios");

function makeClient(adapter) {
  let signOuts = 0;
  const state = {
    accessToken: "old-access",
    refreshToken: "old-refresh",
    signOut() {
      signOuts += 1;
      state.accessToken = null;
      state.refreshToken = null;
    },
    setTokens({ token, refreshToken }) {
      state.accessToken = token;
      state.refreshToken = refreshToken;
    },
  };
  const source = fs.readFileSync(path.join(__dirname, "../src/api/client.ts"), "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
  }).outputText;
  const output = { exports: {} };
  const axiosModule = axios.create({ adapter });
  // Keep Axios' real interceptors and request handling, with an in-memory transport.
  axiosModule.create = (options) => axios.create({ ...options, adapter });
  axiosModule.isAxiosError = axios.isAxiosError;
  axiosModule.isCancel = axios.isCancel;
  axiosModule.default = axiosModule;
  vm.runInNewContext(compiled, {
    exports: output.exports,
    require(name) {
      if (name === "axios") return axiosModule;
      if (name === "../stores/auth") return { useAuthStore: { getState: () => state } };
      throw new Error(`Unexpected import: ${name}`);
    },
  });
  return { client: output.exports.apiClient, state, signOuts: () => signOuts };
}

function fail(config, status, code = "ERR_BAD_RESPONSE") {
  const response = status ? { status, data: { message: "temporary failure" }, config } : undefined;
  throw new axios.AxiosError("failure", code, config, undefined, response);
}

for (const [name, status, code] of [
  ["refresh server error", 500, "ERR_BAD_RESPONSE"],
  ["refresh timeout", undefined, "ECONNABORTED"],
  ["refresh network failure", undefined, "ERR_NETWORK"],
]) {
  test(`${name} preserves tokens and reports the refresh failure`, async () => {
    const session = makeClient(async (config) => {
      if (config.url === "/api/auth/refresh") return fail(config, status, code);
      return fail(config, 401);
    });
    await assert.rejects(session.client.get("/auth/current"), (error) => {
      assert.equal(error.status, status);
      if (code === "ECONNABORTED") assert.match(error.message, /超时/);
      return true;
    });
    assert.equal(session.signOuts(), 0);
    assert.equal(session.state.refreshToken, "old-refresh");
  });
}

test("invalid refresh credentials clear the session", async () => {
  const session = makeClient(async (config) => fail(config, 401));
  await assert.rejects(session.client.get("/auth/current"));
  assert.equal(session.signOuts(), 1);
  assert.equal(session.state.refreshToken, null);
});

test("successful refresh retries with the new access token", async () => {
  let refreshes = 0;
  const session = makeClient(async (config) => {
    if (config.url === "/api/auth/refresh") {
      refreshes += 1;
      return { status: 200, data: { data: { token: "new-access", refresh_token: "new-refresh" } }, config };
    }
    if (config.headers.Authorization !== "Bearer new-access") return fail(config, 401);
    return { status: 200, data: { ok: true }, config };
  });
  const response = await session.client.get("/auth/current");
  assert.equal(response.status, 200);
  assert.equal(refreshes, 1);
  assert.equal(session.state.refreshToken, "new-refresh");
  assert.equal(session.signOuts(), 0);
});

test("late 401 from an old session cannot clear a newer login", async () => {
  const session = makeClient(async (config) => {
    session.state.accessToken = "new-login";
    session.state.refreshToken = "new-login-refresh";
    return fail(config, 401);
  });
  await assert.rejects(session.client.get("/auth/current"));
  assert.equal(session.signOuts(), 0);
  assert.equal(session.state.refreshToken, "new-login-refresh");
});

test("incorrect login does not refresh or clear an existing session", async () => {
  let refreshes = 0;
  const session = makeClient(async (config) => {
    if (config.url === "/api/auth/refresh") refreshes += 1;
    return fail(config, 401);
  });
  await assert.rejects(session.client.post("/auth/login", {}));
  assert.equal(refreshes, 0);
  assert.equal(session.signOuts(), 0);
});
