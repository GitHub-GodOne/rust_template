const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(
  path.join(__dirname, "../src/pages/system/project-cli/projectCliModel.ts"),
  "utf8",
);
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports, String, RegExp, Math, Set, Map });
const cli = output.exports;
const plain = (value) => JSON.parse(JSON.stringify(value));

const arg = (overrides) => ({
  long: null,
  short: null,
  value_name: null,
  help: "",
  required: false,
  multiple: false,
  optional_value: false,
  default_value: null,
  possible_values: [],
  secret: false,
  ...overrides,
});

const environment = arg({ id: "environment", kind: "option", long: "environment", short: "e", value_name: "ENVIRONMENT" });
const start = {
  path: ["start"],
  source: "loco",
  about: "Start an app",
  note: "启动应用服务",
  runnable: true,
  risk: "caution",
  long_running: true,
  args: [
    arg({ id: "worker", kind: "option", long: "worker", short: "w", value_name: "WORKER", multiple: true, optional_value: true }),
    arg({ id: "port", kind: "option", long: "port", short: "p", value_name: "PORT", help: "server port address" }),
    arg({ id: "no-banner", kind: "flag", long: "no-banner", short: "n" }),
    environment,
  ],
};
const recover = {
  path: ["task", "admin.recover"],
  source: "task",
  about: "Create or reset an admin user",
  runnable: true,
  risk: "caution",
  long_running: false,
  args: [
    environment,
    arg({ id: "email", kind: "task_param", required: true }),
    arg({ id: "password", kind: "task_param", required: true, secret: true }),
  ],
};
const command = (path, source = "loco", extra = {}) => ({
  path,
  source,
  about: "",
  runnable: true,
  risk: "safe",
  long_running: false,
  args: [],
  ...extra,
});

test("commands are grouped by source in a fixed order", () => {
  const sections = cli.groupCommands([
    command(["data"], "data", { runnable: false }),
    command(["data", "export"], "data"),
    command(["db"], "loco", { runnable: false }),
    command(["db", "migrate"], "loco"),
    recover,
    start,
  ]);
  assert.deepEqual(
    plain(sections.map((section) => section.title)),
    ["Loco 内置命令", "自定义任务", "数据导入导出"],
  );
  assert.deepEqual(
    plain(sections[0].commands.map(({ command, depth }) => [command.path.join(" "), depth])),
    [["db", 0], ["db migrate", 1], ["start", 0]],
  );
  assert.equal(sections[1].commands[0].depth, 0, "tasks are not indented");
});

test("search keeps parents of matching commands", () => {
  const sections = cli.groupCommands(
    [command(["db"], "loco", { runnable: false }), command(["db", "migrate"]), command(["routes"]), start],
    "MIGRATE",
  );
  assert.deepEqual(
    plain(sections[0].commands.map(({ command }) => command.path.join(" "))),
    ["db", "db migrate"],
  );
  assert.equal(cli.groupCommands([start], "port 地址").length, 0, "every word must match");
  assert.equal(cli.groupCommands([start], "port start").length, 1, "options and notes are searched");
});

test("arguments are built in the server's order with secrets masked", () => {
  const values = { "no-banner": true, port: " 5160 ", worker: true, environment: "test" };
  assert.deepEqual(plain(cli.buildArgs(start, values)), [
    "start",
    "--worker",
    "--port=5160",
    "--no-banner",
    "--environment=test",
  ]);
  assert.deepEqual(
    plain(cli.buildArgs(start, { worker: ["a", "b"] }, ["--help"])),
    ["start", "--worker=a", "--worker=b", "--help"],
  );
  assert.deepEqual(
    plain(cli.buildArgs(recover, { email: "a@b.c", password: "secret" })),
    ["task", "admin.recover", "email:a@b.c", `password:${cli.SECRET_MASK}`],
  );
});

test("payload drops empty values and keeps lists for multi-value args", () => {
  assert.deepEqual(
    plain(cli.payloadValues(start, { port: "", "no-banner": false, worker: ["x"], environment: "dev" })),
    { worker: ["x"], environment: "dev" },
  );
  assert.deepEqual(plain(cli.payloadValues(start, { worker: true })), { worker: true });
});

test("defaults and required fields", () => {
  assert.deepEqual(plain(cli.initialValues(recover, "production")), { environment: "production" });
  assert.deepEqual(plain(cli.missingRequired(recover, { email: "a@b.c" })), ["password:<值>"]);
  assert.deepEqual(plain(cli.missingRequired(recover, { email: "a", password: "b" })), []);
});

test("labels read like the help output", () => {
  assert.equal(cli.argLabel(start.args[1]), "-p, --port <PORT>");
  assert.equal(cli.argLabel(start.args[2]), "-n, --no-banner");
  assert.equal(cli.argLabel(arg({ id: "name", kind: "positional", value_name: "NAME", required: true })), "<NAME>");
  assert.equal(cli.argLabel(arg({ id: "fields", kind: "positional", value_name: "FIELDS", multiple: true })), "[FIELDS]...");
});

test("extra arguments are split like a shell", () => {
  assert.deepEqual(plain(cli.splitArgs(` a  "b c" 'd "e"' f\\ g "" `)), ["a", "b c", 'd "e"', "f g", ""]);
  assert.deepEqual(plain(cli.splitArgs("")), []);
});

test("command lines are quoted for copying", () => {
  assert.equal(cli.formatCommandLine("cargo loco", ["db", "migrate", "-e", "test"]), "cargo loco db migrate -e test");
  assert.equal(cli.formatCommandLine("bin", ["a b", "it's"]), "bin 'a b' 'it'\\''s'");
});

test("colour codes are removed from output", () => {
  assert.equal(cli.stripAnsi("\u001b[32mINFO\u001b[0m ready"), "INFO ready");
  assert.equal(cli.formatDuration(850), "850 ms");
  assert.equal(cli.formatDuration(12_300), "12.3 秒");
  assert.equal(cli.formatDuration(125_000), "2 分 5 秒");
});
