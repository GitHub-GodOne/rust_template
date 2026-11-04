/** A user's saved terminal command, stored in their `ssh_commands` preference. */
export type SshQuickCommand = {
  id: string;
  name: string;
  command: string;
  /** Send Enter after the text so it runs at once; off leaves it to edit. */
  auto_enter: boolean;
  /** Run in every newly opened terminal session, before anything typed. */
  run_on_connect: boolean;
};

export type SshCommandsPreference = {
  version: 1;
  commands: SshQuickCommand[];
};

export const SSH_COMMANDS_PREFERENCE_KEY = "ssh_commands";
export const MAX_SSH_COMMANDS = 50;
export const MAX_SSH_COMMAND_NAME_LENGTH = 40;
export const MAX_SSH_COMMAND_LENGTH = 1000;
/** The server rejects preference values above 16 KiB (UTF-8 bytes). */
const MAX_PREFERENCE_BYTES = 16 * 1024;

/** Reads a stored preference, dropping malformed entries instead of failing. */
export function normalizeSshCommands(value: unknown): SshQuickCommand[] {
  const list =
    value && typeof value === "object"
      ? (value as { commands?: unknown }).commands
      : undefined;
  if (!Array.isArray(list)) return [];
  const seen = new Set<string>();
  const commands: SshQuickCommand[] = [];
  for (const item of list) {
    if (!item || typeof item !== "object") continue;
    const { id, name, command, auto_enter, run_on_connect } = item as Record<
      string,
      unknown
    >;
    if (typeof id !== "string" || !id || seen.has(id)) continue;
    if (typeof command !== "string" || !command.trim()) continue;
    seen.add(id);
    commands.push({
      id,
      name:
        typeof name === "string" && name.trim()
          ? name.trim().slice(0, MAX_SSH_COMMAND_NAME_LENGTH)
          : command.trim().slice(0, MAX_SSH_COMMAND_NAME_LENGTH),
      command: command.slice(0, MAX_SSH_COMMAND_LENGTH),
      auto_enter: auto_enter !== false,
      run_on_connect: run_on_connect === true,
    });
    if (commands.length >= MAX_SSH_COMMANDS) break;
  }
  return commands;
}

export function toSshCommandsPreference(
  commands: SshQuickCommand[],
): SshCommandsPreference {
  return { version: 1, commands };
}

/** True when the list still fits in one stored preference. */
export function fitsPreferenceLimit(commands: SshQuickCommand[]) {
  const json = JSON.stringify({ value: toSshCommandsPreference(commands) });
  return new TextEncoder().encode(json).length < MAX_PREFERENCE_BYTES;
}

/** Terminal input for a command: one Enter per line, plus a final one if set. */
export function sshCommandInput(command: SshQuickCommand) {
  const text = command.command.replace(/\r\n?|\n/g, "\r").replace(/\r+$/, "");
  return command.auto_enter ? `${text}\r` : text;
}

/**
 * Terminal input for a new session: every `run_on_connect` command in list
 * order, each executed (Enter is always sent so commands do not run together).
 */
export function sshConnectInput(commands: SshQuickCommand[]) {
  return commands
    .filter((command) => command.run_on_connect)
    .map((command) => sshCommandInput({ ...command, auto_enter: true }))
    .join("");
}

/** Moves one entry up (-1) or down (+1); out-of-range moves are a no-op. */
export function moveSshCommand(
  commands: SshQuickCommand[],
  index: number,
  offset: -1 | 1,
) {
  const target = index + offset;
  if (index < 0 || index >= commands.length) return commands;
  if (target < 0 || target >= commands.length) return commands;
  const next = [...commands];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}
