#!/usr/bin/env node
// Generates src/styles/dark-theme.css: dark-mode overrides for every
// hand-written stylesheet. Light backgrounds and borders become dark, dark
// text becomes light; hue is kept so the warm palette survives. Colors that
// already suit a dark page (terminals, accent buttons, white text on color)
// are left alone. Re-run after changing CSS:  npm run theme:dark

const fs = require("node:fs");
const path = require("node:path");

const root = path.join(__dirname, "..", "src");
const output = path.join(root, "styles", "dark-theme.css");
const SCOPE = ':root[data-theme="dark"]';

function cssFiles(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory())
      return entry.name === "styles" ? [] : cssFiles(full);
    return entry.name.endsWith(".css") ? [full] : [];
  });
}

/** Comment-free CSS → tree of rules and at-rules. */
function parse(css) {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  let index = 0;
  function block() {
    const nodes = [];
    let buffer = "";
    while (index < text.length) {
      const char = text[index];
      if (char === "{") {
        index += 1;
        const prelude = buffer.trim();
        buffer = "";
        if (prelude.startsWith("@")) {
          nodes.push({ type: "at", prelude, children: block() });
        } else {
          let body = "";
          let depth = 0;
          while (index < text.length && (text[index] !== "}" || depth > 0)) {
            if (text[index] === "{") depth += 1;
            if (text[index] === "}") depth -= 1;
            body += text[index];
            index += 1;
          }
          index += 1;
          nodes.push({ type: "rule", selector: prelude, body });
        }
      } else if (char === "}") {
        index += 1;
        return nodes;
      } else if (char === ";" && buffer.trim().startsWith("@")) {
        buffer = "";
        index += 1;
      } else {
        buffer += char;
        index += 1;
      }
    }
    return nodes;
  }
  return block();
}

function declarations(body) {
  const out = [];
  let current = "";
  let depth = 0;
  for (const char of body) {
    if (char === "(") depth += 1;
    if (char === ")") depth -= 1;
    if (char === ";" && depth === 0) {
      out.push(current);
      current = "";
    } else {
      current += char;
    }
  }
  out.push(current);
  return out
    .map((decl) => decl.trim())
    .filter(Boolean)
    .map((decl) => {
      const colon = decl.indexOf(":");
      return colon < 0
        ? null
        : {
            prop: decl.slice(0, colon).trim().toLowerCase(),
            value: decl.slice(colon + 1).trim(),
          };
    })
    .filter(Boolean);
}

function toRgb(token) {
  const named = { white: [255, 255, 255, 1], black: [0, 0, 0, 1] };
  if (named[token.toLowerCase()]) return named[token.toLowerCase()];
  if (token.startsWith("#")) {
    let hex = token.slice(1);
    if (hex.length === 3 || hex.length === 4)
      hex = [...hex].map((c) => c + c).join("");
    const value = (i) => Number.parseInt(hex.slice(i, i + 2), 16);
    return [
      value(0),
      value(2),
      value(4),
      hex.length === 8 ? value(6) / 255 : 1,
    ];
  }
  const parts = token
    .replace(/rgba?\(|\)/g, "")
    .split(/[\s,/]+/)
    .filter(Boolean)
    .map((part) =>
      part.endsWith("%") ? (Number.parseFloat(part) / 100) * 255 : Number(part),
    );
  return [parts[0], parts[1], parts[2], parts[3] ?? 1];
}

function rgbToHsl([r, g, b]) {
  const [rn, gn, bn] = [r / 255, g / 255, b / 255];
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h;
  if (max === rn) h = (gn - bn) / d + (gn < bn ? 6 : 0);
  else if (max === gn) h = (bn - rn) / d + 2;
  else h = (rn - gn) / d + 4;
  return [h * 60, s, l];
}

function hslToRgb([h, s, l]) {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] =
    h < 60
      ? [c, x, 0]
      : h < 120
        ? [x, c, 0]
        : h < 180
          ? [0, c, x]
          : h < 240
            ? [0, x, c]
            : h < 300
              ? [x, 0, c]
              : [c, 0, x];
  return [r, g, b].map((v) => Math.round((v + m) * 255));
}

