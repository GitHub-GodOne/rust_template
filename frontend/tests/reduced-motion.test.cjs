const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");

const css = fs.readFileSync(path.join(__dirname, "../src/index.css"), "utf8");
const globalReducedMotion = css.match(
  /@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{\s*\*,\s*\*::before,\s*\*::after\s*\{([^}]+)\}/,
)?.[1];

test("reduced motion does not introduce transitions during popup measurement", () => {
  assert.ok(globalReducedMotion, "Keep the global reduced-motion rule");
  // Even 0.01ms makes rc-trigger read the old off-screen coordinates after
  // setting left/top synchronously. An inherited delay must not defer it either.
  assert.match(globalReducedMotion, /transition-duration:\s*0s\s*!important;/);
  assert.match(globalReducedMotion, /transition-delay:\s*0s\s*!important;/);
});

test("reduced motion still shortens animations without losing completion events", () => {
  assert.ok(globalReducedMotion);
  assert.match(
    globalReducedMotion,
    /animation-duration:\s*0\.01ms\s*!important;/,
  );
  assert.match(
    globalReducedMotion,
    /animation-iteration-count:\s*1\s*!important;/,
  );
  assert.match(globalReducedMotion, /scroll-behavior:\s*auto\s*!important;/);
});
