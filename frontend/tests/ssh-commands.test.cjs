const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");

const source = fs.readFileSync(path.join(__dirname, "../src/pages/system/ssh/sshCommands.ts"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { exports: output.exports, TextEncoder });
const commands = output.exports;

const plain = (value) => JSON.parse(JSON.stringify(value));

test("stored preferences are normalized and malformed entries dropped", () => {
  assert.deepEqual(plain(commands.normalizeSshCommands(null)), []);
  assert.deepEqual(plain(commands.normalizeSshCommands({ commands: "nope" })), []);
  assert.deepEqual(
    plain(
      commands.normalizeSshCommands({
        commands: [
          { id: "a", name: " 磁盘 ", command: "df -h", auto_enter: true },
          { id: "a", name: "duplicate", command: "ls" },
          { id: "b", name: "", command: "uptime" },
          { id: "c", name: "empty", command: "   " },
          { name: "no id", command: "ls" },
          { id: "d", name: "edit", command: "kill ", auto_enter: false },
          { id: "e", name: "env", command: "export A=1", run_on_connect: true },
        ],
      }),
    ),
    [
      { id: "a", name: "磁盘", command: "df -h", auto_enter: true, run_on_connect: false },
      { id: "b", name: "uptime", command: "uptime", auto_enter: true, run_on_connect: false },
      { id: "d", name: "edit", command: "kill ", auto_enter: false, run_on_connect: false },
      { id: "e", name: "env", command: "export A=1", auto_enter: true, run_on_connect: true },
    ],
  );
});

test("terminal input runs each line and honours auto-enter", () => {
  const command = (text, autoEnter) => ({ id: "x", name: "x", command: text, auto_enter: autoEnter });
  assert.equal(commands.sshCommandInput(command("df -h", true)), "df -h\r");
  assert.equal(commands.sshCommandInput(command("cd /srv\nls -la\n", true)), "cd /srv\rls -la\r");
  assert.equal(commands.sshCommandInput(command("a\r\nb", false)), "a\rb");
  assert.equal(commands.sshCommandInput(command("kill ", false)), "kill ");
});

test("a new session gets every run-on-connect command, each executed", () => {
  const command = (id, text, autoEnter, runOnConnect) => ({
    id,
    name: id,
    command: text,
    auto_enter: autoEnter,
    run_on_connect: runOnConnect,
  });
  assert.equal(commands.sshConnectInput([command("a", "ls", true, false)]), "");
  assert.equal(
    commands.sshConnectInput([
      command("a", "export A=1", true, true),
      command("b", "df -h", true, false),
      command("c", "cd /srv\nsource .env", false, true),
    ]),
    "export A=1\rcd /srv\rsource .env\r",
  );
});

test("moving commands swaps neighbours and ignores out-of-range moves", () => {
  const list = ["a", "b", "c"].map((id) => ({ id, name: id, command: id, auto_enter: true }));
  assert.deepEqual(plain(commands.moveSshCommand(list, 1, -1).map((item) => item.id)), ["b", "a", "c"]);
  assert.deepEqual(plain(commands.moveSshCommand(list, 1, 1).map((item) => item.id)), ["a", "c", "b"]);
  assert.equal(commands.moveSshCommand(list, 0, -1), list);
  assert.equal(commands.moveSshCommand(list, 2, 1), list);
});

test("the stored list must fit the 16 KiB preference limit", () => {
  const entry = (id, text) => ({ id, name: id, command: text, auto_enter: true });
  assert.equal(commands.fitsPreferenceLimit([entry("a", "ls")]), true);
  const large = Array.from({ length: 20 }, (_, index) => entry(`c${index}`, "磁".repeat(400)));
  assert.equal(commands.fitsPreferenceLimit(large), false, "multi-byte text counts in UTF-8 bytes");
});
