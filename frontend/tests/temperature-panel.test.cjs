const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");

const source = fs.readFileSync(path.join(__dirname, "../src/pages/system/monitoring/TemperaturePanel.tsx"), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, jsx: ts.JsxEmit.ReactJSX },
}).outputText;
const output = { exports: {} };
vm.runInNewContext(compiled, {
  exports: output.exports,
  require(name) {
    if (name === "../../../api/admin/monitoring") return { fetchTemperatureHistory: () => Promise.resolve({ items: [], total: 0 }) };
    if (name === "../../../components/admin/DataTable") {
      return { DataTable: ({ columns, dataSource }) => React.createElement("table", {}, dataSource.map((row) =>
        React.createElement("tbody", { key: row.sensor }, React.createElement("tr", {}, columns.map((column) =>
          React.createElement("td", { key: column.dataIndex }, column.render ? column.render(row[column.dataIndex], row) : row[column.dataIndex])))))) };
    }
    return require(name);
  },
});

function render(snapshot) {
  return renderToStaticMarkup(React.createElement(output.exports.TemperatureCard, { snapshot }));
}

const reading = (sensor, value) => ({ sensor, temperature_celsius: value, critical_celsius: null });

test("highest reading and individual sensor values are visible", () => {
  const markup = render({ captured_at: "2026-10-07T06:00:00Z", available: true, message: null, sensors: [reading("CPU sensor", 42.5), reading("Board sensor", 51.2)] });
  assert.match(markup, /当前最高读数/);
  assert.match(markup, /51\.2 ℃/);
  assert.match(markup, /42\.5 ℃/);
  assert.match(markup, /Board sensor/);
});

test("unavailable readings never appear as zero degrees", () => {
  const markup = render({ captured_at: null, available: false, message: "未检测到可读取的温度传感器", sensors: [reading("Unavailable sensor", null)] });
  assert.match(markup, /不可用/);
  assert.match(markup, /未检测到可读取的温度传感器/);
  assert.doesNotMatch(markup, /0\.0 ℃/);
});

test("a measured zero degrees remains a valid displayed temperature", () => {
  const markup = render({ captured_at: null, available: true, message: null, sensors: [reading("Cold sensor", 0)] });
  assert.match(markup, /0\.0 ℃/);
  assert.doesNotMatch(markup, /等待有效读数/);
});
