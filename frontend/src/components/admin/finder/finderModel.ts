export type FinderKind =
  | "folder"
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "text"
  | "code"
  | "archive"
  | "sheet"
  | "doc"
  | "file";

export type FinderBadgeTone = "green" | "orange" | "blue" | "gray" | "red";

export type FinderItem = {
  key: string;
  name: string;
  kind: FinderKind;
  sizeBytes?: number | null;
  modifiedAt?: string | null;
  /** Shown in the "种类" column; defaults to the kind label. */
  typeLabel?: string;
  badge?: { label: string; tone: FinderBadgeTone };
  /** Rendered dimmed, e.g. deleted or not yet imported. */
  muted?: boolean;
};

export type FinderSortKey = "name" | "modified" | "size" | "kind";
export type FinderSort = { key: FinderSortKey; order: "asc" | "desc" };

const IMAGE = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "svg",
  "bmp",
  "ico",
  "avif",
  "heic",
]);
const VIDEO = new Set(["mp4", "m4v", "webm", "mov", "avi", "mkv", "ogv"]);
const AUDIO = new Set(["mp3", "wav", "ogg", "m4a", "aac", "flac"]);
const ARCHIVE = new Set(["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz"]);
const SHEET = new Set(["xls", "xlsx", "csv", "numbers"]);
const DOC = new Set(["doc", "docx", "ppt", "pptx", "pages", "key", "rtf"]);
const CODE = new Set([
  "js",
  "jsx",
  "ts",
  "tsx",
  "rs",
  "py",
  "go",
  "java",
  "c",
  "cpp",
  "h",
  "css",
  "scss",
  "html",
  "vue",
  "sh",
  "sql",
  "json",
  "yaml",
  "yml",
  "toml",
  "xml",
]);
const TEXT = new Set(["txt", "md", "markdown", "log", "ini", "conf", "env"]);

export function extensionOf(name: string) {
  const index = name.lastIndexOf(".");
  return index > 0 ? name.slice(index + 1).toLowerCase() : "";
}

export function finderKindOf(options: {
  isFolder?: boolean;
  name?: string;
  extension?: string | null;
  mimeType?: string | null;
}): FinderKind {
  if (options.isFolder) return "folder";
  const extension = (
    options.extension || extensionOf(options.name ?? "")
  ).toLowerCase();
  const mime = options.mimeType?.toLowerCase() ?? "";
  if (mime.startsWith("image/") || IMAGE.has(extension)) return "image";
  if (mime.startsWith("video/") || VIDEO.has(extension)) return "video";
  if (mime.startsWith("audio/") || AUDIO.has(extension)) return "audio";
  if (mime === "application/pdf" || extension === "pdf") return "pdf";
  if (ARCHIVE.has(extension)) return "archive";
  if (SHEET.has(extension)) return "sheet";
  if (DOC.has(extension)) return "doc";
  if (CODE.has(extension)) return "code";
  if (mime.startsWith("text/") || TEXT.has(extension)) return "text";
  return "file";
}

const KIND_LABELS: Record<FinderKind, string> = {
  folder: "文件夹",
  image: "图像",
  video: "视频",
  audio: "音频",
  pdf: "PDF 文稿",
  text: "纯文本",
  code: "源代码",
  archive: "压缩包",
  sheet: "表格",
  doc: "文稿",
  file: "文件",
};

export function kindLabel(item: Pick<FinderItem, "kind" | "typeLabel">) {
  return item.typeLabel ?? KIND_LABELS[item.kind];
}

export function formatBytes(value?: number | null) {
  if (value === undefined || value === null || Number.isNaN(value)) return "--";
  if (value < 1000) return `${value} 字节`;
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1000;
  let unit = 0;
  while (size >= 1000 && unit < units.length - 1) {
    size /= 1000;
    unit += 1;
  }
  return `${size >= 100 ? Math.round(size) : size.toFixed(1)} ${units[unit]}`;
}

function pad(value: number) {
  return String(value).padStart(2, "0");
}

/** Finder-style dates: 今天 14:05 / 昨天 09:12 / 2026/10/01 08:30. */
export function formatModified(value?: string | null, now: Date = new Date()) {
  if (!value) return "--";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "--";
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  const startOfToday = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  );
  const dayDiff = Math.floor(
    (startOfToday.getTime() -
      new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime()) /
      86_400_000,
  );
  if (dayDiff === 0) return `今天 ${time}`;
  if (dayDiff === 1) return `昨天 ${time}`;
  return `${date.getFullYear()}/${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${time}`;
}

export type FinderCrumb = { name: string; path: string };

/** `a/b/c` → [{a, "a"}, {b, "a/b"}, {c, "a/b/c"}]; tolerates slashes at either end. */
export function pathCrumbs(path?: string | null): FinderCrumb[] {
  const parts = (path ?? "").split("/").filter(Boolean);
  return parts.map((name, index) => ({
    name,
    path: parts.slice(0, index + 1).join("/"),
  }));
}

/** Parent of `a/b/c` is `a/b`; the root's parent is null. */
export function parentPathOf(path?: string | null) {
  const parts = (path ?? "").split("/").filter(Boolean);
  if (parts.length === 0) return null;
  return parts.slice(0, -1).join("/");
}

/** Folders first, then by the chosen column; names compare naturally (2 < 10). */
export function sortFinderItems<T extends FinderItem>(
  items: T[],
  sort: FinderSort,
) {
  const direction = sort.order === "asc" ? 1 : -1;
  const byName = (a: T, b: T) =>
    a.name.localeCompare(b.name, "zh-CN", {
      numeric: true,
      sensitivity: "base",
    });
  return [...items].sort((a, b) => {
    const folderA = a.kind === "folder";
    const folderB = b.kind === "folder";
    if (folderA !== folderB) return folderA ? -1 : 1;
    let result = 0;
    if (sort.key === "size") {
      result = (a.sizeBytes ?? -1) - (b.sizeBytes ?? -1);
    } else if (sort.key === "modified") {
      result =
        new Date(a.modifiedAt ?? 0).getTime() -
        new Date(b.modifiedAt ?? 0).getTime();
    } else if (sort.key === "kind") {
      result = kindLabel(a).localeCompare(kindLabel(b), "zh-CN");
    }
    return (result || byName(a, b)) * direction;
  });
}

export type FinderHistory = { entries: string[]; index: number };

/** Records a visit; dropping any "forward" entries like a browser does. */
export function historyVisit(
  history: FinderHistory,
  location: string,
): FinderHistory {
  if (history.entries[history.index] === location) return history;
  const entries = [
    ...history.entries.slice(0, history.index + 1),
    location,
  ].slice(-50);
  return { entries, index: entries.length - 1 };
}

export function historyMove(
  history: FinderHistory,
  step: -1 | 1,
): FinderHistory {
  const index = history.index + step;
  if (index < 0 || index >= history.entries.length) return history;
  return { ...history, index };
}
