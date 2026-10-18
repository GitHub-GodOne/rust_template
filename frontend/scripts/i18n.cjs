#!/usr/bin/env node
// i18n tooling for the admin frontend.
//
//   node scripts/i18n.cjs wrap [files...]  wrap Chinese UI strings in t()
//   node scripts/i18n.cjs keys             print every message key as JSON
//   node scripts/i18n.cjs missing          print keys without an English translation
//
// Message keys are the Chinese source strings. Besides t("...") calls in
// src/, server-provided names seeded from the backend fixtures (menu titles,
// permission names) are keys too, translated at runtime with tData().

const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");

const root = path.resolve(__dirname, "..");
const srcDir = path.join(root, "src");
const fixturesDir = path.resolve(root, "../src/fixtures");
const dictionaryPath = path.join(srcDir, "i18n/en.json");
const HAN = /[一-鿿]/;
const TRANSLATORS = new Set(["t", "tr", "tData"]);
const STRING_METHODS = new Set([
  "includes",
  "startsWith",
  "endsWith",
  "indexOf",
  "lastIndexOf",
  "split",
  "replace",
  "replaceAll",
  "match",
  "matchAll",
  "search",
  "test",
  "localeCompare",
]);
// Object properties whose values are data, not display text.
const DATA_PROPERTIES = new Set([
  "value",
  "key",
  "code",
  "id",
  "type",
  "dataIndex",
  "path",
  "icon",
  "permission",
  "permission_code",
  "group_name",
]);

function sourceFiles() {
  const files = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== "i18n") walk(full);
      } else if (/\.(ts|tsx)$/.test(entry.name) && !entry.name.endsWith(".d.ts")) {
        files.push(full);
      }
    }
  };
  walk(srcDir);
  return files;
}

function jsxRenderedText(value) {
  // Babel's whitespace rules for JSX text children.
  const lines = value.split(/\r\n|\n|\r/);
  let lastNonEmptyLine = 0;
  lines.forEach((line, index) => {
    if (/[^ \t]/.test(line)) lastNonEmptyLine = index;
  });
  let rendered = "";
  lines.forEach((line, index) => {
    let trimmed = line.replace(/\t/g, " ");
    if (index !== 0) trimmed = trimmed.replace(/^[ ]+/, "");
    if (index !== lines.length - 1) trimmed = trimmed.replace(/[ ]+$/, "");
    if (trimmed) {
      if (index !== lastNonEmptyLine) trimmed += " ";
      rendered += trimmed;
    }
  });
  return rendered;
}

function isTranslatorCall(node) {
  return (
    node &&
    ts.isCallExpression(node) &&
    ts.isIdentifier(node.expression) &&
    TRANSLATORS.has(node.expression.text)
  );
}

function shouldSkipLiteral(node) {
  const parent = node.parent;
  if (!parent) return true;
  if (
    ts.isImportDeclaration(parent) ||
    ts.isExportDeclaration(parent) ||
    ts.isExternalModuleReference(parent) ||
    ts.isLiteralTypeNode(parent) ||
    ts.isTaggedTemplateExpression(parent) ||
    ts.isCaseClause(parent) ||
    ts.isEnumMember(parent)
  ) {
    return true;
  }
  if (
    (ts.isPropertyAssignment(parent) ||
      ts.isPropertySignature(parent) ||
      ts.isMethodDeclaration(parent) ||
      ts.isPropertyDeclaration(parent)) &&
    parent.name === node
  ) {
    return true;
  }
  if (ts.isPropertyAssignment(parent) && parent.initializer === node) {
    const name = parent.name;
    if ((ts.isIdentifier(name) || ts.isStringLiteral(name)) && DATA_PROPERTIES.has(name.text)) {
      return true;
    }
  }
  if (ts.isComputedPropertyName(parent)) return true;
  if (ts.isElementAccessExpression(parent) && parent.argumentExpression === node) return true;
  if (
    ts.isBinaryExpression(parent) &&
    [
      ts.SyntaxKind.EqualsEqualsEqualsToken,
      ts.SyntaxKind.ExclamationEqualsEqualsToken,
      ts.SyntaxKind.EqualsEqualsToken,
      ts.SyntaxKind.ExclamationEqualsToken,
    ].includes(parent.operatorToken.kind)
  ) {
    return true;
  }
  if (ts.isCallExpression(parent)) {
    if (isTranslatorCall(parent)) return true;
    const callee = parent.expression;
    if (ts.isPropertyAccessExpression(callee)) {
      if (STRING_METHODS.has(callee.name.text)) return true;
      if (ts.isIdentifier(callee.expression) && callee.expression.text === "console") return true;
    }
  }
  if (ts.isJsxAttribute(parent)) {
    const name = parent.name.getText();
    if (name === "key" || name === "value" || name === "permission") return true;
  }
  return false;
}

