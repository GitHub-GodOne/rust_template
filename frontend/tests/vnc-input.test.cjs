const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");
const { i18n, i18nRequire } = require("./support/i18n-stub.cjs");

const source = fs.readFileSync(path.join(__dirname, "../src/pages/system/vnc/vncInput.ts"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, { require: i18nRequire, exports: output.exports });
const vnc = output.exports;
// Values created inside the vm realm have foreign prototypes.
const plain = (value) => JSON.parse(JSON.stringify(value));

test("typed characters map to X11 keysyms without scancodes", () => {
  assert.deepEqual(plain(vnc.charStroke("a")), { keysym: 0x61, code: "" });
  assert.deepEqual(plain(vnc.charStroke("A")), { keysym: 0x41, code: "" });
  assert.deepEqual(plain(vnc.charStroke("é")), { keysym: 0xe9, code: "" });
  assert.deepEqual(plain(vnc.charStroke("中")), { keysym: 0x01000000 | 0x4e2d, code: "" });
  assert.equal(vnc.charStroke("\n").keysym, vnc.KEYS.enter.keysym);
  assert.equal(vnc.charStroke("\u0007"), null);
  assert.equal(vnc.textStrokes("a\r\nb").length, 3);
});

test("combos press in order and release in reverse", () => {
  const sequence = vnc.comboSequence([vnc.KEYS.ctrl, vnc.KEYS.alt, vnc.KEYS.delete]);
  assert.deepEqual(plain(
    sequence.map(({ stroke, down }) => `${down ? "+" : "-"}${stroke.code}`)),
    ["+ControlLeft", "+AltLeft", "+Delete", "-Delete", "-AltLeft", "-ControlLeft"],
  );
});

test("latched modifiers wrap the next key once, in a stable order", () => {
  const strokes = vnc.withModifiers(["shift", "ctrl"], [vnc.letterKey("c")]);
  assert.deepEqual(plain(strokes.map((stroke) => stroke.code)), ["ControlLeft", "ShiftLeft", "KeyC"]);
  // A shortcut that already holds Ctrl does not press it twice.
  const deduped = vnc.withModifiers(["ctrl"], [vnc.KEYS.ctrl, vnc.letterKey("v")]);
  assert.deepEqual(plain(deduped.map((stroke) => stroke.code)), ["ControlLeft", "KeyV"]);
});

test("shortcut presets are unique and cover the common remote keys", () => {
  const ids = vnc.VNC_SHORTCUTS.map((shortcut) => shortcut.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const id of ["ctrl-alt-del", "task-manager", "switch-window", "copy", "paste", "f12"]) {
    assert.ok(ids.includes(id), id);
  }
  assert.equal(vnc.functionKey(12).keysym, 0xffc9);
});

function gesture() {
  const pad = new vnc.TrackpadGesture({ speed: 2 });
  pad.setBounds(800, 600, true);
  return pad;
}

test("one finger moves the cursor relatively and stays inside the screen", () => {
  const pad = gesture();
  pad.start([{ x: 100, y: 100 }], 0);
  const [move] = pad.move([{ x: 110, y: 95 }]);
  assert.deepEqual(plain(move), { type: "move", x: 420, y: 290, buttons: 0 });
  pad.move([{ x: 2000, y: -2000 }]);
  assert.deepEqual(plain(pad.cursor), { x: 799, y: 0 });
  assert.deepEqual(plain(pad.end([], 100)), []);
});

test("taps click at the cursor; two-finger taps and armed taps right-click", () => {
  const pad = gesture();
  pad.start([{ x: 10, y: 10 }], 0);
  assert.deepEqual(plain(pad.end([], 120)), [{ type: "click", x: 400, y: 300, button: 0 }]);

  pad.start([{ x: 10, y: 10 }], 1000);
  pad.start([{ x: 10, y: 10 }, { x: 60, y: 10 }], 1010);
  assert.deepEqual(plain(pad.end([{ x: 60, y: 10 }], 1100)), []);
  assert.equal(pad.end([], 1120)[0].button, 2);

  pad.start([{ x: 10, y: 10 }], 2000);
  assert.equal(pad.end([], 2100, 2)[0].button, 2);

  pad.start([{ x: 10, y: 10 }], 3000);
  assert.deepEqual(plain(pad.end([], 3600)), [], "a slow touch is not a tap");
});

test("two fingers scroll instead of moving the cursor", () => {
  const pad = gesture();
  pad.start([{ x: 0, y: 100 }, { x: 40, y: 100 }], 0);
  const actions = pad.move([{ x: 0, y: 80 }, { x: 40, y: 80 }]);
  assert.equal(actions[0].type, "scroll");
  assert.ok(actions[0].deltaY > 0, "fingers moving up scroll the page down");
  assert.deepEqual(plain(pad.cursor), { x: 400, y: 300 });
  assert.deepEqual(plain(pad.end([], 50)), [], "a scroll never clicks");
});

test("a long press drags with the left button held", () => {
  const pad = gesture();
  pad.start([{ x: 10, y: 10 }], 0);
  assert.deepEqual(plain(pad.longPress()), [{ type: "down", x: 400, y: 300, button: 0 }]);
  assert.equal(pad.move([{ x: 20, y: 10 }])[0].buttons, 1);
  assert.deepEqual(plain(pad.end([], 900)), [{ type: "up", x: 420, y: 300, button: 0 }]);

  pad.start([{ x: 10, y: 10 }], 1000);
  pad.move([{ x: 40, y: 10 }]);
  assert.deepEqual(plain(pad.longPress()), [], "moving first cancels the long press");
  pad.end([], 1100);

  pad.start([{ x: 10, y: 10 }], 2000);
  pad.longPress();
  assert.deepEqual(plain(pad.cancel()), [{ type: "up", x: 480, y: 300, button: 0 }]);
});

test("rescaling the display keeps the cursor on the same remote pixel", () => {
  const pad = gesture();
  pad.start([{ x: 0, y: 0 }], 0);
  pad.move([{ x: 50, y: 25 }]);
  assert.deepEqual(plain(pad.cursor), { x: 500, y: 350 });
  pad.setBounds(400, 300);
  assert.deepEqual(plain(pad.cursor), { x: 250, y: 175 });
  pad.setBounds(400, 300, true);
  assert.deepEqual(plain(pad.cursor), { x: 200, y: 150 });
});

test("with pinch on, spreading fingers zoom and never click", () => {
  const pad = new vnc.TrackpadGesture({ pinch: true });
  pad.setBounds(800, 600, true);
  pad.start([{ x: 100, y: 100 }, { x: 200, y: 100 }], 0);
  const [first] = pad.move([{ x: 90, y: 100 }, { x: 210, y: 100 }]);
  assert.deepEqual(plain(first), { type: "pinch", x: 150, y: 100, scale: 1.2, deltaX: 0, deltaY: 0 });
  const [zoom] = pad.move([{ x: 50, y: 110 }, { x: 250, y: 110 }]);
  assert.equal(zoom.type, "pinch");
  assert.deepEqual(plain({ x: zoom.x, y: zoom.y, deltaY: zoom.deltaY }), { x: 150, y: 110, deltaY: 10 });
  assert.ok(Math.abs(zoom.scale - 200 / 120) < 1e-9);
  // The finger left behind does not drag the cursor, and lifting does not click.
  assert.deepEqual(plain(pad.end([{ x: 250, y: 110 }], 100)), []);
  assert.deepEqual(plain(pad.move([{ x: 300, y: 150 }])), []);
  assert.deepEqual(plain(pad.end([], 150)), []);
  assert.deepEqual(plain(pad.cursor), { x: 400, y: 300 });
});

test("with pinch on, two fingers moving together still scroll", () => {
  const pad = new vnc.TrackpadGesture({ pinch: true });
  pad.setBounds(800, 600, true);
  pad.start([{ x: 100, y: 100 }, { x: 200, y: 100 }], 0);
  const [scroll] = pad.move([{ x: 100, y: 120 }, { x: 200, y: 120 }]);
  assert.equal(scroll.type, "scroll");
  const [later] = pad.move([{ x: 90, y: 140 }, { x: 215, y: 140 }]);
  assert.equal(later.type, "scroll");
});
