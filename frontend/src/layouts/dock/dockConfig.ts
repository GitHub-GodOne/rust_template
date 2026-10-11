export type DockPosition = "bottom" | "left" | "right";
export type DockSize = "small" | "medium" | "large";

/** How the dock behaves on one page. */
export type DockPageMode = "show" | "autoHide" | "hide";

/** A page's overrides; a missing field follows the global default. */
export type DockPageRule = {
  path: string;
  mode?: DockPageMode;
  /** Leave blank space for an always-visible dock on this page. */
  reserve?: boolean;
};

export type DockPageRulePatch = {
  /** `null` removes the override. */
  mode?: DockPageMode | null;
  reserve?: boolean | null;
};

export type DockItemConfig = {
  path: string;
  color: string;
};

export type DockConfig = {
  version: 1;
  /** Show the dock at all. */
  enabled: boolean;
  items: DockItemConfig[];
  position: DockPosition;
  size: DockSize;
  /** Enlarge icons under the pointer. */
  magnify: boolean;
  /** Default for pages without a rule: slide away until the pointer reaches the screen edge. */
  autoHide: boolean;
  /** Default for pages without a rule: keep page content clear of a visible dock. */
  reserveSpace: boolean;
  /** Per-page overrides of the default behaviour. */
  pageRules: DockPageRule[];
  /** Page names under the icons. */
  showLabels: boolean;
};

export const MAX_DOCK_ITEMS = 16;
export const MAX_DOCK_PAGE_RULES = 64;

export const DOCK_PAGE_MODE_LABELS: Record<DockPageMode, string> = {
  show: "常驻显示",
  autoHide: "自动隐藏",
  hide: "不显示 Dock",
};

export const DOCK_COLORS = [
  "#5f625f",
  "#c86743",
  "#4f7f9f",
  "#666b73",
  "#313336",
  "#8b8177",
  "#4f735b",
  "#b7791f",
  "#7a5c99",
  "#2f7bb0",
  "#b84a46",
  "#3d8a8a",
];

export const DOCK_SIZES: Record<
  DockSize,
  { item: number; icon: number; font: number }
> = {
  small: { item: 40, icon: 34, font: 16 },
  medium: { item: 50, icon: 43, font: 20 },
  large: { item: 60, icon: 52, font: 24 },
};

export const DEFAULT_DOCK_CONFIG: DockConfig = {
  version: 1,
  enabled: true,
  items: [
    { path: "/admin/dashboard", color: "#5f625f" },
    { path: "/admin/system/uploads", color: "#c86743" },
    { path: "/admin/system/files", color: "#4f7f9f" },
    { path: "/admin/system/commands", color: "#666b73" },
    { path: "/admin/system/ssh", color: "#313336" },
    { path: "/admin/system/settings", color: "#8b8177" },
  ],
  position: "bottom",
  size: "medium",
  magnify: true,
  autoHide: false,
  reserveSpace: true,
  pageRules: [],
  showLabels: false,
};

const HEX_COLOR = /^#[0-9a-f]{6}$/i;

/** Stable default color for a page, so a newly added icon looks the same everywhere. */
export function colorForPath(path: string) {
  let hash = 0;
  for (const char of path) {
    hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  }
  return DOCK_COLORS[hash % DOCK_COLORS.length];
}

function pick<T extends string>(
  value: unknown,
  allowed: readonly T[],
  fallback: T,
): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

/**
 * Turns whatever was stored (older versions, hand-edited JSON, nothing)
 * into a valid config: unknown fields are dropped, duplicates removed and
 * the item count capped.
 */
