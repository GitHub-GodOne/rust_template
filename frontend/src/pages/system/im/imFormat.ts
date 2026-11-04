import type { ImConversation, ImMessage } from "../../../api/admin/im";
import { t } from "../../../i18n";

type GroupEvent = {
  action?: string;
  actor?: string;
  targets?: string[];
  name?: string | null;
};

/** Renders a `system` message (a stored group event) in the viewer's language. */
export function systemText(content: string) {
  let event: GroupEvent;
  try {
    event = JSON.parse(content) as GroupEvent;
  } catch {
    return content;
  }
  const actor = event.actor ?? "";
  const targets = (event.targets ?? []).join(t("、"));
  switch (event.action) {
    case "created":
      return targets
        ? t("{0} 创建了群聊，并邀请 {1} 加入", actor, targets)
        : t("{0} 创建了群聊", actor);
    case "invited":
      return t("{0} 邀请 {1} 加入了群聊", actor, targets);
    case "removed":
      return t("{0} 将 {1} 移出了群聊", actor, targets);
    case "left":
      return t("{0} 退出了群聊", actor);
    case "renamed":
      return t("{0} 将群名修改为“{1}”", actor, event.name ?? "");
    default:
      return t("群聊有新动态");
  }
}

/** One-line summary for the conversation list. */
export function messagePreview(
  message: ImMessage | null | undefined,
  conversation: Pick<ImConversation, "kind" | "viewer_id">,
) {
  if (!message) return "";
  if (message.kind === "system") return systemText(message.content);
  const body =
    message.kind === "image"
      ? t("[图片]")
      : message.kind === "file"
        ? t("[文件] {0}", message.file_name ?? "")
        : message.content.replace(/\s+/g, " ");
  if (conversation.kind !== "group" || !message.sender_name) return body;
  const sender =
    message.sender_id === conversation.viewer_id
      ? t("我")
      : message.sender_name;
  return `${sender}: ${body}`;
}

function sameDay(a: Date, b: Date) {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function clock(date: Date) {
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

/** Short time for the chat list: 14:05, 昨天, 10-21, 2025-10-21. */
export function listTime(value?: string | null, now = new Date()) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  if (sameDay(date, now)) return clock(date);
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (sameDay(date, yesterday)) return t("昨天");
  const monthDay = `${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  return date.getFullYear() === now.getFullYear()
    ? monthDay
    : `${date.getFullYear()}-${monthDay}`;
}

/** Divider shown above a message that comes 5+ minutes after the previous one. */
export function dividerTime(
  current: string,
  previous?: string,
  now = new Date(),
) {
  const date = new Date(current);
  if (
    previous &&
    date.getTime() - new Date(previous).getTime() < 5 * 60 * 1000
  ) {
    return null;
  }
  if (sameDay(date, now)) return clock(date);
  return `${listTime(current, now)} ${clock(date)}`;
}

const AVATAR_COLORS = [
  "#c15f3c",
  "#3b82f6",
  "#16a34a",
  "#9333ea",
  "#d97706",
  "#0891b2",
  "#db2777",
  "#4b5563",
];

export function avatarColor(seed: number) {
  return AVATAR_COLORS[Math.abs(seed) % AVATAR_COLORS.length];
}

export function initial(name?: string | null) {
  const trimmed = name?.trim();
  return trimmed ? Array.from(trimmed)[0].toUpperCase() : "?";
}
