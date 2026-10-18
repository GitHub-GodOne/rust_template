const assert = require("node:assert/strict");
const test = require("node:test");
const { allKeys, loadDictionary, jsxRenderedText } = require("../scripts/i18n.cjs");

const placeholders = (text) => (text.match(/\{\d+\}/g) ?? []).sort();

test("every Chinese UI string goes through t() and has an English translation", () => {
  const { keys, untranslated } = allKeys();
  const dictionary = loadDictionary();
  assert.deepEqual(untranslated, [], "wrap these strings with t()");
  const missing = keys.filter((key) => !dictionary[key]);
  assert.deepEqual(missing, [], "add these keys to src/i18n/en.json");
  assert.ok(keys.length > 2000, `only ${keys.length} keys found`);
});

test("translations keep their placeholders and the dictionary has no stale keys", () => {
  const { keys } = allKeys();
  const dictionary = loadDictionary();
  const used = new Set(keys);
  const stale = Object.keys(dictionary).filter((key) => !used.has(key));
  assert.deepEqual(stale, [], "remove unused keys from src/i18n/en.json");
  for (const [key, value] of Object.entries(dictionary)) {
    assert.deepEqual(placeholders(value), placeholders(key), `placeholders differ for ${key}`);
    assert.doesNotMatch(value, /[一-鿿]/, `English text still contains Chinese: ${key}`);
  }
});

test("JSX text is normalized like React renders it", () => {
  assert.equal(jsxRenderedText("\n    保存\n  "), "保存");
  assert.equal(jsxRenderedText(" 剩余 "), " 剩余 ");
  assert.equal(jsxRenderedText("第一行\n   第二行"), "第一行 第二行");
});
