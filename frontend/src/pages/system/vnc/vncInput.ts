import { t } from "../../../i18n";
// Pure VNC input helpers: X11 keysyms, shortcut presets and the trackpad
// gesture model. Kept import-free so the node tests can load them directly.

export type KeyStroke = { keysym: number; code: string };

export type ModifierName = "ctrl" | "alt" | "shift" | "meta";

export const MODIFIER_ORDER: ModifierName[] = ["ctrl", "alt", "shift", "meta"];

export const MODIFIER_LABELS: Record<ModifierName, string> = {
  ctrl: "Ctrl",
  alt: "Alt",
  shift: "Shift",
  meta: "Win",
};

export const KEYS = {
  ctrl: { keysym: 0xffe3, code: "ControlLeft" },
  alt: { keysym: 0xffe9, code: "AltLeft" },
  shift: { keysym: 0xffe1, code: "ShiftLeft" },
  meta: { keysym: 0xffeb, code: "MetaLeft" },
  escape: { keysym: 0xff1b, code: "Escape" },
  tab: { keysym: 0xff09, code: "Tab" },
  enter: { keysym: 0xff0d, code: "Enter" },
  backspace: { keysym: 0xff08, code: "Backspace" },
  delete: { keysym: 0xffff, code: "Delete" },
  insert: { keysym: 0xff63, code: "Insert" },
  home: { keysym: 0xff50, code: "Home" },
  end: { keysym: 0xff57, code: "End" },
  pageUp: { keysym: 0xff55, code: "PageUp" },
  pageDown: { keysym: 0xff56, code: "PageDown" },
  left: { keysym: 0xff51, code: "ArrowLeft" },
  up: { keysym: 0xff52, code: "ArrowUp" },
  right: { keysym: 0xff53, code: "ArrowRight" },
  down: { keysym: 0xff54, code: "ArrowDown" },
  printScreen: { keysym: 0xff61, code: "PrintScreen" },
  space: { keysym: 0x0020, code: "Space" },
} satisfies Record<string, KeyStroke>;

export function functionKey(index: number): KeyStroke {
  // XK_F1 is 0xffbe; F1-F12 are consecutive.
  return { keysym: 0xffbe + index - 1, code: `F${index}` };
}

/** A letter key used inside shortcuts, e.g. the C of Ctrl+C. */
export function letterKey(letter: string): KeyStroke {
  const lower = letter.toLowerCase();
  return { keysym: lower.charCodeAt(0), code: `Key${lower.toUpperCase()}` };
}

/**
 * Keysym for typed text. No physical code is attached so servers that prefer
 * scancodes still receive the exact character (including upper case).
 */
export function charStroke(character: string): KeyStroke | null {
  if (character === "\n" || character === "\r") {
    return KEYS.enter;
  }
  if (character === "\t") {
    return KEYS.tab;
  }
  const codePoint = character.codePointAt(0);
  if (codePoint === undefined || codePoint < 0x20 || codePoint === 0x7f) {
    return null;
  }
  const latin1 =
    (codePoint >= 0x20 && codePoint <= 0x7e) ||
    (codePoint >= 0xa0 && codePoint <= 0xff);
  return { keysym: latin1 ? codePoint : 0x01000000 | codePoint, code: "" };
}

export function textStrokes(text: string): KeyStroke[] {
  const strokes: KeyStroke[] = [];
  let previous = "";
  for (const character of Array.from(text)) {
    // Treat CRLF as one Enter.
    if (character === "\n" && previous === "\r") {
      previous = character;
      continue;
    }
    const stroke = charStroke(character);
    if (stroke) {
      strokes.push(stroke);
    }
    previous = character;
  }
  return strokes;
}

/** Holds every key down in order, then releases them in reverse. */
export function comboSequence(strokes: KeyStroke[]) {
  return [
    ...strokes.map((stroke) => ({ stroke, down: true })),
    ...[...strokes].reverse().map((stroke) => ({ stroke, down: false })),
  ];
}

export function withModifiers(
  modifiers: readonly ModifierName[],
  strokes: KeyStroke[],
): KeyStroke[] {
  const held = MODIFIER_ORDER.filter((name) => modifiers.includes(name)).map(
    (name) => KEYS[name],
  );
  const extra = strokes.filter(
    (stroke) => !held.some((modifier) => modifier.keysym === stroke.keysym),
  );
  return [...held, ...extra];
}

export type VncShortcutGroup = "system" | "edit" | "navigation" | "function";

export type VncShortcut = {
  id: string;
  label: string;
  description: string;
  group: VncShortcutGroup;
  strokes: KeyStroke[];
};

export const SHORTCUT_GROUP_LABELS: Record<VncShortcutGroup, string> = {
  system: t("系统"),
  edit: t("编辑"),
  navigation: t("按键"),
  function: t("功能键"),
};

const ctrl = (letter: string) => [KEYS.ctrl, letterKey(letter)];
const win = (letter: string) => [KEYS.meta, letterKey(letter)];

