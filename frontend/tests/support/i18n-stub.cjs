// Stand-in for src/i18n in node tests: returns the Chinese source text with
// `{0}`-style placeholders filled, exactly like the zh-CN runtime.
const format = (text, ...args) =>
  args.length === 0
    ? text
    : text.replace(/\{(\d+)\}/g, (match, index) => {
        const value = args[Number(index)];
        return value === undefined || value === null ? "" : String(value);
      });

const i18n = { t: format, tData: (text) => text ?? "", locale: "zh-CN", isEnglish: false };

/** `require` for sandboxed modules: serves the i18n stub, defers the rest. */
function i18nRequire(name) {
  if (/(^|\/)i18n$/.test(name)) return i18n;
  return require(name);
}

module.exports = { i18n, i18nRequire };
