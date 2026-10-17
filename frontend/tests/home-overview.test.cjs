const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const ts = require("typescript");
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");

function load(relativePath, modules = {}) {
  const source = fs.readFileSync(path.join(__dirname, relativePath), "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, jsx: ts.JsxEmit.ReactJSX },
  }).outputText;
  const output = { exports: {} };
  vm.runInNewContext(compiled, {
    exports: output.exports,
    require(name) {
      return modules[name] ?? require(name);
    },
  });
  return output.exports;
}

const home = load("../src/pages/home/homeOverview.ts");
const { ActivityTrend } = load("../src/pages/home/ActivityTrend.tsx", { "./homeOverview": home });

const todo = (key, level, count) => ({ key, level, count, title: key, description: "", path: "/" });

test("greeting follows the time of day", () => {
  assert.equal(home.greeting(3), "夜深了");
  assert.equal(home.greeting(8), "早上好");
  assert.equal(home.greeting(13), "中午好");
  assert.equal(home.greeting(15), "下午好");
  assert.equal(home.greeting(21), "晚上好");
});

test("uptime and relative time are human readable", () => {
  assert.equal(home.formatUptime(30), "1 分钟");
  assert.equal(home.formatUptime(3 * 3600 + 120), "3 小时 2 分钟");
  assert.equal(home.formatUptime(2 * 86_400 + 5 * 3600), "2 天 5 小时");
  const now = new Date("2026-10-10T12:00:00Z");
  assert.equal(home.formatRelativeTime("2026-10-10T11:59:40Z", now), "刚刚");
  assert.equal(home.formatRelativeTime("2026-10-10T11:15:00Z", now), "45 分钟前");
  assert.equal(home.formatRelativeTime("2026-10-08T12:00:00Z", now), "2 天前");
});

test("outstanding todos come first, most severe on top, cleared items keep order", () => {
  const sorted = home.sortTodos([
    todo("locked", "warning", 0),
    todo("mine", "info", 2),
    todo("overdue", "error", 1),
    todo("unassigned", "warning", 4),
    todo("refunds", "warning", 0),
  ]);
  assert.deepEqual(
    Array.from(sorted, (item) => item.key),
    ["overdue", "unassigned", "mine", "locked", "refunds"],
  );
  assert.equal(home.pendingTodoTotal(sorted), 7);
});

test("axis maximum rounds up to a clean step", () => {
  assert.equal(home.niceMax(0), 1);
  assert.equal(home.niceMax(2), 2);
  assert.equal(home.niceMax(7), 10);
  assert.equal(home.niceMax(130), 200);
  assert.equal(home.niceMax(480), 500);
});

test("trend bars scale against the rounded maximum and flag today", () => {
  const { max, bars } = home.buildTrendBars([
    { date: "2026-10-09", total: 40, errors: 4 },
    { date: "2026-10-10", total: 80, errors: 0 },
  ]);
  assert.equal(max, 100);
  assert.equal(bars[0].label, "10-09");
  assert.equal(bars[0].normal, 36);
  assert.equal(bars[0].totalPercent, 40);
  assert.equal(bars[0].errorPercent, 4);
  assert.equal(bars[1].label, "今天");
  assert.equal(bars[1].isToday, true);
});

test("quick entries keep the preferred order and respect permissions", () => {
  const menus = [
    { key: "home", label: "首页", path: "/admin/dashboard" },
    {
      key: "system",
      label: "系统管理",
      children: [
        { key: "logs", label: "日志中心", path: "/admin/system/logs", permission: "system:log:list" },
        { key: "users", label: "用户管理", path: "/admin/system/users", permission: "system:user:list" },
        { key: "roles", label: "角色管理", path: "/admin/system/roles", permission_code: "system:role:list" },
      ],
    },
  ];
  const allowed = new Set(["system:user:list", "system:log:list"]);
  const entries = home.pickQuickEntries(menus, (permission) => !permission || allowed.has(permission));
  assert.deepEqual(
    Array.from(entries, (entry) => entry.label),
    ["用户管理", "日志中心"],
  );
});

test("trend chart renders a legend, today's value and an accessible table", () => {
  const markup = renderToStaticMarkup(
    React.createElement(ActivityTrend, {
      points: [
        { date: "2026-10-09", total: 12, errors: 3 },
        { date: "2026-10-10", total: 18, errors: 0 },
      ],
    }),
  );
  assert.match(markup, /正常操作/);
  assert.match(markup, /错误/);
  assert.match(markup, /今天/);
  assert.match(markup, /home-trend-value[^>]*>18</);
  assert.match(markup, /<caption>近 7 日系统活动<\/caption>/);
  assert.match(markup, /<th scope="row">2026-10-09<\/th><td>12<\/td><td>3<\/td>/);
  // Only days with errors get an error segment.
  assert.equal((markup.match(/home-trend-segment is-error/g) ?? []).length, 1);
});