export const VNC_SHORTCUTS: VncShortcut[] = [
  {
    id: "ctrl-alt-del",
    label: "Ctrl+Alt+Del",
    description: t("安全选项 / 解锁"),
    group: "system",
    strokes: [KEYS.ctrl, KEYS.alt, KEYS.delete],
  },
  {
    id: "task-manager",
    label: "Ctrl+Shift+Esc",
    description: t("任务管理器"),
    group: "system",
    strokes: [KEYS.ctrl, KEYS.shift, KEYS.escape],
  },
  {
    id: "start",
    label: "Win",
    description: t("开始菜单"),
    group: "system",
    strokes: [KEYS.meta],
  },
  {
    id: "desktop",
    label: "Win+D",
    description: t("显示桌面"),
    group: "system",
    strokes: win("d"),
  },
  {
    id: "explorer",
    label: "Win+E",
    description: t("文件资源管理器"),
    group: "system",
    strokes: win("e"),
  },
  {
    id: "run",
    label: "Win+R",
    description: t("运行"),
    group: "system",
    strokes: win("r"),
  },
  {
    id: "lock",
    label: "Win+L",
    description: t("锁定电脑"),
    group: "system",
    strokes: win("l"),
  },
  {
    id: "switch-window",
    label: "Alt+Tab",
    description: t("切换窗口"),
    group: "system",
    strokes: [KEYS.alt, KEYS.tab],
  },
  {
    id: "close-window",
    label: "Alt+F4",
    description: t("关闭窗口"),
    group: "system",
    strokes: [KEYS.alt, functionKey(4)],
  },
  {
    id: "screenshot",
    label: "PrtSc",
    description: t("截屏"),
    group: "system",
    strokes: [KEYS.printScreen],
  },
  {
    id: "copy",
    label: "Ctrl+C",
    description: t("复制"),
    group: "edit",
    strokes: ctrl("c"),
  },
  {
    id: "paste",
    label: "Ctrl+V",
    description: t("粘贴"),
    group: "edit",
    strokes: ctrl("v"),
  },
  {
    id: "cut",
    label: "Ctrl+X",
    description: t("剪切"),
    group: "edit",
    strokes: ctrl("x"),
  },
  {
    id: "undo",
    label: "Ctrl+Z",
    description: t("撤销"),
    group: "edit",
    strokes: ctrl("z"),
  },
  {
    id: "redo",
    label: "Ctrl+Y",
    description: t("重做"),
    group: "edit",
    strokes: ctrl("y"),
  },
  {
    id: "select-all",
    label: "Ctrl+A",
    description: t("全选"),
    group: "edit",
    strokes: ctrl("a"),
  },
  {
    id: "save",
    label: "Ctrl+S",
    description: t("保存"),
    group: "edit",
    strokes: ctrl("s"),
  },
  {
    id: "find",
    label: "Ctrl+F",
    description: t("查找"),
    group: "edit",
    strokes: ctrl("f"),
  },
  ...(
    [
      ["escape", "Esc", t("取消")],
      ["tab", "Tab", t("切换焦点")],
      ["enter", "Enter", t("确认")],
      ["backspace", "Backspace", t("退格")],
      ["delete", "Delete", t("删除")],
      ["insert", "Insert", t("插入")],
      ["home", "Home", t("行首")],
      ["end", "End", t("行尾")],
      ["pageUp", "PgUp", t("上一页")],
      ["pageDown", "PgDn", t("下一页")],
      ["up", "↑", t("上")],
      ["down", "↓", t("下")],
      ["left", "←", t("左")],
      ["right", "→", t("右")],
    ] as const
  ).map(([key, label, description]) => ({
    id: `key-${key}`,
    label,
    description,
    group: "navigation" as const,
    strokes: [KEYS[key]],
  })),
  ...Array.from({ length: 12 }, (_, index) => ({
    id: `f${index + 1}`,
    label: `F${index + 1}`,
    description: "",
    group: "function" as const,
    strokes: [functionKey(index + 1)],
  })),
];

export type Point = { x: number; y: number };

export type TrackpadAction =
  | { type: "move"; x: number; y: number; buttons: number }
  | { type: "down"; x: number; y: number; button: number }
  | { type: "up"; x: number; y: number; button: number }
  | { type: "click"; x: number; y: number; button: number }
  | { type: "scroll"; x: number; y: number; deltaX: number; deltaY: number };

export type TrackpadOptions = {
  /** Pointer travel per finger pixel. */
  speed: number;
  /** Wheel pixels per two-finger pixel. */
  scrollSpeed: number;
  /** Finger travel (px) below which a touch still counts as a tap. */
  tapSlop: number;
  /** Longest touch (ms) that still counts as a tap. */
  tapMaxMs: number;
  /** Holding still this long (ms) starts a left-button drag. */
  longPressMs: number;
};

export const DEFAULT_TRACKPAD_OPTIONS: TrackpadOptions = {
  speed: 1.6,
  scrollSpeed: 1.5,
  tapSlop: 8,
  tapMaxMs: 260,
  longPressMs: 450,
};