function hasJsx(node) {
  let found = false;
  const visit = (child) => {
    if (found) return;
    if (
      ts.isJsxElement(child) ||
      ts.isJsxSelfClosingElement(child) ||
      ts.isJsxFragment(child)
    ) {
      found = true;
      return;
    }
    ts.forEachChild(child, visit);
  };
  visit(node);
  return found;
}

function isPrimitiveType(checker, expression) {
  const type = checker.getTypeAtLocation(expression);
  const primitive = (candidate) =>
    (candidate.flags &
      (ts.TypeFlags.StringLike |
        ts.TypeFlags.NumberLike |
        ts.TypeFlags.BigIntLike |
        ts.TypeFlags.Null |
        ts.TypeFlags.Undefined)) !==
    0;
  return type.isUnion() ? type.types.every(primitive) : primitive(type);
}

function bindsName(sourceFile, name) {
  let found = false;
  const visit = (node) => {
    if (found) return;
    if (
      (ts.isParameter(node) ||
        ts.isVariableDeclaration(node) ||
        ts.isBindingElement(node) ||
        ts.isFunctionDeclaration(node)) &&
      node.name &&
      ts.isIdentifier(node.name) &&
      node.name.text === name
    ) {
      found = true;
      return;
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return found;
}

function i18nImportPath(file) {
  let relative = path.relative(path.dirname(file), path.join(srcDir, "i18n"));
  if (!relative.startsWith(".")) relative = `./${relative}`;
  return relative.split(path.sep).join("/");
}

function wrapFile(program, file) {
  const checker = program.getTypeChecker();
  const sourceFile = program.getSourceFile(file);
  const text = sourceFile.getFullText();
  if (!HAN.test(text)) return false;
  const alreadyImported = /from\s+["'][./]*i18n["']/.test(text);
  const fn = bindsName(sourceFile, "t") ? "tr" : "t";
  const replacements = [];

  // Applies already-collected inner replacements to a source range.
  const render = (start, end) => {
    const inner = replacements
      .filter((item) => item.start >= start && item.end <= end)
      .sort((a, b) => b.start - a.start);
    let result = text.slice(start, end);
    for (const item of inner) {
      result =
        result.slice(0, item.start - start) + item.text + result.slice(item.end - start);
    }
    return result;
  };
  const replaceRange = (start, end, replacement) => {
    for (let index = replacements.length - 1; index >= 0; index -= 1) {
      if (replacements[index].start >= start && replacements[index].end <= end) {
        replacements.splice(index, 1);
      }
    }
    replacements.push({ start, end, text: replacement });
  };
  const call = (template, args) =>
    `${fn}(${[JSON.stringify(template), ...args].join(", ")})`;

  const handleJsxChildren = (children) => {
    // Merge runs of text + primitive expressions into one message.
    let run = [];
    const flush = () => {
      const hanText = run.some(
        (child) => ts.isJsxText(child) && HAN.test(child.text),
      );
      if (hanText) {
        let template = "";
        const args = [];
        for (const child of run) {
          if (ts.isJsxText(child)) {
            template += jsxRenderedText(child.text);
          } else {
            template += `{${args.length}}`;
            args.push(render(child.expression.getStart(), child.expression.getEnd()));
          }
        }
        const leading = /^\s/.test(template);
        const trailing = /\s$/.test(template) && template.trim().length > 0;
        const core = template.trim();
        const hasEntity = run.some(
          (child) => ts.isJsxText(child) && /&[a-z#0-9]+;/i.test(child.text),
        );
        if (core && !hasEntity) {
          const first = run[0];
          const last = run[run.length - 1];
          const replacement = `${leading ? '{" "}' : ""}{${call(core, args)}}${trailing ? '{" "}' : ""}`;
          // JSX text has no trivia: use its full range so edge spaces are not duplicated.
          const start = ts.isJsxText(first) ? first.pos : first.getStart();
          const end = ts.isJsxText(last) ? last.end : last.getEnd();
          replaceRange(start, end, replacement);
        }
      }
      run = [];
    };
    for (const child of children) {
      if (ts.isJsxText(child)) {
        run.push(child);
      } else if (
        ts.isJsxExpression(child) &&
        child.expression &&
        !hasJsx(child.expression) &&
        !isTranslatorCall(child.expression) &&
        isPrimitiveType(checker, child.expression)
      ) {
        run.push(child);
      } else {
        flush();
      }
    }
    flush();
  };

  const visit = (node) => {
    ts.forEachChild(node, visit);
    if (ts.isJsxElement(node) || ts.isJsxFragment(node)) {
      handleJsxChildren(node.children);
      return;
    }
    if (
      (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) &&
      HAN.test(node.text) &&
      !shouldSkipLiteral(node)
    ) {
      const replacement = call(node.text, []);
      replaceRange(
        node.getStart(),
        node.getEnd(),
        ts.isJsxAttribute(node.parent) ? `{${replacement}}` : replacement,
      );
      return;
    }
    if (ts.isTemplateExpression(node) && !shouldSkipLiteral(node)) {
      const parts = [node.head.text, ...node.templateSpans.map((span) => span.literal.text)];
      if (!parts.some((part) => HAN.test(part))) return;
      let template = node.head.text;
      const args = [];
      node.templateSpans.forEach((span, index) => {
        template += `{${index}}${span.literal.text}`;
        args.push(render(span.expression.getStart(), span.expression.getEnd()));
      });
      const replacement = call(template, args);
      replaceRange(
        node.getStart(),
        node.getEnd(),
        ts.isJsxAttribute(node.parent) ? `{${replacement}}` : replacement,
      );
    }
  };
  visit(sourceFile);
  if (replacements.length === 0) return false;

  let output = text;
  for (const item of replacements.sort((a, b) => b.start - a.start)) {
    output = output.slice(0, item.start) + item.text + output.slice(item.end);
  }
  if (!alreadyImported) {
    const imports = sourceFile.statements.filter(ts.isImportDeclaration);
    const specifier = fn === "t" ? "t" : "t as tr";
    const line = `import { ${specifier} } from "${i18nImportPath(file)}";\n`;
    if (imports.length > 0) {
      const lastImport = imports[imports.length - 1];
      // Offsets after the last import are untouched by earlier replacements
      // only if no replacement precedes it; imports never contain UI text.
      const position = lastImport.getEnd();
      output = `${output.slice(0, position)}\n${line.trimEnd()}${output.slice(position)}`;
    } else {
      output = line + output;
    }
  }
  fs.writeFileSync(file, output);
  return replacements.length;
}

function createProgram() {
  const configPath = path.join(root, "tsconfig.json");
  const config = ts.readConfigFile(configPath, ts.sys.readFile);
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, root);
  return ts.createProgram(parsed.fileNames, parsed.options);
}

function translatorKeys(file) {
  const sourceFile = ts.createSourceFile(
    file,
    fs.readFileSync(file, "utf8"),
    ts.ScriptTarget.Latest,
    true,
    file.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS,
  );
  const keys = [];
  const untranslated = [];
  const visit = (node) => {
    if (isTranslatorCall(node) && node.arguments.length > 0) {
      const first = node.arguments[0];
      if (ts.isStringLiteral(first) || ts.isNoSubstitutionTemplateLiteral(first)) {
        keys.push(first.text);
      }
    } else if (
      (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node) || ts.isJsxText(node)) &&
      HAN.test(node.text) &&
      !(ts.isJsxText(node) ? false : shouldSkipLiteral(node))
    ) {
      untranslated.push(node.text.trim());
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return { keys, untranslated };
}

function fixtureKeys() {
  const keys = [];
  const read = (name, field) => {
    const file = path.join(fixturesDir, name);
    if (!fs.existsSync(file)) return;
    for (const line of fs.readFileSync(file, "utf8").split("\n")) {
      const match = line.match(new RegExp(`^\\s+${field}:\\s*(.+?)\\s*$`));
      if (match && HAN.test(match[1])) keys.push(match[1].replace(/^"|"$/g, ""));
    }
  };
  read("menus.yaml", "title");
  read("permissions.yaml", "name");
  // Seeded names shown on the home page.
  read("roles.yaml", "name");
  read("tenants.yaml", "name");
  read("departments.yaml", "name");
  // UI text generated by the backend (home to-dos).
  const dashboard = path.resolve(root, "../src/controllers/admin/dashboard.rs");
  if (fs.existsSync(dashboard)) {
    for (const match of fs.readFileSync(dashboard, "utf8").matchAll(/"((?:[^"\\]|\\.)*)"/g)) {
      if (HAN.test(match[1])) keys.push(match[1]);
    }
  }
  // Audit feature names and groups: feature("key", "名称", "分组", ...).
  const audit = path.resolve(root, "../src/services/audit.rs");
  if (fs.existsSync(audit)) {
    const source = fs.readFileSync(audit, "utf8");
    for (const match of source.matchAll(/feature\(\s*"[^"]+",\s*"([^"]+)",\s*"([^"]+)"/g)) {
      for (const text of [match[1], match[2]]) {
        if (HAN.test(text)) keys.push(text);
      }
    }
  }
  return keys;
}

function allKeys() {
  const keys = new Set();
  const untranslated = [];
  for (const file of sourceFiles()) {
    const result = translatorKeys(file);
    for (const key of result.keys) keys.add(key);
    for (const text of result.untranslated) {
      untranslated.push(`${path.relative(root, file)}: ${text}`);
    }
  }
  for (const key of fixtureKeys()) keys.add(key);
  return { keys: [...keys].sort(), untranslated };
}

function loadDictionary() {
  return JSON.parse(fs.readFileSync(dictionaryPath, "utf8"));
}

module.exports = { allKeys, loadDictionary, jsxRenderedText };

if (require.main === module) {
  const [command, ...files] = process.argv.slice(2);
  if (command === "wrap") {
    const program = createProgram();
    const targets = files.length > 0 ? files.map((file) => path.resolve(file)) : sourceFiles();
    let total = 0;
    for (const file of targets) {
      const count = wrapFile(program, file);
      if (count) {
        total += count;
        console.log(`${path.relative(root, file)}: ${count}`);
      }
    }
    console.log(`wrapped ${total} strings`);
  } else if (command === "keys") {
    console.log(JSON.stringify(allKeys().keys, null, 2));
  } else if (command === "missing") {
    const dictionary = loadDictionary();
    const { keys, untranslated } = allKeys();
    console.log(JSON.stringify(keys.filter((key) => !dictionary[key]), null, 2));
    if (untranslated.length) {
      console.error(`untranslated literals:\n${untranslated.join("\n")}`);
    }
  } else {
    console.error("usage: i18n.cjs wrap [files...] | keys | missing");
    process.exit(1);
  }
}
