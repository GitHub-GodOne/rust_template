import {
  ArrowLeftOutlined,
  ClearOutlined,
  CloseOutlined,
  CodeOutlined,
  CopyOutlined,
  DisconnectOutlined,
  FolderOpenOutlined,
  LinkOutlined,
  MoreOutlined,
  PlusOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import type { IDisposable } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useMutation, useQuery } from "@tanstack/react-query";
import {
  Button,
  Card,
  Drawer,
  Empty,
  Modal,
  Select,
  Space,
  Spin,
  Tabs,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import type { TabsProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useNavigate } from "react-router-dom";
import {
  type SshFileRecord,
  buildSshSessionWebSocketUrl,
  closeSshSession,
  createSshSession,
  createSshSessionTicket,
  fetchSshSessionFiles,
  fetchSshSessions,
  fetchSshTargets,
  previewSshSessionFile,
} from "../../../api/admin/ssh";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { createClientId } from "../../../utils/clientId";

type TerminalStatus =
  | "idle"
  | "connecting"
  | "connected"
  | "detached"
  | "closed"
  | "error";

type ServerTerminalMessage = {
  type: string;
  data?: string;
};

type SshSession = {
  id: string;
  serverSessionId?: string;
  targetKey: string;
  status: TerminalStatus;
  terminalSize: { cols: number; rows: number };
};

type SpecialKey = {
  label: string;
  data: string;
};

type SessionRuntime = {
  terminal: Terminal;
  fitAddon: FitAddon;
  inputDisposable: IDisposable;
  socket: WebSocket | null;
  host: HTMLDivElement | null;
  resizeObserver: ResizeObserver | null;
};

type InitialTerminalState = {
  sessions: SshSession[];
  activeSessionId: string;
};

const DEFAULT_TERMINAL_SIZE = { cols: 120, rows: 32 };
const specialKeys: SpecialKey[] = [
  { label: "Tab", data: "\t" },
  { label: "Esc", data: "\x1b" },
  { label: "↑", data: "\x1b[A" },
  { label: "↓", data: "\x1b[B" },
  { label: "←", data: "\x1b[D" },
  { label: "→", data: "\x1b[C" },
  { label: "Ctrl-C", data: "\x03" },
  { label: "Ctrl-D", data: "\x04" },
  { label: "Ctrl-L", data: "\x0c" },
  { label: "Enter", data: "\r" },
];
const sshTextExtensions = new Set([
  "txt",
  "md",
  "markdown",
  "json",
  "yaml",
  "yml",
  "toml",
  "xml",
  "csv",
  "log",
  "ini",
  "conf",
  "rs",
  "js",
  "jsx",
  "ts",
  "tsx",
  "css",
  "html",
  "sh",
  "sql",
]);

function sshPreviewKind(record: SshFileRecord) {
  const extension = record.extension?.toLowerCase() ?? "";
  if (sshTextExtensions.has(extension)) {
    return "text";
  }
  if (["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].includes(extension)) {
    return "image";
  }
  if (["mp4", "webm", "mov", "m4v", "ogv"].includes(extension)) {
    return "video";
  }
  if (["mp3", "wav", "ogg", "m4a", "aac", "flac"].includes(extension)) {
    return "audio";
  }
  if (extension === "pdf") {
    return "pdf";
  }
  return "unknown";
}
const sshAutoConnectKey = "gpt-images-admin-ssh-auto-connect";
const sshTargetKeyStorageKey = "gpt-images-admin-ssh-target";
const sshSessionsStorageKey = "gpt-images-admin-ssh-sessions";
const sshActiveSessionStorageKey = "gpt-images-admin-ssh-active-session";

function loadPersistedSessions(): SshSession[] {
  if (typeof localStorage === "undefined") {
    return [];
  }
  try {
    const raw = localStorage.getItem(sshSessionsStorageKey);
    if (!raw) {
      return [];
    }
    const sessions = JSON.parse(raw) as Partial<SshSession>[];
    return sessions
      .filter((session) => typeof session?.id === "string")
      .map((session) => ({
        id: session.id as string,
        serverSessionId:
          typeof session.serverSessionId === "string"
            ? session.serverSessionId
            : undefined,
        targetKey:
          typeof session.targetKey === "string" && session.targetKey.trim()
            ? session.targetKey
            : "local-shell",
        status: "idle",
        terminalSize: {
          cols:
            typeof session.terminalSize?.cols === "number"
              ? session.terminalSize.cols
              : DEFAULT_TERMINAL_SIZE.cols,
          rows:
            typeof session.terminalSize?.rows === "number"
              ? session.terminalSize.rows
              : DEFAULT_TERMINAL_SIZE.rows,
        },
      }));
  } catch {
    return [];
  }
}

function persistSessions(sessions: SshSession[], activeSessionId: string) {
  if (typeof localStorage === "undefined") {
    return;
  }
  localStorage.setItem(
    sshSessionsStorageKey,
    JSON.stringify(
      sessions.map((session) => ({
        id: session.id,
        serverSessionId: session.serverSessionId,
        targetKey: session.targetKey,
        terminalSize: session.terminalSize,
      })),
    ),
  );
  localStorage.setItem(sshActiveSessionStorageKey, activeSessionId);
}

function statusColor(status: TerminalStatus) {
  if (status === "connected") {
    return "green";
  }
  if (status === "connecting") {
    return "processing";
  }
  if (status === "detached") {
    return "blue";
  }
  if (status === "error") {
    return "red";
  }
  return "default";
}

function statusLabel(status: TerminalStatus) {
  if (status === "connected") {
    return "已连接";
  }
  if (status === "connecting") {
    return "连接中";
  }
  if (status === "closed") {
    return "已关闭";
  }
  if (status === "detached") {
    return "已断开本机";
  }
  if (status === "error") {
    return "异常";
  }
  return "未连接";
}

function sessionId() {
  return createClientId("ssh-session");
}

function formatBytes(value: number) {
  if (value < 1024) {
    return `${value} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let size = value / 1024;
  let unitIndex = 0;
  while (size >= 1024 && unitIndex < units.length - 1) {
    size /= 1024;
    unitIndex += 1;
  }
  return `${size.toFixed(size >= 10 ? 1 : 2)} ${units[unitIndex]}`;
}

function terminalBufferText(terminal: Terminal) {
  const buffer = terminal.buffer.active;
  const lines: string[] = [];

  for (let index = 0; index < buffer.length; index += 1) {
    const line = buffer.getLine(index);
    if (!line) {
      continue;
    }
    const text = line.translateToString(true);
    if (line.isWrapped && lines.length > 0) {
      lines[lines.length - 1] += text;
    } else {
      lines.push(text);
    }
  }

  while (lines.at(-1) === "") {
    lines.pop();
  }
  return lines.join("\n");
}

async function writeClipboardText(text: string) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return;
    }
  } catch {
    // LAN deployments often run over HTTP, where the Clipboard API is blocked.
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.readOnly = true;
  textarea.style.position = "fixed";
  textarea.style.inset = "0 auto auto -9999px";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.focus({ preventScroll: true });
  textarea.select();
  textarea.setSelectionRange(0, text.length);
  const copied = document.execCommand("copy");
  textarea.remove();
  if (!copied) {
    throw new Error("当前浏览器不允许读取剪贴板");
  }
}

function parentPath(path: string) {
  const normalized = path.replace(/\/+$/g, "");
  const index = normalized.lastIndexOf("/");
  if (index <= 0) {
    return "/";
  }
  return normalized.slice(0, index);
}

type SshTerminalHostProps = {
  sessionId: string;
  active: boolean;
  onBind: (sessionId: string, host: HTMLDivElement | null) => void;
  onFocus: (sessionId: string) => void;
};

const SshTerminalHost = memo(function SshTerminalHost({
  sessionId,
  active,
  onBind,
  onFocus,
}: SshTerminalHostProps) {
  const bindRef = useCallback(
    (host: HTMLDivElement | null) => {
      onBind(sessionId, host);
    },
    [onBind, sessionId],
  );

  const handleMouseDown = useCallback(() => {
    onFocus(sessionId);
  }, [onFocus, sessionId]);

  return (
    <div
      ref={bindRef}
      className="ssh-terminal-host"
      onMouseDown={handleMouseDown}
      style={{ display: active ? "block" : "none" }}
    />
  );
});

function loadInitialTerminalState(): InitialTerminalState {
  const persistedSessions = loadPersistedSessions();
  if (persistedSessions.length > 0) {
    const persistedActiveSessionId = localStorage.getItem(
      sshActiveSessionStorageKey,
    );
    const activeSessionId =
      persistedActiveSessionId &&
      persistedSessions.some(
        (session) => session.id === persistedActiveSessionId,
      )
        ? persistedActiveSessionId
        : persistedSessions[0].id;

    return { sessions: persistedSessions, activeSessionId };
  }

  const firstSession: SshSession = {
    id: sessionId(),
    targetKey: localStorage.getItem(sshTargetKeyStorageKey) ?? "local-shell",
    status: "idle",
    terminalSize: DEFAULT_TERMINAL_SIZE,
  };

  return { sessions: [firstSession], activeSessionId: firstSession.id };
}

export function SshPage({ visible = true }: { visible?: boolean }) {
  const navigate = useNavigate();
  const pageRef = useRef<HTMLDivElement | null>(null);
  const runtimeMapRef = useRef(new Map<string, SessionRuntime>());
  const hostMapRef = useRef(new Map<string, HTMLDivElement | null>());
  const autoConnectAttemptedRef = useRef(false);
  const visibleRef = useRef(visible);
  const [initialState] = useState(loadInitialTerminalState);
  const [sessions, setSessions] = useState<SshSession[]>(initialState.sessions);
  const [activeSessionId, setActiveSessionId] = useState<string>(
    initialState.activeSessionId,
  );
  const [fileBrowserSession, setFileBrowserSession] =
    useState<SshSession | null>(null);
  const [fileBrowserPath, setFileBrowserPath] = useState<string>();
  const [mobileControlsOpen, setMobileControlsOpen] = useState(false);
  const [filePreviewing, setFilePreviewing] = useState<SshFileRecord | null>(
    null,
  );
  const [filePreviewUrl, setFilePreviewUrl] = useState<string>();
  const [filePreviewText, setFilePreviewText] = useState<string>();
  const [filePreviewError, setFilePreviewError] = useState<string>();
  const [filePreviewLoading, setFilePreviewLoading] = useState(false);

  useEffect(() => {
    return () => {
      if (filePreviewUrl) {
        URL.revokeObjectURL(filePreviewUrl);
      }
    };
  }, [filePreviewUrl]);

  const targetsQuery = useQuery({
    queryKey: ["admin-ssh-targets"],
    queryFn: fetchSshTargets,
  });
  const sessionsQuery = useQuery({
    queryKey: ["admin-ssh-sessions"],
    queryFn: fetchSshSessions,
    refetchInterval: 5000,
  });
  const createSessionMutation = useMutation({ mutationFn: createSshSession });
  const ticketMutation = useMutation({ mutationFn: createSshSessionTicket });
  const closeSessionMutation = useMutation({ mutationFn: closeSshSession });
  const sshFilesQuery = useQuery({
    queryKey: [
      "admin-ssh-session-files",
      fileBrowserSession?.serverSessionId,
      fileBrowserPath,
    ],
    enabled: Boolean(fileBrowserSession?.serverSessionId),
    queryFn: () =>
      fetchSshSessionFiles({
        sessionId: fileBrowserSession?.serverSessionId ?? "",
        path: fileBrowserPath,
      }),
  });

  const activeSession = useMemo(
    () =>
      sessions.find((session) => session.id === activeSessionId) ??
      sessions[0] ??
      null,
    [activeSessionId, sessions],
  );

  useEffect(() => {
    visibleRef.current = visible;
  }, [visible]);

  useEffect(() => {
    if (
      sessions.length > 0 &&
      !sessions.some((session) => session.id === activeSessionId)
    ) {
      setActiveSessionId(sessions[0].id);
    }
  }, [activeSessionId, sessions]);

  useEffect(() => {
    persistSessions(sessions, activeSessionId);
  }, [activeSessionId, sessions]);

  const setAutoConnect = useCallback((enabled: boolean, targetKey?: string) => {
    if (enabled) {
      localStorage.setItem(sshAutoConnectKey, "1");
      if (targetKey) {
        localStorage.setItem(sshTargetKeyStorageKey, targetKey);
      }
      return;
    }

    localStorage.removeItem(sshAutoConnectKey);
  }, []);

  const updateSession = useCallback(
    (sessionIdValue: string, updater: (session: SshSession) => SshSession) => {
      setSessions((current) =>
        current.map((session) =>
          session.id === sessionIdValue ? updater(session) : session,
        ),
      );
    },
    [],
  );

  const cleanupSocket = useCallback((socket?: WebSocket | null) => {
    if (!socket) {
      return;
    }
    socket.onopen = null;
    socket.onmessage = null;
    socket.onerror = null;
    socket.onclose = null;
    if (
      socket.readyState === WebSocket.OPEN ||
      socket.readyState === WebSocket.CONNECTING
    ) {
      socket.close();
    }
  }, []);

  const disconnectSession = useCallback(
    (sessionIdValue: string, preserveAutoConnect = false) => {
      const runtime = runtimeMapRef.current.get(sessionIdValue);
      if (runtime) {
        cleanupSocket(runtime.socket);
        runtime.socket = null;
      }
      if (!preserveAutoConnect) {
        setAutoConnect(false);
      }
      updateSession(sessionIdValue, (session) => ({
        ...session,
        status: session.serverSessionId ? "detached" : "closed",
      }));
    },
    [cleanupSocket, setAutoConnect, updateSession],
  );

  const destroySessionRuntime = useCallback(
    (sessionIdValue: string) => {
      const runtime = runtimeMapRef.current.get(sessionIdValue);
      if (!runtime) {
        return;
      }
      cleanupSocket(runtime.socket);
      runtime.resizeObserver?.disconnect();
      runtime.inputDisposable.dispose();
      runtime.terminal.dispose();
      runtimeMapRef.current.delete(sessionIdValue);
      hostMapRef.current.delete(sessionIdValue);
    },
    [cleanupSocket],
  );

  const fitTerminal = useCallback(
    (sessionIdValue: string, notifySocket = true) => {
      const runtime = runtimeMapRef.current.get(sessionIdValue);
      if (!runtime) {
        return DEFAULT_TERMINAL_SIZE;
      }
      if (!visibleRef.current) {
        return {
          cols: runtime.terminal.cols || DEFAULT_TERMINAL_SIZE.cols,
          rows: runtime.terminal.rows || DEFAULT_TERMINAL_SIZE.rows,
        };
      }
      try {
        runtime.fitAddon.fit();
      } catch {
        return DEFAULT_TERMINAL_SIZE;
      }
      const nextSize = {
        cols: runtime.terminal.cols,
        rows: runtime.terminal.rows,
      };
      updateSession(sessionIdValue, (session) =>
        session.terminalSize.cols === nextSize.cols &&
        session.terminalSize.rows === nextSize.rows
          ? session
          : { ...session, terminalSize: nextSize },
      );
      if (notifySocket && runtime.socket?.readyState === WebSocket.OPEN) {
        runtime.socket.send(JSON.stringify({ type: "resize", ...nextSize }));
      }
      return nextSize;
    },
    [updateSession],
  );

  const createSession = useCallback(
    (targetKey?: string) => {
      const nextSession: SshSession = {
        id: sessionId(),
        targetKey: targetKey ?? activeSession?.targetKey ?? "local-shell",
        status: "idle",
        terminalSize: DEFAULT_TERMINAL_SIZE,
      };
      setSessions((current) => [...current, nextSession]);
      setActiveSessionId(nextSession.id);
    },
    [activeSession?.targetKey],
  );

  const mountTerminal = useCallback(
    (sessionIdValue: string, runtime: SessionRuntime, host: HTMLDivElement) => {
      runtime.host = host;
      runtime.resizeObserver?.disconnect();
      runtime.resizeObserver = null;
      if (!runtime.terminal.element) {
        host.innerHTML = "";
        runtime.terminal.open(host);
      }
      runtime.terminal.focus();
      if (typeof ResizeObserver !== "undefined") {
        runtime.resizeObserver = new ResizeObserver(() => {
          fitTerminal(sessionIdValue);
        });
        runtime.resizeObserver.observe(host);
      }
      window.requestAnimationFrame(() => {
        fitTerminal(sessionIdValue, false);
        runtime.terminal.focus();
      });
    },
    [fitTerminal],
  );

  const closeSession = useCallback(
    (targetSessionId: string) => {
      if (sessions.length <= 1) {
        return;
      }
      disconnectSession(targetSessionId, true);
      destroySessionRuntime(targetSessionId);
      const remaining = sessions.filter(
        (session) => session.id !== targetSessionId,
      );
      setSessions(remaining);
      if (activeSessionId === targetSessionId) {
        setActiveSessionId(remaining[remaining.length - 1]?.id ?? "");
      }
    },
    [activeSessionId, destroySessionRuntime, disconnectSession, sessions],
  );

  const closeSharedSession = useCallback(
    async (session: SshSession) => {
      if (!session.serverSessionId) {
        closeSession(session.id);
        return;
      }
      disconnectSession(session.id, true);
      await closeSessionMutation.mutateAsync(session.serverSessionId);
      updateSession(session.id, (current) => ({
        ...current,
        status: "closed",
      }));
      void sessionsQuery.refetch();
    },
    [
      closeSession,
      closeSessionMutation,
      disconnectSession,
      sessionsQuery,
      updateSession,
    ],
  );

  const bindHost = useCallback(
    (sessionIdValue: string, host: HTMLDivElement | null) => {
      hostMapRef.current.set(sessionIdValue, host);
      const runtime = runtimeMapRef.current.get(sessionIdValue);
      if (!runtime) {
        return;
      }
      if (!host) {
        runtime.resizeObserver?.disconnect();
        runtime.resizeObserver = null;
        runtime.host = null;
        return;
      }
      if (runtime.host !== host || !runtime.terminal.element) {
        mountTerminal(sessionIdValue, runtime, host);
      }
    },
    [mountTerminal],
  );

  const focusTerminal = useCallback((sessionIdValue: string) => {
    runtimeMapRef.current.get(sessionIdValue)?.terminal.focus();
  }, []);

  const sendTerminalInput = useCallback(
    (sessionIdValue: string, data: string) => {
      const runtime = runtimeMapRef.current.get(sessionIdValue);
      if (runtime?.socket?.readyState !== WebSocket.OPEN) {
        message.warning("终端未连接，请先接入会话");
        return;
      }
      runtime.socket.send(JSON.stringify({ type: "input", data }));
      runtime.terminal.focus();
    },
    [],
  );

  const copyTerminalContent = useCallback(async (sessionIdValue: string) => {
    const runtime = runtimeMapRef.current.get(sessionIdValue);
    if (!runtime) {
      message.warning("终端尚未就绪");
      return;
    }

    const selected = runtime.terminal.hasSelection();
    const text = selected
      ? runtime.terminal.getSelection()
      : terminalBufferText(runtime.terminal);
    if (!text) {
      message.info("终端中暂无可复制内容");
      runtime.terminal.focus();
      return;
    }

    try {
      await writeClipboardText(text);
      message.success(selected ? "已复制选中内容" : "已复制终端内容");
    } catch (error) {
      message.error(error instanceof Error ? error.message : "复制失败");
    } finally {
      runtime.terminal.focus();
    }
  }, []);

  useLayoutEffect(() => {
    for (const session of sessions) {
      if (runtimeMapRef.current.has(session.id)) {
        continue;
      }
      const terminal = new Terminal({
        cursorBlink: true,
        convertEol: true,
        rightClickSelectsWord: true,
        scrollOnUserInput: true,
        fontFamily:
          'Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
        fontSize: 13,
        theme: {
          background: "#050505",
          foreground: "#f5f5f5",
          cursor: "#ffffff",
          selectionBackground: "#334155",
        },
      });
      const fitAddon = new FitAddon();
      terminal.loadAddon(fitAddon);
      terminal.attachCustomKeyEventHandler((event) => {
        const isCopyShortcut =
          (event.ctrlKey || event.metaKey) &&
          !event.altKey &&
          event.key.toLowerCase() === "c";
        if (!isCopyShortcut || !terminal.hasSelection()) {
          return true;
        }
        if (event.type === "keydown") {
          void copyTerminalContent(session.id);
        }
        return false;
      });
      terminal.writeln("后台 SSH 终端已就绪，请选择目标并点击连接。");
      const inputDisposable = terminal.onData((data) => {
        const runtime = runtimeMapRef.current.get(session.id);
        if (runtime?.socket?.readyState === WebSocket.OPEN) {
          runtime.socket.send(JSON.stringify({ type: "input", data }));
        }
      });
      const host = hostMapRef.current.get(session.id) ?? null;
      const runtime: SessionRuntime = {
        terminal,
        fitAddon,
        inputDisposable,
        socket: null,
        host: null,
        resizeObserver: null,
      };
      runtimeMapRef.current.set(session.id, runtime);
      if (host) {
        mountTerminal(session.id, runtime, host);
      }
    }
  }, [copyTerminalContent, mountTerminal, sessions]);

  useEffect(() => {
    return () => {
      for (const sessionIdValue of runtimeMapRef.current.keys()) {
        destroySessionRuntime(sessionIdValue);
      }
    };
  }, [destroySessionRuntime]);

  useEffect(() => {
    const viewport = window.visualViewport;
    let animationFrame = 0;

    const syncViewport = () => {
      window.cancelAnimationFrame(animationFrame);
      animationFrame = window.requestAnimationFrame(() => {
        const page = pageRef.current;
        if (page) {
          page.style.setProperty(
            "--ssh-viewport-height",
            `${Math.round(viewport?.height ?? window.innerHeight)}px`,
          );
          page.style.setProperty(
            "--ssh-viewport-offset-top",
            `${Math.round(viewport?.offsetTop ?? 0)}px`,
          );
        }
        if (!activeSessionId) {
          return;
        }
        fitTerminal(activeSessionId);
        runtimeMapRef.current.get(activeSessionId)?.terminal.scrollToBottom();
      });
    };

    syncViewport();
    window.addEventListener("resize", syncViewport);
    window.addEventListener("orientationchange", syncViewport);
    viewport?.addEventListener("resize", syncViewport);
    viewport?.addEventListener("scroll", syncViewport);
    return () => {
      window.cancelAnimationFrame(animationFrame);
      window.removeEventListener("resize", syncViewport);
      window.removeEventListener("orientationchange", syncViewport);
      viewport?.removeEventListener("resize", syncViewport);
      viewport?.removeEventListener("scroll", syncViewport);
    };
  }, [activeSessionId, fitTerminal]);

  useEffect(() => {
    if (!targetsQuery.data?.length) {
      return;
    }
    setSessions((current) =>
      current.map((session) => {
        if (
          targetsQuery.data?.some((target) => target.key === session.targetKey)
        ) {
          return session;
        }
        return { ...session, targetKey: targetsQuery.data[0].key };
      }),
    );
  }, [targetsQuery.data]);

  useEffect(() => {
    if (!sessionsQuery.data?.length) {
      return;
    }
    setSessions((current) => {
      let next = current.map((session) => {
        const serverSession = sessionsQuery.data.find(
          (item) => item.id === session.serverSessionId,
        );
        if (!serverSession) {
          return session;
        }
        return {
          ...session,
          targetKey: serverSession.target_key,
          status:
            serverSession.status === "closed"
              ? "closed"
              : runtimeMapRef.current.get(session.id)?.socket
                ? session.status
                : "detached",
          terminalSize: {
            cols: serverSession.cols,
            rows: serverSession.rows,
          },
        } as SshSession;
      });
      for (const serverSession of sessionsQuery.data) {
        if (
          next.some((session) => session.serverSessionId === serverSession.id)
        ) {
          continue;
        }
        next = [
          ...next,
          {
            id: sessionId(),
            serverSessionId: serverSession.id,
            targetKey: serverSession.target_key,
            status: serverSession.status === "closed" ? "closed" : "detached",
            terminalSize: {
              cols: serverSession.cols,
              rows: serverSession.rows,
            },
          },
        ];
      }
      return next;
    });
  }, [sessionsQuery.data]);

  useEffect(() => {
    if (!activeSession?.targetKey) {
      return;
    }
    localStorage.setItem(sshTargetKeyStorageKey, activeSession.targetKey);
  }, [activeSession?.targetKey]);

  const connectSession = useCallback(
    async (sessionIdValue: string) => {
      const session = sessions.find((item) => item.id === sessionIdValue);
      if (!session?.targetKey) {
        message.warning("请先选择 SSH 目标");
        return;
      }
      const runtime = runtimeMapRef.current.get(sessionIdValue);
      if (!runtime) {
        message.error("终端尚未就绪，请稍后再试");
        return;
      }
      disconnectSession(sessionIdValue, true);
      runtime.terminal.writeln(
        `\r\n[${new Date().toLocaleTimeString()}] 正在接入共享 SSH 会话...`,
      );
      updateSession(sessionIdValue, (current) => ({
        ...current,
        status: "connecting",
      }));
      try {
        const size = fitTerminal(sessionIdValue, false);
        const serverSessionId = session.serverSessionId
          ? session.serverSessionId
          : (
              await createSessionMutation.mutateAsync({
                target_key: session.targetKey,
                cols: size.cols,
                rows: size.rows,
              })
            ).id;
        const ticket = await ticketMutation.mutateAsync(serverSessionId);
        runtime.terminal.writeln("正在附加终端...");
        setAutoConnect(true, session.targetKey);
        updateSession(sessionIdValue, (current) => ({
          ...current,
          serverSessionId,
        }));
        const socket = new WebSocket(
          buildSshSessionWebSocketUrl(serverSessionId, ticket.ticket),
        );
        runtime.socket = socket;
        socket.onopen = () => {
          const currentRuntime = runtimeMapRef.current.get(sessionIdValue);
          if (currentRuntime?.socket !== socket) {
            return;
          }
          const nextSize = fitTerminal(sessionIdValue, false);
          socket.send(JSON.stringify({ type: "resize", ...nextSize }));
          currentRuntime.terminal.focus();
        };
        socket.onmessage = (event) => {
          const currentRuntime = runtimeMapRef.current.get(sessionIdValue);
          if (!currentRuntime) {
            return;
          }
          try {
            const payload = JSON.parse(event.data) as ServerTerminalMessage;
            if (payload.type === "output") {
              currentRuntime.terminal.write(payload.data ?? "");
            } else if (payload.type === "connected") {
              updateSession(sessionIdValue, (current) => ({
                ...current,
                status: "connected",
              }));
              currentRuntime.terminal.focus();
            } else if (payload.type === "closed") {
              updateSession(sessionIdValue, (current) => ({
                ...current,
                status: "closed",
              }));
            } else if (payload.type === "error") {
              updateSession(sessionIdValue, (current) => ({
                ...current,
                status: "error",
              }));
              currentRuntime.terminal.writeln(
                `\r\n${payload.data ?? "终端连接异常"}`,
              );
            }
          } catch {
            currentRuntime.terminal.write(String(event.data));
          }
        };
        socket.onerror = () => {
          const currentRuntime = runtimeMapRef.current.get(sessionIdValue);
          if (currentRuntime?.socket === socket) {
            updateSession(sessionIdValue, (current) => ({
              ...current,
              status: "error",
            }));
          }
          currentRuntime?.terminal.writeln("\r\n终端连接异常");
        };
        socket.onclose = () => {
          const currentRuntime = runtimeMapRef.current.get(sessionIdValue);
          if (currentRuntime?.socket === socket) {
            currentRuntime.socket = null;
          }
          updateSession(sessionIdValue, (current) => ({
            ...current,
            status: current.status === "error" ? "error" : "detached",
          }));
        };
        void sessionsQuery.refetch();
      } catch (error) {
        updateSession(sessionIdValue, (current) => ({
          ...current,
          status: "error",
        }));
        setAutoConnect(false);
        runtime.terminal.writeln(
          `\r\n${error instanceof Error ? error.message : "连接失败"}`,
        );
        message.error(error instanceof Error ? error.message : "连接失败");
      }
    },
    [
      createSessionMutation,
      disconnectSession,
      fitTerminal,
      sessions,
      sessionsQuery,
      setAutoConnect,
      ticketMutation,
      updateSession,
    ],
  );

  useEffect(() => {
    if (
      autoConnectAttemptedRef.current ||
      localStorage.getItem(sshAutoConnectKey) !== "1" ||
      !activeSession ||
      targetsQuery.isLoading ||
      !targetsQuery.data?.some(
        (target) => target.key === activeSession.targetKey,
      )
    ) {
      return;
    }

    autoConnectAttemptedRef.current = true;
    void connectSession(activeSession.id);
  }, [
    activeSession,
    connectSession,
    targetsQuery.data,
    targetsQuery.isLoading,
  ]);

  const selectedTarget = targetsQuery.data?.find(
    (target) => target.key === activeSession?.targetKey,
  );

  const openFileBrowser = useCallback((session: SshSession) => {
    if (!session.serverSessionId) {
      message.warning("请先连接 SSH 会话");
      return;
    }
    setFileBrowserSession(session);
    setFileBrowserPath(undefined);
  }, []);

  const closeFilePreview = useCallback(() => {
    setFilePreviewing(null);
    setFilePreviewText(undefined);
    setFilePreviewError(undefined);
    setFilePreviewLoading(false);
    setFilePreviewUrl((current) => {
      if (current) {
        URL.revokeObjectURL(current);
      }
      return undefined;
    });
  }, []);

  const openFilePreview = useCallback(
    async (record: SshFileRecord) => {
      if (!fileBrowserSession?.serverSessionId || record.is_dir) {
        return;
      }
      setFilePreviewing(record);
      setFilePreviewLoading(true);
      setFilePreviewError(undefined);
      setFilePreviewText(undefined);
      setFilePreviewUrl((current) => {
        if (current) {
          URL.revokeObjectURL(current);
        }
        return undefined;
      });
      try {
        const blob = await previewSshSessionFile({
          sessionId: fileBrowserSession.serverSessionId,
          path: record.path,
        });
        if (sshPreviewKind(record) === "text") {
          setFilePreviewText(await blob.text());
        } else {
          setFilePreviewUrl(URL.createObjectURL(blob));
        }
      } catch (error) {
        setFilePreviewError(
          error instanceof Error ? error.message : "文件预览失败",
        );
      } finally {
        setFilePreviewLoading(false);
      }
    },
    [fileBrowserSession?.serverSessionId],
  );

  const renderFilePreview = () => {
    if (filePreviewLoading) {
      return (
        <div className="material-preview-empty">
          <Spin tip="正在加载预览" />
        </div>
      );
    }
    if (!filePreviewing) {
      return <Empty description="暂无预览内容" />;
    }
    if (filePreviewError) {
      return <Empty description={filePreviewError} />;
    }
    const kind = sshPreviewKind(filePreviewing);
    if (kind === "text") {
      return <pre className="material-preview-text">{filePreviewText}</pre>;
    }
    if (!filePreviewUrl) {
      return <Empty description="暂无预览内容" />;
    }
    if (kind === "image") {
      return (
        <img
          className="material-preview-image"
          src={filePreviewUrl}
          alt={filePreviewing.name}
        />
      );
    }
    if (kind === "video") {
      return (
        <video className="material-preview-frame" src={filePreviewUrl} controls>
          <track kind="captions" />
        </video>
      );
    }
    if (kind === "audio") {
      return (
        <div className="material-preview-empty">
          <audio
            className="material-preview-audio"
            src={filePreviewUrl}
            controls
          >
            <track kind="captions" />
          </audio>
        </div>
      );
    }
    if (kind === "pdf") {
      return (
        <iframe
          className="material-preview-frame"
          src={filePreviewUrl}
          title={filePreviewing.name}
        />
      );
    }
    return <Empty description="该文件类型暂不支持在线预览" />;
  };

  const sshFileColumns: ColumnsType<SshFileRecord> = useMemo(
    () => [
      {
        title: "名称",
        dataIndex: "name",
        render: (name: string, record) =>
          record.is_dir ? (
            <Button type="link" onClick={() => setFileBrowserPath(record.path)}>
              <FolderOpenOutlined /> {name}
            </Button>
          ) : (
            <Typography.Text className="ssh-file-name">{name}</Typography.Text>
          ),
      },
      {
        title: "类型",
        width: 90,
        render: (_, record) =>
          record.is_dir ? "目录" : record.extension || "文件",
      },
      {
        title: "大小",
        width: 110,
        render: (_, record) =>
          record.is_dir ? "-" : formatBytes(record.size_bytes),
      },
      {
        title: "更新时间",
        width: 190,
        render: (_, record) =>
          record.updated_at
            ? new Date(record.updated_at).toLocaleString()
            : "-",
      },
    ],
    [],
  );

  useEffect(() => {
    if (!activeSessionId || !visible) {
      return;
    }
    const runtime = runtimeMapRef.current.get(activeSessionId);
    if (!runtime?.host) {
      return;
    }
    window.requestAnimationFrame(() => {
      fitTerminal(activeSessionId, false);
      runtime.terminal.focus();
    });
  }, [activeSessionId, fitTerminal, visible]);

  const sessionTabs: TabsProps["items"] = sessions.map((session, index) => {
    const target = targetsQuery.data?.find(
      (item) => item.key === session.targetKey,
    );
    const title = target?.name ?? `终端 ${index + 1}`;
    return {
      key: session.id,
      label: (
        <Space size={6}>
          <span>{`${title} ${index + 1}`}</span>
          <Tag color={statusColor(session.status)}>
            {statusLabel(session.status)}
          </Tag>
          <Button
            className="ssh-tab-file-button"
            size="small"
            type="text"
            icon={<FolderOpenOutlined />}
            disabled={!session.serverSessionId}
            onClick={(event) => {
              event.stopPropagation();
              openFileBrowser(session);
            }}
          />
        </Space>
      ),
      children: null,
    };
  });

  return (
    <div ref={pageRef} className="ssh-page">
      <CrudPage
        title="SSH 管理"
        subtitle="后台免二次登录的本机 Shell 与远程 SSH 交互式终端"
        breadcrumb={["系统管理", "SSH 管理"]}
        icon={<CodeOutlined />}
        toolbar={
          <Space wrap className="ssh-page-actions">
            <Button
              icon={<ReloadOutlined />}
              onClick={() => targetsQuery.refetch()}
            >
              刷新目标
            </Button>
            <PermissionButton
              type="primary"
              icon={<PlusOutlined />}
              permission="system:ssh:connect"
              onClick={() => createSession()}
            >
              新建终端
            </PermissionButton>
          </Space>
        }
      >
        <Card className="ssh-terminal-card">
          <div className="ssh-mobile-terminal-header">
            <Button
              type="text"
              icon={<ArrowLeftOutlined />}
              aria-label="退出终端"
              onClick={() => navigate("/admin/dashboard")}
            />
            <div className="ssh-mobile-terminal-title">
              <Typography.Text strong>
                {selectedTarget?.name ?? "SSH 终端"}
              </Typography.Text>
              <Typography.Text type="secondary">
                {activeSession ? statusLabel(activeSession.status) : "无会话"}
              </Typography.Text>
            </div>
            <Space size={2}>
              <Tooltip title="复制选中内容；未选择时复制全部终端内容">
                <Button
                  type="text"
                  icon={<CopyOutlined />}
                  aria-label="复制终端内容"
                  onClick={() =>
                    activeSession && copyTerminalContent(activeSession.id)
                  }
                />
              </Tooltip>
              <PermissionButton
                type="text"
                icon={<PlusOutlined />}
                permission="system:ssh:connect"
                aria-label="新建终端"
                onClick={() => createSession()}
              />
              <Button
                type="text"
                icon={<MoreOutlined />}
                aria-label="终端控制"
                onClick={() => setMobileControlsOpen(true)}
              />
            </Space>
          </div>
          <Tabs
            type="editable-card"
            hideAdd
            activeKey={activeSessionId}
            items={sessionTabs}
            destroyInactiveTabPane={false}
            onChange={setActiveSessionId}
            onEdit={(targetKey, action) => {
              if (action === "remove" && typeof targetKey === "string") {
                closeSession(targetKey);
              }
            }}
            className="ssh-session-tabs"
          />
          {activeSession ? (
            <div className="ssh-session-panel">
              <div className="ssh-terminal-toolbar">
                <Space wrap className="ssh-toolbar-group">
                  <Select
                    loading={targetsQuery.isLoading}
                    value={activeSession.targetKey}
                    style={{ width: 280 }}
                    onChange={(value) => {
                      updateSession(activeSession.id, (current) => ({
                        ...current,
                        targetKey: value,
                      }));
                    }}
                    options={(targetsQuery.data ?? []).map((targetOption) => ({
                      value: targetOption.key,
                      label:
                        targetOption.target_type === "local"
                          ? targetOption.name
                          : `${targetOption.name} (${targetOption.username}@${targetOption.host}:${targetOption.port ?? 22})`,
                    }))}
                  />
                  <PermissionButton
                    type="primary"
                    icon={<LinkOutlined />}
                    permission="system:ssh:connect"
                    loading={
                      ticketMutation.isPending ||
                      createSessionMutation.isPending
                    }
                    onClick={() => connectSession(activeSession.id)}
                  >
                    连接
                  </PermissionButton>
                  <Button
                    icon={<DisconnectOutlined />}
                    onClick={() => disconnectSession(activeSession.id)}
                  >
                    断开
                  </Button>
                  <Button
                    icon={<ClearOutlined />}
                    onClick={() =>
                      runtimeMapRef.current
                        .get(activeSession.id)
                        ?.terminal.clear()
                    }
                  >
                    清屏
                  </Button>
                  <Button
                    icon={<CopyOutlined />}
                    onClick={() => copyTerminalContent(activeSession.id)}
                  >
                    复制
                  </Button>
                  <Tag color={statusColor(activeSession.status)}>
                    {statusLabel(activeSession.status)}
                  </Tag>
                  <Typography.Text type="secondary">
                    {activeSession.terminalSize.cols} ×{" "}
                    {activeSession.terminalSize.rows}
                  </Typography.Text>
                </Space>
                <Space wrap className="ssh-toolbar-group">
                  <Typography.Text type="secondary">
                    {selectedTarget?.target_type === "local"
                      ? "当前目标：本机 Shell"
                      : selectedTarget
                        ? `当前目标：${selectedTarget.username}@${selectedTarget.host}`
                        : "未选择目标"}
                  </Typography.Text>
                  <Button
                    icon={<CloseOutlined />}
                    loading={closeSessionMutation.isPending}
                    onClick={() => void closeSharedSession(activeSession)}
                  >
                    关闭会话
                  </Button>
                </Space>
              </div>
              <div className="ssh-mobile-keybar">
                {specialKeys.map((key) => (
                  <Button
                    key={key.label}
                    size="small"
                    onClick={() =>
                      sendTerminalInput(activeSession.id, key.data)
                    }
                  >
                    {key.label}
                  </Button>
                ))}
              </div>
              <div className="ssh-terminal-stage">
                {sessions.map((session) => (
                  <SshTerminalHost
                    key={session.id}
                    sessionId={session.id}
                    active={activeSessionId === session.id}
                    onBind={bindHost}
                    onFocus={focusTerminal}
                  />
                ))}
              </div>
            </div>
          ) : null}
        </Card>
        <Drawer
          title="终端控制"
          open={mobileControlsOpen}
          placement="bottom"
          height="auto"
          className="ssh-mobile-controls-drawer"
          onClose={() => setMobileControlsOpen(false)}
        >
          {activeSession ? (
            <Space direction="vertical" size={12} className="admin-full-width">
              <Select
                loading={targetsQuery.isLoading}
                value={activeSession.targetKey}
                className="admin-full-width"
                onChange={(value) => {
                  updateSession(activeSession.id, (current) => ({
                    ...current,
                    targetKey: value,
                  }));
                }}
                options={(targetsQuery.data ?? []).map((targetOption) => ({
                  value: targetOption.key,
                  label:
                    targetOption.target_type === "local"
                      ? targetOption.name
                      : `${targetOption.name} (${targetOption.username}@${targetOption.host}:${targetOption.port ?? 22})`,
                }))}
              />
              <div className="ssh-mobile-control-grid">
                <PermissionButton
                  type="primary"
                  icon={<LinkOutlined />}
                  permission="system:ssh:connect"
                  loading={
                    ticketMutation.isPending || createSessionMutation.isPending
                  }
                  onClick={() => {
                    setMobileControlsOpen(false);
                    connectSession(activeSession.id);
                  }}
                >
                  连接
                </PermissionButton>
                <Button
                  icon={<DisconnectOutlined />}
                  onClick={() => {
                    setMobileControlsOpen(false);
                    disconnectSession(activeSession.id);
                  }}
                >
                  断开
                </Button>
                <Button
                  icon={<ClearOutlined />}
                  onClick={() => {
                    runtimeMapRef.current
                      .get(activeSession.id)
                      ?.terminal.clear();
                    setMobileControlsOpen(false);
                  }}
                >
                  清屏
                </Button>
                <Button
                  icon={<CopyOutlined />}
                  onClick={() => {
                    setMobileControlsOpen(false);
                    void copyTerminalContent(activeSession.id);
                  }}
                >
                  复制
                </Button>
                <Button
                  icon={<FolderOpenOutlined />}
                  disabled={!activeSession.serverSessionId}
                  onClick={() => {
                    setMobileControlsOpen(false);
                    openFileBrowser(activeSession);
                  }}
                >
                  文件
                </Button>
                <Button
                  icon={<ReloadOutlined />}
                  onClick={() => targetsQuery.refetch()}
                >
                  刷新目标
                </Button>
                <Button
                  danger
                  icon={<CloseOutlined />}
                  loading={closeSessionMutation.isPending}
                  onClick={() => {
                    setMobileControlsOpen(false);
                    void closeSharedSession(activeSession);
                  }}
                >
                  关闭会话
                </Button>
              </div>
            </Space>
          ) : null}
        </Drawer>
        <Modal
          title="SSH 文件管理"
          open={Boolean(fileBrowserSession)}
          width={900}
          footer={null}
          onCancel={() => {
            setFileBrowserSession(null);
            setFileBrowserPath(undefined);
          }}
          className="ssh-file-modal"
        >
          <Space direction="vertical" size={12} className="admin-full-width">
            <Space wrap className="ssh-file-toolbar">
              <Typography.Text type="secondary">当前目录</Typography.Text>
              <Typography.Text code copyable>
                {sshFilesQuery.data?.path ?? fileBrowserPath ?? "加载中..."}
              </Typography.Text>
              <Button
                size="small"
                disabled={
                  !sshFilesQuery.data?.path || sshFilesQuery.data.path === "/"
                }
                onClick={() =>
                  setFileBrowserPath(
                    parentPath(sshFilesQuery.data?.path ?? "/"),
                  )
                }
              >
                上级目录
              </Button>
              <Button
                size="small"
                icon={<ReloadOutlined />}
                loading={sshFilesQuery.isFetching}
                onClick={() => sshFilesQuery.refetch()}
              >
                刷新
              </Button>
            </Space>
            {sshFilesQuery.isError ? (
              <Empty
                description={
                  sshFilesQuery.error instanceof Error
                    ? sshFilesQuery.error.message
                    : "目录加载失败"
                }
              />
            ) : (
              <DataTable<SshFileRecord>
                rowKey="path"
                loading={sshFilesQuery.isFetching}
                columns={sshFileColumns}
                dataSource={[
                  ...(sshFilesQuery.data?.directories ?? []),
                  ...(sshFilesQuery.data?.files ?? []),
                ]}
                onRow={(record) => ({
                  onDoubleClick: () =>
                    record.is_dir
                      ? setFileBrowserPath(record.path)
                      : void openFilePreview(record),
                })}
                pagination={false}
                scroll={{ x: 680, y: 460 }}
                className="ssh-file-table"
              />
            )}
          </Space>
        </Modal>
        <Modal
          title={filePreviewing?.name ?? "文件预览"}
          open={Boolean(filePreviewing)}
          footer={null}
          width="min(980px, 96vw)"
          className="material-preview-modal ssh-file-preview-modal"
          onCancel={closeFilePreview}
        >
          <div className="material-preview-body">{renderFilePreview()}</div>
        </Modal>
      </CrudPage>
    </div>
  );
}