export const LEFT_BUTTON = 0;
export const RIGHT_BUTTON = 2;

function centroid(points: readonly Point[]): Point {
  const total = points.reduce(
    (sum, point) => ({ x: sum.x + point.x, y: sum.y + point.y }),
    { x: 0, y: 0 },
  );
  return { x: total.x / points.length, y: total.y / points.length };
}

/**
 * Laptop-trackpad style touch control, like remote-desktop apps on phones:
 * one finger moves a visible cursor, a tap clicks where the cursor is, a
 * two-finger tap right-clicks, two fingers scroll and a long press drags.
 */
export class TrackpadGesture {
  readonly options: TrackpadOptions;
  cursor: Point = { x: 0, y: 0 };
  private width = 0;
  private height = 0;
  private active = false;
  private dragging = false;
  private travel = 0;
  private maxTouches = 0;
  private startedAt = 0;
  private last: Point = { x: 0, y: 0 };

  constructor(options: Partial<TrackpadOptions> = {}) {
    this.options = { ...DEFAULT_TRACKPAD_OPTIONS, ...options };
  }

  get isDragging() {
    return this.dragging;
  }

  get hasMoved() {
    return this.travel > this.options.tapSlop;
  }

  /**
   * Sets the on-screen size of the remote display. When the display is
   * rescaled the cursor is scaled with it, so it stays on the same remote pixel.
   */
  setBounds(width: number, height: number, center = false) {
    const nextWidth = Math.max(0, width);
    const nextHeight = Math.max(0, height);
    if (center || this.width === 0 || this.height === 0) {
      this.cursor = { x: nextWidth / 2, y: nextHeight / 2 };
    } else {
      this.cursor = {
        x: (this.cursor.x * nextWidth) / this.width,
        y: (this.cursor.y * nextHeight) / this.height,
      };
    }
    this.width = nextWidth;
    this.height = nextHeight;
    this.cursor = this.clamp(this.cursor);
  }

  start(points: readonly Point[], now: number): TrackpadAction[] {
    if (points.length === 0) {
      return [];
    }
    if (!this.active) {
      this.active = true;
      this.travel = 0;
      this.maxTouches = 0;
      this.startedAt = now;
    }
    this.maxTouches = Math.max(this.maxTouches, points.length);
    // A finger joining or leaving shifts the centroid; never treat that as movement.
    this.last = centroid(points);
    return [];
  }

  move(points: readonly Point[]): TrackpadAction[] {
    if (!this.active || points.length === 0) {
      return [];
    }
    const next = centroid(points);
    const dx = next.x - this.last.x;
    const dy = next.y - this.last.y;
    this.last = next;
    this.travel += Math.hypot(dx, dy);
    if (points.length >= 2 && !this.dragging) {
      if (!this.hasMoved) {
        return [];
      }
      return [
        {
          type: "scroll",
          ...this.cursor,
          deltaX: -dx * this.options.scrollSpeed,
          deltaY: -dy * this.options.scrollSpeed,
        },
      ];
    }
    this.cursor = this.clamp({
      x: this.cursor.x + dx * this.options.speed,
      y: this.cursor.y + dy * this.options.speed,
    });
    return [{ type: "move", ...this.cursor, buttons: this.dragging ? 1 : 0 }];
  }

  /** Called once the finger has been held still for `longPressMs`. */
  longPress(): TrackpadAction[] {
    if (!this.active || this.dragging || this.hasMoved || this.maxTouches > 1) {
      return [];
    }
    this.dragging = true;
    return [{ type: "down", ...this.cursor, button: LEFT_BUTTON }];
  }

  /**
   * `remaining` are the fingers still down. `tapButton` is the button a
   * one-finger tap sends (right while the one-shot right click is armed).
   */
  end(
    remaining: readonly Point[],
    now: number,
    tapButton = LEFT_BUTTON,
  ): TrackpadAction[] {
    if (!this.active) {
      return [];
    }
    if (remaining.length > 0) {
      this.last = centroid(remaining);
      return [];
    }
    this.active = false;
    if (this.dragging) {
      this.dragging = false;
      return [{ type: "up", ...this.cursor, button: LEFT_BUTTON }];
    }
    if (this.hasMoved || now - this.startedAt > this.options.tapMaxMs) {
      return [];
    }
    return [
      {
        type: "click",
        ...this.cursor,
        button: this.maxTouches >= 2 ? RIGHT_BUTTON : tapButton,
      },
    ];
  }

  cancel(): TrackpadAction[] {
    const wasDragging = this.dragging;
    this.active = false;
    this.dragging = false;
    return wasDragging
      ? [{ type: "up", ...this.cursor, button: LEFT_BUTTON }]
      : [];
  }

  private clamp(point: Point): Point {
    return {
      x: Math.min(Math.max(point.x, 0), Math.max(this.width - 1, 0)),
      y: Math.min(Math.max(point.y, 0), Math.max(this.height - 1, 0)),
    };
  }
}