export function normalizeDockConfig(raw: unknown): DockConfig {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) {
    return structuredCloneConfig(DEFAULT_DOCK_CONFIG);
  }
  const source = raw as Record<string, unknown>;
  const seen = new Set<string>();
  const items: DockItemConfig[] = [];
  const rawItems = Array.isArray(source.items)
    ? source.items
    : DEFAULT_DOCK_CONFIG.items;
  for (const entry of rawItems) {
    if (!entry || typeof entry !== "object") continue;
    const { path, color } = entry as Record<string, unknown>;
    if (
      typeof path !== "string" ||
      !path.startsWith("/admin/") ||
      seen.has(path)
    ) {
      continue;
    }
    seen.add(path);
    items.push({
      path,
      color:
        typeof color === "string" && HEX_COLOR.test(color)
          ? color.toLowerCase()
          : colorForPath(path),
    });
    if (items.length >= MAX_DOCK_ITEMS) break;
  }
  const seenRules = new Set<string>();
  const pageRules: DockPageRule[] = [];
  for (const entry of Array.isArray(source.pageRules) ? source.pageRules : []) {
    if (!entry || typeof entry !== "object") continue;
    const { path, mode, reserve } = entry as Record<string, unknown>;
    if (
      typeof path !== "string" ||
      !path.startsWith("/admin/") ||
      seenRules.has(path)
    ) {
      continue;
    }
    const rule: DockPageRule = { path };
    if (["show", "autoHide", "hide"].includes(mode as string)) {
      rule.mode = mode as DockPageMode;
    }
    if (typeof reserve === "boolean") {
      rule.reserve = reserve;
    }
    // A rule that overrides nothing is dropped.
    if (rule.mode === undefined && rule.reserve === undefined) continue;
    seenRules.add(path);
    pageRules.push(rule);
    if (pageRules.length >= MAX_DOCK_PAGE_RULES) break;
  }
  const flag = (key: keyof DockConfig) =>
    typeof source[key] === "boolean"
      ? (source[key] as boolean)
      : (DEFAULT_DOCK_CONFIG[key] as boolean);
  return {
    version: 1,
    enabled: flag("enabled"),
    items,
    position: pick(
      source.position,
      ["bottom", "left", "right"] as const,
      "bottom",
    ),
    size: pick(source.size, ["small", "medium", "large"] as const, "medium"),
    magnify: flag("magnify"),
    autoHide: flag("autoHide"),
    reserveSpace: flag("reserveSpace"),
    pageRules,
    showLabels: flag("showLabels"),
  };
}

/** The behaviour on `pathname`: its own rule, else the global default. */
export function dockModeForPath(
  config: DockConfig,
  pathname: string,
): DockPageMode {
  const rule = config.pageRules.find((entry) => entry.path === pathname);
  if (rule?.mode) return rule.mode;
  return config.autoHide ? "autoHide" : "show";
}

/**
 * Whether page content leaves room for the dock on `pathname`. Only an
 * always-shown dock takes space; auto-hidden or hidden docks never do.
 */
export function dockReservesSpaceForPath(config: DockConfig, pathname: string) {
  if (dockModeForPath(config, pathname) !== "show") return false;
  const rule = config.pageRules.find((entry) => entry.path === pathname);
  return rule?.reserve ?? config.reserveSpace;
}

/** Sets (or with `null`, removes) the rule for one page. */
export function withPageRule(
  config: DockConfig,
  path: string,
  patch: DockPageRulePatch | null,
): DockConfig {
  const index = config.pageRules.findIndex((entry) => entry.path === path);
  const previous = index >= 0 ? config.pageRules[index] : undefined;
  const pick = <T>(next: T | null | undefined, old: T | undefined) =>
    patch === null || next === null ? undefined : (next ?? old);
  const mode = pick(patch?.mode, previous?.mode);
  const reserve = pick(patch?.reserve, previous?.reserve);
  // Only store the fields that are set, so saved JSON stays tidy.
  const current: DockPageRule = {
    path,
    ...(mode === undefined ? {} : { mode }),
    ...(reserve === undefined ? {} : { reserve }),
  };
  const keep = current.mode !== undefined || current.reserve !== undefined;
  const pageRules = [...config.pageRules];
  if (index >= 0) {
    if (keep) pageRules[index] = current;
    else pageRules.splice(index, 1);
  } else if (keep) {
    pageRules.push(current);
  }
  return { ...config, pageRules };
}

function structuredCloneConfig(config: DockConfig): DockConfig {
  return {
    ...config,
    items: config.items.map((item) => ({ ...item })),
    pageRules: config.pageRules.map((rule) => ({ ...rule })),
  };
}

export function moveDockItem(
  items: DockItemConfig[],
  from: number,
  to: number,
) {
  if (
    from === to ||
    from < 0 ||
    to < 0 ||
    from >= items.length ||
    to >= items.length
  ) {
    return items;
  }
  const next = [...items];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return next;
}

export function sameDockConfig(a: DockConfig, b: DockConfig) {
  return JSON.stringify(a) === JSON.stringify(b);
}
