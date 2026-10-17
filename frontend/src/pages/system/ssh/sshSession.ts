export type TerminalStatus =
  | "idle"
  | "connecting"
  | "connected"
  | "detached"
  | "closed"
  | "error";

export const SESSION_NOT_FOUND_CODE = "SSH_SESSION_NOT_FOUND";
export const MAX_AUTO_RECONNECT_ATTEMPTS = 3;

/** True when the server no longer has the session (restart or shell exit). */
export function isSessionGone(error: unknown) {
  if (!error || typeof error !== "object") return false;
  const { code, status, message } = error as {
    code?: string;
    status?: number;
    message?: string;
  };
  return (
    code === SESSION_NOT_FOUND_CODE ||
    status === 404 ||
    /ssh session (not found|has ended)/i.test(message ?? "")
  );
}

/** 1s, 2s, 4s … capped at 10s. */
export function reconnectDelay(attempt: number) {
  return Math.min(1000 * 2 ** Math.max(0, attempt), 10_000);
}

export type OverlayAction = "connect" | "reconnect" | "new" | null;

export type StatusOverlay = {
  tone: "idle" | "busy" | "warning" | "error";
  title: string;
  description: string;
  action: OverlayAction;
  actionLabel?: string;
};

/** What to show over the terminal; `null` when the terminal is live. */
export function statusOverlay(
  status: TerminalStatus,
  options: { errorMessage?: string; reconnectAttempt?: number } = {},
): StatusOverlay | null {
  switch (status) {
    case "connected":
      return null;
    case "connecting":
      return {
        tone: "busy",
        title: options.reconnectAttempt
          ? `正在自动重连（第 ${options.reconnectAttempt} 次）`
          : "正在连接",
        description: "稍等片刻，终端即将就绪",
        action: null,
      };
    case "detached":
      return {
        tone: "warning",
        title: "连接已断开",
        description: "网络中断或页面切到了后台，服务器上的会话仍然保留",
        action: "reconnect",
        actionLabel: "重新连接",
      };
    case "closed":
      return {
        tone: "idle",
        title: "会话已结束",
        description: "终端进程已退出或会话已被关闭",
        action: "new",
        actionLabel: "新建连接",
      };
    case "error":
      return {
        tone: "error",
        title: "连接失败",
        description: options.errorMessage || "无法连接到终端，请稍后重试",
        action: "reconnect",
        actionLabel: "重试",
      };
    default:
      return {
        tone: "idle",
        title: "终端未连接",
        description: "选择目标后连接，即可开始使用",
        action: "connect",
        actionLabel: "连接",
      };
  }
}

type SessionRef = {
  serverSessionId?: string;
  status: TerminalStatus;
};

/**
 * Forgets server session ids the server no longer lists, so the next
 * connect creates a fresh session instead of failing. Sessions with a live
 * socket or a connect in flight are left alone.
 */
export function forgetMissingServerSessions<T extends SessionRef>(
  sessions: T[],
  serverSessionIds: Set<string>,
  hasLiveSocket: (session: T) => boolean,
): T[] {
  let changed = false;
  const next = sessions.map((session) => {
    if (
      !session.serverSessionId ||
      serverSessionIds.has(session.serverSessionId) ||
      session.status === "connecting" ||
      hasLiveSocket(session)
    ) {
      return session;
    }
    changed = true;
    return {
      ...session,
      serverSessionId: undefined,
      status: session.status === "detached" ? "idle" : session.status,
    };
  });
  return changed ? next : sessions;
}