function format([r, g, b], alpha) {
  if (alpha < 1) return `rgba(${r}, ${g}, ${b}, ${Number(alpha.toFixed(3))})`;
  return `#${[r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** surface: backgrounds/borders; ink: text. Returns null when unchanged. */
function convert(token, role) {
  const [r, g, b, a] = toRgb(token);
  const [h, s, l] = rgbToHsl([r, g, b]);
  if (role === "surface" && l >= 0.55) {
    return format(
      hslToRgb([h, Math.min(s * 0.5, 0.16), 0.1 + (1 - l) * 0.6]),
      a,
    );
  }
  if (role === "border" && l >= 0.55) {
    return format(
      hslToRgb([h, Math.min(s * 0.5, 0.14), 0.18 + (1 - l) * 0.5]),
      a,
    );
  }
  if (role === "ink" && l <= 0.5) {
    return format(hslToRgb([h, Math.min(s, 0.5), 0.95 - l * 0.7]), a);
  }
  return null;
}

const COLOR = /#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)|\b(?:white|black)\b/g;
const ROLE = {
  background: "surface",
  "background-color": "surface",
  "background-image": "surface",
  color: "ink",
  fill: "ink",
  stroke: "ink",
  "caret-color": "ink",
  "text-decoration-color": "ink",
  border: "border",
  "border-color": "border",
  "border-top": "border",
  "border-right": "border",
  "border-bottom": "border",
  "border-left": "border",
  "border-top-color": "border",
  "border-bottom-color": "border",
  "border-left-color": "border",
  "border-right-color": "border",
  outline: "border",
  "outline-color": "border",
};

/** Each color declaration with its dark value, or null when it can stay. */
function darkDeclarations(body) {
  const out = [];
  for (const { prop, value } of declarations(body)) {
    const role = ROLE[prop];
    if (!role) continue;
    let changed = false;
    const next = value.replace(COLOR, (token) => {
      const converted = convert(token, role);
      if (converted) changed = true;
      return converted ?? token;
    });
    out.push({ prop, dark: changed ? next : null });
  }
  return out;
}

/** `background` resets its longhands, so they compete in the cascade. */
const cascadeGroup = (prop) =>
  prop.startsWith("background") ? "background" : prop;

function scopeSelector(selector) {
  return selector
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      if (/^(:root|html)\b/.test(part))
        return part.replace(/^(:root|html)/, SCOPE);
      return `${SCOPE} ${part}`;
    })
    .join(",\n");
}

/**
 * Flattens rules to (context, selector, property) entries in source order.
 * Only the last entry per key wins in the light theme, so only that one
 * decides the dark override; an earlier, overridden declaration must not
 * resurface through the more specific dark selector.
 */
function collect(nodes, context, entries) {
  for (const node of nodes) {
    if (node.type === "at") {
      if (/^@(media|supports|layer)/.test(node.prelude)) {
        collect(node.children, [...context, node.prelude], entries);
      }
      continue;
    }
    const decls = darkDeclarations(node.body);
    const selectors = node.selector
      .split(",")
      .map((part) => part.trim())
      .filter(Boolean);
    for (const selector of selectors) {
      for (const decl of decls) {
        entries.push({ context, selector, ...decl });
      }
    }
  }
  return entries;
}

function render(entries) {
  const keyOf = (entry) =>
    [...entry.context, entry.selector, cascadeGroup(entry.prop)].join("\u0000");
  const winners = new Map();
  for (const entry of entries) {
    winners.set(keyOf(entry), entry);
  }
  const blocks = new Map();
  for (const entry of entries) {
    if (winners.get(keyOf(entry)) !== entry || !entry.dark) continue;
    const blockKey = [...entry.context, entry.selector].join("\u0000");
    if (!blocks.has(blockKey)) {
      blocks.set(blockKey, {
        context: entry.context,
        selector: entry.selector,
        decls: [],
      });
    }
    blocks.get(blockKey).decls.push(`${entry.prop}: ${entry.dark}`);
  }
  return [...blocks.values()]
    .map(({ context, selector, decls }) => {
      const indent = "\t".repeat(context.length);
      let text = `${indent}${scopeSelector(selector)} {\n${decls
        .map((decl) => `${indent}\t${decl};`)
        .join("\n")}\n${indent}}`;
      for (let depth = context.length - 1; depth >= 0; depth -= 1) {
        const outer = "\t".repeat(depth);
        text = `${outer}${context[depth]} {\n${text}\n${outer}}`;
      }
      return text;
    })
    .join("\n\n");
}

function main() {
  const files = cssFiles(root).sort();
  const sections = files
    .map((file) => {
      const rendered = render(
        collect(parse(fs.readFileSync(file, "utf8")), [], []),
      );
      return rendered
        ? `/* ${path.relative(root, file).replaceAll("\\", "/")} */\n${rendered}`
        : "";
    })
    .filter(Boolean);

  fs.mkdirSync(path.dirname(output), { recursive: true });
  fs.writeFileSync(
    output,
    `/* Generated by scripts/build-dark-theme.cjs — do not edit; run npm run theme:dark. */\n\n${SCOPE} body {\n\tbackground: #141311;\n\tcolor: #ece7df;\n}\n\n${sections.join("\n\n")}\n`,
  );
  console.log(
    `dark theme: ${files.length} stylesheets → ${path.relative(process.cwd(), output)}`,
  );
}

if (require.main === module) main();

module.exports = { convert, scopeSelector };
