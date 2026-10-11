import type {
  CliArgSpec,
  CliCommandSpec,
  CliValue,
} from "../../../api/admin/projectCli";

export type CliValues = Record<string, CliValue | undefined>;

export type CommandSection = {
  source: string;
  title: string;
  commands: { command: CliCommandSpec; depth: number }[];
};

const SECTION_TITLES: Record<string, string> = {
  loco: "Loco 内置命令",
  task: "自定义任务",
  service: "系统服务",
  vnc: "VNC",
  data: "数据导入导出",
};
const SECTION_ORDER = ["loco", "task", "service", "vnc", "data"];

export const SECRET_MASK = "******";

export function commandKey(path: string[]) {
  return path.join(" ");
}

function matches(command: CliCommandSpec, keyword: string) {
  if (!keyword) return true;
  const haystack = [
    commandKey(command.path),
    command.about,
    command.note ?? "",
    ...command.args.map((arg) => `${arg.long ?? ""} ${arg.id} ${arg.help}`),
  ]
    .join(" ")
    .toLowerCase();
  return keyword
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((word) => haystack.includes(word));
}

/**
 * Groups commands by where they come from. Searching keeps the parents of
 * matching commands so the tree still reads correctly.
 */
export function groupCommands(
  commands: CliCommandSpec[],
  keyword = "",
): CommandSection[] {
  const keep = new Set<string>();
  for (const command of commands) {
    if (!matches(command, keyword.trim())) continue;
    for (let length = 1; length <= command.path.length; length += 1) {
      keep.add(commandKey(command.path.slice(0, length)));
    }
  }
  const sections = new Map<string, CommandSection>();
  for (const command of commands) {
    if (!keep.has(commandKey(command.path))) continue;
    let section = sections.get(command.source);
    if (!section) {
      section = {
        source: command.source,
        title: SECTION_TITLES[command.source] ?? command.source,
        commands: [],
      };
      sections.set(command.source, section);
    }
    section.commands.push({ command, depth: 0 });
  }
  for (const section of sections.values()) {
    const shortest = Math.min(
      ...section.commands.map(({ command }) => command.path.length),
    );
    for (const entry of section.commands) {
      entry.depth = entry.command.path.length - shortest;
    }
  }
  const rank = (source: string) => {
    const index = SECTION_ORDER.indexOf(source);
    return index < 0 ? SECTION_ORDER.length : index;
  };
  return [...sections.values()].sort((a, b) => rank(a.source) - rank(b.source));
}

export function argLabel(arg: CliArgSpec) {
  if (arg.kind === "task_param") return `${arg.id}:<值>`;
  if (arg.kind === "positional") {
    const name = arg.value_name ?? arg.id.toUpperCase();
    return `${arg.required ? `<${name}>` : `[${name}]`}${arg.multiple ? "..." : ""}`;
  }
  const names = [
    arg.short ? `-${arg.short}` : null,
    arg.long ? `--${arg.long}` : null,
  ]
    .filter(Boolean)
    .join(", ");
  return arg.kind === "option" && arg.value_name
    ? `${names} <${arg.value_name}>`
    : names;
}

function optionName(arg: CliArgSpec) {
  return arg.long ? `--${arg.long}` : `-${arg.short ?? arg.id}`;
}

/** Form defaults: every `--environment` starts at this server's environment. */
export function initialValues(
  command: CliCommandSpec,
  environment: string,
): CliValues {
  const values: CliValues = {};
  for (const arg of command.args) {
    if (arg.kind === "option" && arg.long === "environment") {
      values[arg.id] = environment;
    }
  }
  return values;
}

function textValues(value: CliValue | undefined): string[] {
  if (value === undefined || typeof value === "boolean") return [];
  const list = Array.isArray(value) ? value : [value];
  return list.map((item) => String(item).trim()).filter(Boolean);
}

/** Labels of required arguments that have no value yet. */
export function missingRequired(command: CliCommandSpec, values: CliValues) {
  return command.args
    .filter(
      (arg) =>
        arg.required &&
        arg.kind !== "flag" &&
        textValues(values[arg.id]).length === 0,
    )
    .map(argLabel);
}

/**
 * Arguments after the binary, in the order the server builds them; secret
 * values are masked. The server re-validates everything.
 */
export function buildArgs(
  command: CliCommandSpec,
  values: CliValues,
  extraArgs: string[] = [],
): string[] {
  const args = [...command.path];
  const positionals: string[] = [];
  const taskParams: string[] = [];
  for (const arg of command.args) {
    const value = values[arg.id];
    const mask = (text: string) => (arg.secret ? SECRET_MASK : text);
    if (arg.kind === "flag") {
      if (value === true) args.push(optionName(arg));
      continue;
    }
    if (arg.kind === "option" && value === true && arg.optional_value) {
      args.push(optionName(arg));
      continue;
    }
    for (const text of textValues(value)) {
      if (arg.kind === "option") {
        if (arg.long) args.push(`--${arg.long}=${mask(text)}`);
        else args.push(optionName(arg), mask(text));
      } else if (arg.kind === "positional") {
        positionals.push(mask(text));
      } else {
        taskParams.push(`${arg.id}:${mask(text)}`);
      }
    }
  }
  return [...args, ...positionals, ...taskParams, ...extraArgs];
}

/** Values to send: drops empty fields so the server only sees what was set. */
export function payloadValues(command: CliCommandSpec, values: CliValues) {
  const payload: Record<string, CliValue> = {};
  for (const arg of command.args) {
    const value = values[arg.id];
    if (value === undefined || value === false) continue;
    if (value === true) {
      payload[arg.id] = true;
      continue;
    }
    const texts = textValues(value);
    if (texts.length === 0) continue;
    payload[arg.id] = arg.multiple ? texts : texts[0];
  }
  return payload;
}

/** Splits extra arguments like a shell would: spaces, quotes, backslashes. */
export function splitArgs(text: string): string[] {
  const args: string[] = [];
  let current = "";
  let quote: '"' | "'" | null = null;
  let started = false;
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index];
    if (quote) {
      if (char === quote) quote = null;
      else if (char === "\\" && quote === '"' && index + 1 < text.length) {
        index += 1;
        current += text[index];
      } else current += char;
    } else if (char === '"' || char === "'") {
      quote = char;
      started = true;
    } else if (char === "\\" && index + 1 < text.length) {
      index += 1;
      current += text[index];
      started = true;
    } else if (/\s/.test(char)) {
      if (started) args.push(current);
      current = "";
      started = false;
    } else {
      current += char;
      started = true;
    }
  }
  if (started) args.push(current);
  return args;
}

export function formatCommandLine(program: string, args: string[]) {
  const quote = (value: string) =>
    /^[A-Za-z0-9_@%+=:,./*-]+$/.test(value)
      ? value
      : `'${value.replace(/'/g, "'\\''")}'`;
  return [program, ...args.map(quote)].join(" ");
}

const ANSI_PATTERN = new RegExp(
  `${String.fromCharCode(27)}\\[[0-9;?]*[A-Za-z]`,
  "g",
);

export function stripAnsi(text: string) {
  return text.replace(ANSI_PATTERN, "");
}

export function formatDuration(ms?: number | null) {
  if (ms === undefined || ms === null) return "";
  if (ms < 1000) return `${ms} ms`;
  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} 秒`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes} 分 ${Math.round(seconds % 60)} 秒`;
}
