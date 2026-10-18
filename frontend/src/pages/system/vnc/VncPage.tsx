import {
  AppstoreOutlined,
  ArrowLeftOutlined,
  CloseOutlined,
  DeleteOutlined,
  DesktopOutlined,
  DisconnectOutlined,
  FontSizeOutlined,
  FullscreenExitOutlined,
  FullscreenOutlined,
  KeyOutlined,
  LinkOutlined,
  MoreOutlined,
  PlusOutlined,
  ReloadOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import RFB from "@novnc/novnc";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Checkbox,
  Drawer,
  Empty,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Tooltip,
  Typography,
  message,
} from "antd";
import type {
  FormEvent,
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
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
  buildVncSessionWebSocketUrl,
  closeVncSession,
  createVncSession,
  createVncSessionTicket,
  deleteVncCredentials,
  fetchVncSessions,
  fetchVncTargets,
  saveVncCredentials,
} from "../../../api/admin/vnc";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import "./vnc.css";
import { t } from "../../../i18n";
import { createClientId } from "../../../utils/clientId";
import { VncQuickBar, VncShortcutsDrawer } from "./VncQuickBar";
import { useVncPointerModes } from "./useVncPointerModes";
import {
  KEYS,
  type KeyStroke,
  type ModifierName,
  charStroke,
  comboSequence,
  letterKey,
  textStrokes,
  withModifiers,
} from "./vncInput";

type VncStatus =
  | "idle"
  | "connecting"
  | "connected"
  | "disconnected"
  | "closed"
  | "error";

type VncSession = {
  id: string;
  serverSessionId?: string;
  targetKey: string;
  targetName?: string;
  status: VncStatus;
  errorMessage?: string;
};

type VncCredentialRequest = {
  sessionId: string;
  targetKey: string;
  types: string[];
  credentialsSaved: boolean;
};

type CreateVncSessionVariables = {
  target_key: string;
  localSessionId?: string;
};

type InitialVncState = {
  sessions: VncSession[];
  activeSessionId: string;
};

type RfbRuntime = {
  rfb: RFB | null;
  host: HTMLDivElement | null;
};

type PendingVncCredentialSave = {
  credentials: {
    target_key: string;
    username?: string;
    password?: string;
    target?: string;
  };
};

type AttachSessionResult = "attached" | "stale" | "failed";

type VncViewerHostProps = {
  sessionId: string;
  active: boolean;
  status: VncStatus;
  errorMessage?: string;
  trackpad: boolean;
  rightClickArmed: boolean;
  showLocalPointer: boolean;
  onBind: (sessionId: string, host: HTMLDivElement | null) => void;
  onConnect: () => void;
  onRightClickUsed: () => void;
};

const vncTargetKeyStorageKey = "gpt-images-admin-vnc-target";
const vncSessionsStorageKey = "gpt-images-admin-vnc-sessions";
const vncActiveSessionStorageKey = "gpt-images-admin-vnc-active-session";
const vncTrackpadStorageKey = "gpt-images-admin-vnc-trackpad";
const vncLocalPointerStorageKey = "gpt-images-admin-vnc-local-pointer";
const vncQuickBarStorageKey = "gpt-images-admin-vnc-quickbar";

function storedFlag(key: string, fallback: () => boolean) {
  if (typeof localStorage === "undefined") {
    return fallback();
  }
  const value = localStorage.getItem(key);
  return value === null ? fallback() : value === "1";
}

function storeFlag(key: string, value: boolean) {
  if (typeof localStorage !== "undefined") {
    localStorage.setItem(key, value ? "1" : "0");
  }
}

function prefersTrackpad() {
  return (
    typeof window !== "undefined" &&
    Boolean(window.matchMedia?.("(pointer: coarse)").matches)
  );
}
const insecureVncContextMessage = t(
  "VNC 需要 HTTPS 安全环境，请使用 HTTPS 域名访问，或在服务器本机通过 localhost 打开",
);

function requireSecureVncContext() {
  if (!window.isSecureContext) {
    throw new Error(insecureVncContextMessage);
  }
}

function sessionId() {
  return createClientId("vnc-session");
}

function statusLabel(status: VncStatus) {
  if (status === "connected") {
    return t("已连接");
  }
  if (status === "connecting") {
    return t("连接中");
  }
  if (status === "disconnected") {
    return t("已断开");
  }
  if (status === "closed") {
    return t("已关闭");
  }
  if (status === "error") {
    return t("异常");
  }
  return t("未连接");
}

function loadPersistedSessions(): VncSession[] {
  if (typeof localStorage === "undefined") {
    return [];
  }
  try {
    const raw = localStorage.getItem(vncSessionsStorageKey);
    if (!raw) {
      return [];
    }
    const sessions = JSON.parse(raw) as Partial<VncSession>[];
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
            : "local-vnc",
        targetName:
          typeof session.targetName === "string"
            ? session.targetName
            : undefined,
        status: "idle",
      }));
  } catch {
    return [];
  }
}

function persistSessions(sessions: VncSession[], activeSessionId: string) {
  if (typeof localStorage === "undefined") {
    return;
  }
  localStorage.setItem(
    vncSessionsStorageKey,
    JSON.stringify(
      sessions.map((session) => ({
        id: session.id,
        serverSessionId: session.serverSessionId,
        targetKey: session.targetKey,
        targetName: session.targetName,
      })),
    ),
  );
  localStorage.setItem(vncActiveSessionStorageKey, activeSessionId);
}

function initialState(): InitialVncState {
  const sessions = loadPersistedSessions();
  const fallback = sessions[0]?.id ?? sessionId();
  const activeSessionId =
    typeof localStorage === "undefined"
      ? fallback
      : (localStorage.getItem(vncActiveSessionStorageKey) ?? fallback);
  if (sessions.length > 0) {
    return {
      sessions,
      activeSessionId: sessions.some(
        (session) => session.id === activeSessionId,
      )
        ? activeSessionId
        : sessions[0].id,
    };
  }
  return {
    sessions: [{ id: fallback, targetKey: "local-vnc", status: "idle" }],
    activeSessionId: fallback,
  };
}

const VncViewerHost = memo(function VncViewerHost({
  sessionId,
  active,
  status,
  errorMessage,
  trackpad,
  rightClickArmed,
  showLocalPointer,
  onBind,
  onConnect,
  onRightClickUsed,
}: VncViewerHostProps) {
  const pointerRef = useRef<HTMLSpanElement | null>(null);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const trackpadCursorRef = useRef<HTMLSpanElement | null>(null);
  useVncPointerModes({
    stageRef,
    cursorRef: trackpadCursorRef,
    enabled: active && status === "connected",
    trackpad,
    rightClickArmed,
    onRightClickUsed,
  });
  const bindRef = useCallback(
    (host: HTMLDivElement | null) => {
      onBind(sessionId, host);
    },
    [onBind, sessionId],
  );
  const updatePointer = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const pointer = pointerRef.current;
      if (!pointer || status !== "connected") {
        return;
      }
      // Trackpad mode draws its own cursor; the finger position is irrelevant.
      if (!showLocalPointer || (trackpad && event.pointerType === "touch")) {
        pointer.classList.remove("is-visible");
        return;
      }
      const bounds = event.currentTarget.getBoundingClientRect();
      pointer.style.transform = `translate3d(${event.clientX - bounds.left}px, ${event.clientY - bounds.top}px, 0) translate(-50%, -50%)`;
      pointer.classList.toggle("is-touch", event.pointerType === "touch");
      pointer.classList.add("is-visible");
    },
    [showLocalPointer, status, trackpad],
  );
  const hidePointer = useCallback(() => {
    pointerRef.current?.classList.remove("is-visible");
  }, []);

  return (
    <div
      ref={stageRef}
      className={`vnc-viewer-stage${rightClickArmed ? " is-right-click-armed" : ""}${trackpad ? " is-trackpad" : ""}`}
      style={{ display: active ? "block" : "none" }}
      onPointerDown={updatePointer}
      onPointerMove={updatePointer}
      onPointerLeave={hidePointer}
      onPointerCancel={hidePointer}
      onPointerUp={(event) => {
        if (event.pointerType === "touch") {
          hidePointer();
        }
      }}
    >
      <div ref={bindRef} className="vnc-viewer-host" />
      <span ref={pointerRef} className="vnc-local-pointer" aria-hidden="true" />
      <span
        ref={trackpadCursorRef}
        className="vnc-trackpad-cursor"
        aria-hidden="true"
      >
        <svg
          viewBox="0 0 16 24"
          width="18"
          height="27"
          focusable="false"
          aria-hidden="true"
        >
          <path
            d="M1.5 1.5v18.2l4.6-4.4 3.1 7.2 3-1.3-3.1-7.1h6.4z"
            fill="#fff"
            stroke="#151515"
            strokeWidth="1.3"
            strokeLinejoin="round"
          />
        </svg>
      </span>
      {status !== "connected" ? (
        <div className="vnc-viewer-state" aria-live="polite">
          <DesktopOutlined />
          <strong>
            {status === "connecting"
              ? t("正在建立 VNC 连接")
              : status === "error"
                ? t("VNC 连接失败")
                : t("VNC 尚未连接")}
          </strong>
          <span>
            {errorMessage ??
              (status === "connecting"
                ? t("正在等待目标响应或登录凭据")
                : t("请选择目标后连接当前会话"))}
          </span>
          {status !== "connecting" ? (
            <PermissionButton
              permission="system:vnc:connect"
              type="primary"
              icon={<LinkOutlined />}
              className="vnc-viewer-connect"
              onClick={onConnect}
            >
              {status === "error" ? t("重试") : t("连接")}
            </PermissionButton>
          ) : null}
        </div>
      ) : null}
    </div>
  );
});

export function VncPage({ visible = true }: { visible?: boolean }) {
  const [state, setState] = useState<InitialVncState>(() => initialState());
  const [selectedTargetKey, setSelectedTargetKey] = useState(() => {
    if (typeof localStorage === "undefined") {
      return "local-vnc";
    }
    return localStorage.getItem(vncTargetKeyStorageKey) ?? "local-vnc";
  });
  const [credentialRequest, setCredentialRequest] =
    useState<VncCredentialRequest | null>(null);
  const [credentialUsername, setCredentialUsername] = useState("");
  const [credentialPassword, setCredentialPassword] = useState("");
  const [credentialTarget, setCredentialTarget] = useState("");
  const [rememberCredentials, setRememberCredentials] = useState(false);
  const [mobileControlsOpen, setMobileControlsOpen] = useState(false);
  const [viewerExpanded, setViewerExpanded] = useState(false);
  const [trackpad, setTrackpad] = useState(() =>
    storedFlag(vncTrackpadStorageKey, prefersTrackpad),
  );
  const [showLocalPointer, setShowLocalPointer] = useState(() =>
    storedFlag(vncLocalPointerStorageKey, () => true),
  );
  const [showQuickBar, setShowQuickBar] = useState(() =>
    storedFlag(vncQuickBarStorageKey, () => true),
  );
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [rightClickArmed, setRightClickArmed] = useState(false);
  const [latched, setLatchedState] = useState<ModifierName[]>([]);
  const latchedRef = useRef<ModifierName[]>([]);
  const [remoteClipboards, setRemoteClipboards] = useState<
    Record<string, string>
  >({});
  const runtimesRef = useRef(new Map<string, RfbRuntime>());
  const pendingCredentialSavesRef = useRef(
    new Map<string, PendingVncCredentialSave>(),
  );
  const mobileKeyboardRef = useRef<HTMLInputElement | null>(null);
  const workspaceRef = useRef<HTMLDivElement | null>(null);

  // The viewer fills the space left below the admin header.
  useLayoutEffect(() => {
    const workspace = workspaceRef.current;
    if (!workspace || !visible) {
      return;
    }
    const measure = () => {
      const top = Math.max(
        0,
        Math.round(workspace.getBoundingClientRect().top),
      );
      workspace.style.setProperty("--vnc-workspace-top", `${top}px`);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [visible]);

  // Hides the admin chrome (header, dock) that would cover the remote screen.
  useEffect(() => {
    if (!visible) {
      return;
    }
    document.body.classList.add("vnc-viewer-open");
    return () => document.body.classList.remove("vnc-viewer-open");
  }, [visible]);

  useEffect(() => {
    document.body.classList.toggle(
      "vnc-viewer-expanded",
      visible && viewerExpanded,
    );
    return () => document.body.classList.remove("vnc-viewer-expanded");
  }, [visible, viewerExpanded]);
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const targetsQuery = useQuery({
    queryKey: ["vnc-targets"],
    queryFn: fetchVncTargets,
  });
  const sessionsQuery = useQuery({
    queryKey: ["vnc-sessions"],
    queryFn: fetchVncSessions,
    refetchInterval: visible ? 8000 : false,
  });

  const targetOptions = useMemo(
    () =>
      (targetsQuery.data ?? []).map((target) => ({
        value: target.key,
        label: `${target.name} (${target.host}:${target.port})${
          target.credentials_saved ? t(" · 已保存凭据") : ""
        }`,
      })),
    [targetsQuery.data],
  );

  const serverSessionsById = useMemo(() => {
    const map = new Map<
      string,
      NonNullable<typeof sessionsQuery.data>[number]
    >();
    for (const session of sessionsQuery.data ?? []) {
      map.set(session.id, session);
    }
    return map;
  }, [sessionsQuery.data]);

  const activeSession = state.sessions.find(
    (session) => session.id === state.activeSessionId,
  );
  const selectedTarget = targetsQuery.data?.find(
    (target) => target.key === (activeSession?.targetKey ?? selectedTargetKey),
  );
  const selectedCredentialTarget = targetsQuery.data?.find(
    (target) => target.key === selectedTargetKey,
  );

  const updateSession = useCallback(
    (id: string, patch: Partial<VncSession>) => {
      setState((current) => ({
        ...current,
        sessions: current.sessions.map((session) =>
          session.id === id ? { ...session, ...patch } : session,
        ),
      }));
    },
    [],
  );

  const disconnectRuntime = useCallback((id: string) => {
    pendingCredentialSavesRef.current.delete(id);
    const runtime = runtimesRef.current.get(id);
    if (!runtime?.rfb) {
      return;
    }
    runtime.rfb.disconnect();
    runtime.rfb = null;
  }, []);

  const bindHost = useCallback((id: string, host: HTMLDivElement | null) => {
    const runtime = runtimesRef.current.get(id) ?? { rfb: null, host: null };
    runtime.host = host;
    runtimesRef.current.set(id, runtime);
  }, []);

  const waitForHost = useCallback(async (id: string) => {
    for (let attempt = 0; attempt < 90; attempt += 1) {
      const host = runtimesRef.current.get(id)?.host;
      if (host?.isConnected && host.clientWidth > 0 && host.clientHeight > 0) {
        return host;
      }
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => resolve()),
      );
    }
    throw new Error(t("VNC 画布初始化超时，请切换标签后重试"));
  }, []);

  const attachSession = useCallback(
    async (session: VncSession): Promise<AttachSessionResult> => {
      if (!session.serverSessionId) {
        message.warning(t("请先创建 VNC 会话"));
        return "failed";
      }
      disconnectRuntime(session.id);
      updateSession(session.id, {
        status: "connecting",
        errorMessage: undefined,
      });
      try {
        requireSecureVncContext();
        const host = await waitForHost(session.id);
        const ticket = await createVncSessionTicket(session.serverSessionId);
        let securityFailureMessage: string | undefined;
        const ticketCredentials = {
          username: ticket.username ?? undefined,
          password: ticket.password ?? undefined,
          target: ticket.target ?? undefined,
        };
        const hasTicketCredentials = Object.values(ticketCredentials).some(
          (value) => Boolean(value),
        );
        const rfb = new RFB(
          host,
          buildVncSessionWebSocketUrl(session.serverSessionId, ticket.ticket),
          {
            credentials: hasTicketCredentials ? ticketCredentials : undefined,
          },
        );
        rfb.scaleViewport = true;
        rfb.resizeSession = true;
        rfb.showDotCursor = true;
        rfb.background = "#0b1020";
        rfb.addEventListener("clipboard", (event: Event) => {
          const text = (event as CustomEvent<{ text?: string }>).detail?.text;
          if (typeof text === "string") {
            setRemoteClipboards((current) => ({
              ...current,
              [session.id]: text,
            }));
          }
        });
        rfb.addEventListener("connect", () => {
          updateSession(session.id, {
            status: "connected",
            errorMessage: undefined,
          });
          setCredentialRequest((current) =>
            current?.sessionId === session.id ? null : current,
          );
          const pendingSave = pendingCredentialSavesRef.current.get(session.id);
          if (pendingSave) {
            pendingCredentialSavesRef.current.delete(session.id);
            void saveVncCredentials(pendingSave.credentials)
              .then(async () => {
                await queryClient.invalidateQueries({
                  queryKey: ["vnc-targets"],
                });
                message.success(t("VNC 登录凭据已保存"));
              })
              .catch((error: unknown) => {
                message.warning(
                  error instanceof Error
                    ? error.message
                    : t("VNC 凭据保存失败"),
                );
              });
          }
        });
        rfb.addEventListener("disconnect", (event: Event) => {
          const detail = (event as CustomEvent<{ clean?: boolean }>).detail;
          updateSession(session.id, {
            status: detail?.clean ? "disconnected" : "error",
            errorMessage: detail?.clean
              ? undefined
              : (securityFailureMessage ??
                t(
                  "VNC 握手失败；security type 262/X509Plain 不受支持，请调整目标 VNC 的认证配置",
                )),
          });
          setCredentialRequest((current) =>
            current?.sessionId === session.id ? null : current,
          );
          pendingCredentialSavesRef.current.delete(session.id);
        });
        rfb.addEventListener("credentialsrequired", (event: Event) => {
          const types = (event as CustomEvent<{ types?: string[] }>).detail
            ?.types ?? ["password"];
          const canUseTicketCredentials = types.every((type) => {
            if (type === "username") {
              return Boolean(ticketCredentials.username);
            }
            if (type === "password") {
              return Boolean(ticketCredentials.password);
            }
            if (type === "target") {
              return Boolean(ticketCredentials.target);
            }
            return false;
          });
          if (canUseTicketCredentials) {
            rfb.sendCredentials(ticketCredentials);
            return;
          }
          setCredentialUsername(ticket.username ?? "");
          setCredentialPassword(ticket.password ?? "");
          setCredentialTarget(ticket.target ?? session.targetKey);
          setRememberCredentials(ticket.credentials_saved);
          setCredentialRequest({
            sessionId: session.id,
            targetKey: session.targetKey,
            types,
            credentialsSaved: ticket.credentials_saved,
          });
        });
        rfb.addEventListener("securityfailure", (event: Event) => {
          const detail = (
            event as CustomEvent<{ reason?: string; status?: number }>
          ).detail;
          securityFailureMessage = detail?.reason
            ? t("VNC 身份验证失败：{0}", detail.reason)
            : t("VNC 身份验证失败，请检查用户名和密码");
          if (ticket.credentials_saved) {
            securityFailureMessage += t("；已清除失效的已保存凭据，请重新连接");
            void deleteVncCredentials(session.targetKey)
              .then(() =>
                queryClient.invalidateQueries({ queryKey: ["vnc-targets"] }),
              )
              .catch(() => undefined);
          }
          pendingCredentialSavesRef.current.delete(session.id);
          updateSession(session.id, {
            status: "error",
            errorMessage: securityFailureMessage,
          });
          message.error(securityFailureMessage);
        });
        const runtime = runtimesRef.current.get(session.id) ?? {
          rfb: null,
          host,
        };
        runtime.rfb = rfb;
        runtime.host = host;
        runtimesRef.current.set(session.id, runtime);
        return "attached";
      } catch (error) {
        const reason =
          error instanceof Error ? error.message : t("连接 VNC 失败");
        const staleSession = reason
          .toLocaleLowerCase()
          .includes("vnc session not found");
        updateSession(session.id, {
          status: staleSession ? "idle" : "error",
          serverSessionId: staleSession ? undefined : session.serverSessionId,
          errorMessage: staleSession ? undefined : reason,
        });
        if (staleSession) {
          return "stale";
        }
        message.error(reason);
        return "failed";
      }
    },
    [disconnectRuntime, queryClient, updateSession, waitForHost],
  );

  const createSessionMutation = useMutation({
    mutationFn: ({ target_key }: CreateVncSessionVariables) => {
      requireSecureVncContext();
      return createVncSession({ target_key });
    },
    onSuccess: async (record, variables) => {
      const id = variables.localSessionId ?? sessionId();
      const nextSession: VncSession = {
        id,
        serverSessionId: record.id,
        targetKey: record.target_key,
        targetName: record.target_name,
        status: "idle",
      };
      setState((current) => {
        if (variables.localSessionId) {
          return {
            sessions: current.sessions.map((session) =>
              session.id === variables.localSessionId ? nextSession : session,
            ),
            activeSessionId: variables.localSessionId,
          };
        }
        return {
          sessions: [...current.sessions, nextSession],
          activeSessionId: id,
        };
      });
      await sessionsQuery.refetch();
      await attachSession(nextSession);
    },
    onError: (error) => {
      message.error(
        error instanceof Error ? error.message : t("创建 VNC 会话失败"),
      );
    },
  });

  const closeSessionMutation = useMutation({
    mutationFn: closeVncSession,
    onSuccess: async () => {
      await sessionsQuery.refetch();
    },
  });

  const deleteCredentialsMutation = useMutation({
    mutationFn: deleteVncCredentials,
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["vnc-targets"] });
      message.success(t("已清除当前 VNC 目标的保存凭据"));
    },
    onError: (error) => {
      message.error(
        error instanceof Error ? error.message : t("清除 VNC 凭据失败"),
      );
    },
  });

  useEffect(() => {
    persistSessions(state.sessions, state.activeSessionId);
  }, [state]);

  useEffect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(vncTargetKeyStorageKey, selectedTargetKey);
    }
  }, [selectedTargetKey]);

  useEffect(() => {
    const serverSessions = sessionsQuery.data;
    if (serverSessions === undefined) {
      return;
    }
    setState((current) => {
      const liveServerIds = new Set(
        serverSessions.map((session) => session.id),
      );
      let changed = false;
      const reconciledSessions = current.sessions.map((session) => {
        if (
          session.serverSessionId &&
          !liveServerIds.has(session.serverSessionId)
        ) {
          changed = true;
          return {
            ...session,
            serverSessionId: undefined,
            status: "idle" as const,
            errorMessage: undefined,
          };
        }
        return session;
      });
      const existingServerIds = new Set(
        reconciledSessions
          .map((session) => session.serverSessionId)
          .filter(Boolean),
      );
      const sharedSessions = serverSessions
        .filter((session) => !existingServerIds.has(session.id))
        .map((session) => ({
          id: sessionId(),
          serverSessionId: session.id,
          targetKey: session.target_key,
          targetName: session.target_name,
          status: session.status === "closed" ? "closed" : "idle",
        })) satisfies VncSession[];
      if (sharedSessions.length === 0) {
        return changed ? { ...current, sessions: reconciledSessions } : current;
      }
      return {
        ...current,
        sessions: [...reconciledSessions, ...sharedSessions],
      };
    });
  }, [sessionsQuery.data]);

  useEffect(() => {
    return () => {
      // Keep the host bindings: ref callbacks will not re-register them when
      // StrictMode replays this effect, and unmounting hosts unbind themselves.
      for (const runtime of runtimesRef.current.values()) {
        runtime.rfb?.disconnect();
        runtime.rfb = null;
      }
    };
  }, []);

  useEffect(() => {
    if (!viewerExpanded) {
      return;
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setViewerExpanded(false);
      }
    };
    const frame = requestAnimationFrame(() => {
      window.dispatchEvent(new Event("resize"));
    });
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("keydown", handleKeyDown);
      requestAnimationFrame(() => {
        window.dispatchEvent(new Event("resize"));
      });
    };
  }, [viewerExpanded]);

  const addBlankSession = useCallback(() => {
    const id = sessionId();
    setState((current) => ({
      sessions: [
        ...current.sessions,
        { id, targetKey: selectedTargetKey, status: "idle" },
      ],
      activeSessionId: id,
    }));
  }, [selectedTargetKey]);

  const removeLocalTab = useCallback(
    (id: string) => {
      disconnectRuntime(id);
      runtimesRef.current.delete(id);
      setState((current) => {
        const sessions = current.sessions.filter(
          (session) => session.id !== id,
        );
        const fallback = sessions[0]?.id ?? sessionId();
        return {
          sessions:
            sessions.length > 0
              ? sessions
              : [
                  {
                    id: fallback,
                    targetKey: selectedTargetKey,
                    status: "idle",
                  },
                ],
          activeSessionId:
            current.activeSessionId === id
              ? (sessions[0]?.id ?? fallback)
              : current.activeSessionId,
        };
      });
    },
    [disconnectRuntime, selectedTargetKey],
  );

  const closeServerSession = useCallback(
    async (session: VncSession) => {
      disconnectRuntime(session.id);
      if (session.serverSessionId) {
        await closeSessionMutation.mutateAsync(session.serverSessionId);
      }
      updateSession(session.id, {
        status: "closed",
        serverSessionId: undefined,
      });
    },
    [closeSessionMutation, disconnectRuntime, updateSession],
  );

  const connectActiveSession = useCallback(async () => {
    if (!activeSession) {
      return;
    }
    if (activeSession.serverSessionId) {
      const result = await attachSession(activeSession);
      if (result !== "stale") {
        return;
      }
    }
    createSessionMutation.mutate({
      target_key: activeSession.targetKey || selectedTargetKey,
      localSessionId: activeSession.id,
    });
  }, [activeSession, attachSession, createSessionMutation, selectedTargetKey]);

  const closeCredentialPrompt = useCallback(() => {
    if (credentialRequest) {
      disconnectRuntime(credentialRequest.sessionId);
      updateSession(credentialRequest.sessionId, {
        status: "disconnected",
        errorMessage: t("未提供 VNC 登录凭据"),
      });
    }
    setCredentialRequest(null);
    setCredentialUsername("");
    setCredentialPassword("");
    setCredentialTarget("");
    setRememberCredentials(false);
  }, [credentialRequest, disconnectRuntime, updateSession]);

  const submitCredentials = useCallback(() => {
    if (!credentialRequest) {
      return;
    }
    const needsUsername = credentialRequest.types.includes("username");
    const needsPassword = credentialRequest.types.includes("password");
    const needsTarget = credentialRequest.types.includes("target");
    if (
      (needsUsername && !credentialUsername.trim()) ||
      (needsPassword && !credentialPassword) ||
      (needsTarget && !credentialTarget.trim())
    ) {
      message.warning(t("请填写 VNC 要求的登录凭据"));
      return;
    }
    const rfb = runtimesRef.current.get(credentialRequest.sessionId)?.rfb;
    if (!rfb) {
      message.error(t("VNC 连接已经断开，请重新连接"));
      closeCredentialPrompt();
      return;
    }
    const credentials = {
      username: needsUsername ? credentialUsername.trim() : undefined,
      password: needsPassword ? credentialPassword : undefined,
      target: needsTarget ? credentialTarget.trim() : undefined,
    };
    const savedCredentials = {
      target_key: credentialRequest.targetKey,
      username: credentialUsername.trim() || undefined,
      password: credentialPassword || undefined,
      target: credentialTarget.trim() || undefined,
    };
    const shouldRemember = rememberCredentials;
    const shouldDelete = credentialRequest.credentialsSaved && !shouldRemember;
    if (shouldRemember) {
      pendingCredentialSavesRef.current.set(credentialRequest.sessionId, {
        credentials: savedCredentials,
      });
    } else {
      pendingCredentialSavesRef.current.delete(credentialRequest.sessionId);
    }
    rfb.sendCredentials(credentials);
    setCredentialRequest(null);
    setCredentialUsername("");
    setCredentialPassword("");
    setCredentialTarget("");
    setRememberCredentials(false);

    if (shouldDelete) {
      void deleteVncCredentials(credentialRequest.targetKey)
        .then(() =>
          queryClient.invalidateQueries({ queryKey: ["vnc-targets"] }),
        )
        .catch((error: unknown) => {
          message.warning(
            error instanceof Error ? error.message : t("VNC 凭据清除失败"),
          );
        });
    }
  }, [
    closeCredentialPrompt,
    credentialPassword,
    credentialRequest,
    credentialTarget,
    credentialUsername,
    queryClient,
    rememberCredentials,
  ]);

  const setLatched = useCallback((next: ModifierName[]) => {
    latchedRef.current = next;
    setLatchedState(next);
  }, []);

  const activeRfb = useCallback(() => {
    if (!activeSession || activeSession.status !== "connected") {
      message.warning(t("请先连接 VNC 会话"));
      return null;
    }
    return runtimesRef.current.get(activeSession.id)?.rfb ?? null;
  }, [activeSession]);

  /** Sends one key combination, applying and then clearing latched modifiers. */
  const sendStrokes = useCallback(
    (strokes: KeyStroke[]) => {
      const rfb = activeRfb();
      if (!rfb) {
        return false;
      }
      for (const { stroke, down } of comboSequence(
        withModifiers(latchedRef.current, strokes),
      )) {
        rfb.sendKey(stroke.keysym, stroke.code, down);
      }
      if (latchedRef.current.length > 0) {
        setLatched([]);
      }
      // Keep the phone keyboard open; otherwise give the remote screen focus
      // so a physical keyboard keeps typing into it.
      if (document.activeElement !== mobileKeyboardRef.current) {
        rfb.focus({ preventScroll: true });
      }
      return true;
    },
    [activeRfb, setLatched],
  );

  const sendCharacter = useCallback(
    (character: string) => {
      const stroke =
        latchedRef.current.length > 0 && /^[a-z]$/i.test(character)
          ? letterKey(character)
          : charStroke(character);
      return stroke ? sendStrokes([stroke]) : false;
    },
    [sendStrokes],
  );

  const toggleModifier = useCallback(
    (name: ModifierName) => {
      const current = latchedRef.current;
      setLatched(
        current.includes(name)
          ? current.filter((item) => item !== name)
          : [...current, name],
      );
    },
    [setLatched],
  );

  const toggleTrackpad = useCallback(() => {
    setTrackpad((current) => {
      const next = !current;
      storeFlag(vncTrackpadStorageKey, next);
      message.info(
        next
          ? t("触控板模式：拖动移动光标，轻点单击，双指轻点右键，长按拖拽")
          : t("触摸模式：点哪里就点击哪里"),
      );
      return next;
    });
  }, []);

  const toggleLocalPointer = useCallback(() => {
    setShowLocalPointer((current) => {
      storeFlag(vncLocalPointerStorageKey, !current);
      return !current;
    });
  }, []);

  const toggleQuickBar = useCallback(() => {
    setShowQuickBar((current) => {
      storeFlag(vncQuickBarStorageKey, !current);
      return !current;
    });
  }, []);

  const handleRightClickUsed = useCallback(() => {
    setRightClickArmed(false);
  }, []);

  const pasteToRemote = useCallback(
    (text: string) => {
      const rfb = activeRfb();
      if (!rfb) {
        return false;
      }
      rfb.clipboardPasteFrom(text);
      return true;
    },
    [activeRfb],
  );

  const typeText = useCallback(
    (text: string) => {
      const rfb = activeRfb();
      if (!rfb) {
        return false;
      }
      for (const stroke of textStrokes(text)) {
        rfb.sendKey(stroke.keysym, stroke.code, true);
        rfb.sendKey(stroke.keysym, stroke.code, false);
      }
      return true;
    },
    [activeRfb],
  );

  const openMobileKeyboard = useCallback(() => {
    if (!activeSession || activeSession.status !== "connected") {
      message.warning(t("请先连接 VNC 会话"));
      return;
    }
    mobileKeyboardRef.current?.focus({ preventScroll: true });
  }, [activeSession]);

  const handleMobileKeyboardInput = useCallback(
    (event: FormEvent<HTMLInputElement>) => {
      const inputEvent = event.nativeEvent as InputEvent;
      if (inputEvent.inputType === "deleteContentBackward") {
        event.preventDefault();
        sendStrokes([KEYS.backspace]);
        return;
      }
      if (!inputEvent.data) {
        return;
      }
      event.preventDefault();
      for (const character of Array.from(inputEvent.data)) {
        sendCharacter(character);
      }
      event.currentTarget.value = "";
    },
    [sendCharacter, sendStrokes],
  );

  const handleMobileKeyboardKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLInputElement>) => {
      const supportedKeys: Record<string, KeyStroke> = {
        Enter: KEYS.enter,
        Escape: KEYS.escape,
        Tab: KEYS.tab,
      };
      const stroke = supportedKeys[event.key];
      if (!stroke) {
        return;
      }
      event.preventDefault();
      sendStrokes([stroke]);
    },
    [sendStrokes],
  );

  const sessionTitle = (session: VncSession, index: number) => {
    const serverSession = session.serverSessionId
      ? serverSessionsById.get(session.serverSessionId)
      : undefined;
    return (
      session.targetName ?? serverSession?.target_name ?? `VNC ${index + 1}`
    );
  };
  const activeConnected = activeSession?.status === "connected";

  return (
    <div className="vnc-page">
      <div ref={workspaceRef} className="vnc-workspace">
        <div
          className={`vnc-viewer-shell${viewerExpanded ? " is-expanded" : ""}`}
        >
          <section className="vnc-viewer-card">
            <div className="vnc-mobile-header">
              <Button
                type="text"
                icon={<ArrowLeftOutlined />}
                aria-label={t("退出 VNC")}
                onClick={() => navigate("/admin/dashboard")}
              />
              <div className="vnc-mobile-title">
                <Typography.Text strong>
                  {selectedTarget?.name ?? activeSession?.targetName ?? "VNC"}
                </Typography.Text>
                <Typography.Text type="secondary">
                  {activeSession
                    ? statusLabel(activeSession.status)
                    : t("无会话")}
                </Typography.Text>
              </div>
              <Space size={2}>
                <Button
                  type="text"
                  icon={<DesktopOutlined />}
                  aria-label={t("连接当前 VNC")}
                  loading={createSessionMutation.isPending}
                  onClick={connectActiveSession}
                />
                <Button
                  type="text"
                  icon={<FontSizeOutlined />}
                  aria-label={t("打开键盘")}
                  disabled={activeSession?.status !== "connected"}
                  onClick={openMobileKeyboard}
                />
                <Button
                  type="text"
                  icon={<MoreOutlined />}
                  aria-label={t("VNC 控制")}
                  onPointerDown={(event) => event.stopPropagation()}
                  onPointerUp={(event) => {
                    event.stopPropagation();
                    if (event.pointerType !== "mouse") {
                      setMobileControlsOpen(true);
                    }
                  }}
                  onClick={(event) => {
                    event.stopPropagation();
                    setMobileControlsOpen(true);
                  }}
                />
              </Space>
            </div>
            <div
              className={`vnc-window-bar${state.sessions.length <= 1 ? " is-single" : ""}`}
            >
              <div
                className="vnc-tabstrip"
                role="tablist"
                aria-label={t("VNC 会话")}
              >
                {state.sessions.map((session, index) => {
                  const active = session.id === state.activeSessionId;
                  const title = sessionTitle(session, index);
                  return (
                    <div
                      key={session.id}
                      className={`vnc-tab${active ? " is-active" : ""}`}
                    >
                      <button
                        type="button"
                        role="tab"
                        aria-selected={active}
                        className="vnc-tab-main"
                        title={`${title} · ${statusLabel(session.status)}`}
                        onClick={() =>
                          setState((current) => ({
                            ...current,
                            activeSessionId: session.id,
                          }))
                        }
                      >
                        <span
                          className={`vnc-status-dot is-${session.status}`}
                          aria-label={statusLabel(session.status)}
                        />
                        <span className="vnc-tab-title">{title}</span>
                      </button>
                      {state.sessions.length > 1 && (
                        <button
                          type="button"
                          className="vnc-tab-close"
                          aria-label={t("关闭标签")}
                          onClick={() => removeLocalTab(session.id)}
                        >
                          <CloseOutlined />
                        </button>
                      )}
                    </div>
                  );
                })}
                <Tooltip title={t("新建标签")}>
                  <Button
                    type="text"
                    size="small"
                    className="vnc-tab-add"
                    icon={<PlusOutlined />}
                    aria-label={t("新建标签")}
                    onClick={addBlankSession}
                  />
                </Tooltip>
              </div>
              <div className="vnc-window-actions">
                <Select
                  size="small"
                  variant="borderless"
                  className="vnc-target-select"
                  popupMatchSelectWidth={false}
                  value={selectedTargetKey}
                  options={targetOptions}
                  loading={targetsQuery.isLoading}
                  onChange={setSelectedTargetKey}
                />
                <Tooltip
                  title={activeConnected ? t("重新连接") : t("连接当前标签")}
                >
                  <PermissionButton
                    permission="system:vnc:connect"
                    type="text"
                    size="small"
                    icon={<LinkOutlined />}
                    aria-label={t("连接当前标签")}
                    loading={createSessionMutation.isPending}
                    onClick={connectActiveSession}
                  />
                </Tooltip>
                <Tooltip title={t("在新标签中连接所选目标")}>
                  <PermissionButton
                    permission="system:vnc:connect"
                    type="text"
                    size="small"
                    icon={<DesktopOutlined />}
                    aria-label={t("新建连接")}
                    onClick={() =>
                      createSessionMutation.mutate({
                        target_key: selectedTargetKey,
                      })
                    }
                  />
                </Tooltip>
                <Tooltip title={t("刷新会话")}>
                  <Button
                    type="text"
                    size="small"
                    icon={<ReloadOutlined />}
                    aria-label={t("刷新会话")}
                    onClick={() => void sessionsQuery.refetch()}
                  />
                </Tooltip>
                {selectedCredentialTarget?.credentials_saved ? (
                  <Popconfirm
                    title={t("清除已保存的 VNC 凭据？")}
                    okText={t("清除")}
                    cancelText={t("取消")}
                    onConfirm={() =>
                      deleteCredentialsMutation.mutate(selectedTargetKey)
                    }
                  >
                    <Tooltip title={t("清除已保存凭据")}>
                      <Button
                        type="text"
                        size="small"
                        icon={<KeyOutlined />}
                        aria-label={t("清除已保存的 VNC 凭据")}
                        loading={deleteCredentialsMutation.isPending}
                      />
                    </Tooltip>
                  </Popconfirm>
                ) : null}
                <Tooltip title={t("快捷键与文本发送")}>
                  <Button
                    type="text"
                    size="small"
                    icon={<AppstoreOutlined />}
                    aria-label={t("快捷键与文本发送")}
                    onClick={() => setShortcutsOpen(true)}
                  />
                </Tooltip>
                <Tooltip
                  title={
                    showQuickBar ? t("隐藏快捷操作栏") : t("显示快捷操作栏")
                  }
                >
                  <Button
                    type="text"
                    size="small"
                    className={showQuickBar ? "is-active" : undefined}
                    icon={<ThunderboltOutlined />}
                    aria-label={t("快捷操作栏")}
                    aria-pressed={showQuickBar}
                    onClick={toggleQuickBar}
                  />
                </Tooltip>
                <Tooltip title={viewerExpanded ? t("退出全屏") : t("全屏显示")}>
                  <Button
                    type="text"
                    size="small"
                    icon={
                      viewerExpanded ? (
                        <FullscreenExitOutlined />
                      ) : (
                        <FullscreenOutlined />
                      )
                    }
                    aria-label={viewerExpanded ? t("退出全屏") : t("全屏显示")}
                    onClick={() => setViewerExpanded((current) => !current)}
                  />
                </Tooltip>
                <Popconfirm
                  title={t("关闭共享会话？")}
                  description={t("其他设备上打开的这个会话也会断开")}
                  okText={t("关闭")}
                  cancelText={t("取消")}
                  disabled={!activeSession?.serverSessionId}
                  onConfirm={() =>
                    activeSession && void closeServerSession(activeSession)
                  }
                >
                  <Tooltip title={t("关闭共享会话")}>
                    <Button
                      type="text"
                      size="small"
                      danger
                      icon={<DisconnectOutlined />}
                      aria-label={t("关闭共享会话")}
                      disabled={!activeSession?.serverSessionId}
                      loading={closeSessionMutation.isPending}
                    />
                  </Tooltip>
                </Popconfirm>
              </div>
            </div>
            <div className="vnc-viewer-area">
              {state.sessions.length > 0 ? (
                state.sessions.map((session) => (
                  <VncViewerHost
                    key={session.id}
                    sessionId={session.id}
                    active={state.activeSessionId === session.id}
                    status={session.status}
                    errorMessage={session.errorMessage}
                    trackpad={trackpad}
                    rightClickArmed={rightClickArmed}
                    showLocalPointer={showLocalPointer}
                    onBind={bindHost}
                    onConnect={connectActiveSession}
                    onRightClickUsed={handleRightClickUsed}
                  />
                ))
              ) : (
                <Empty description={t("暂无 VNC 会话")} />
              )}
            </div>
            {activeConnected ? (
              <div
                className={`vnc-quickbar-shell${showQuickBar ? "" : " is-collapsed"}`}
              >
                <VncQuickBar
                  connected={activeConnected}
                  trackpad={trackpad}
                  rightClickArmed={rightClickArmed}
                  latched={latched}
                  onOpenKeyboard={openMobileKeyboard}
                  onToggleTrackpad={toggleTrackpad}
                  onToggleRightClick={() =>
                    setRightClickArmed((current) => !current)
                  }
                  onToggleModifier={toggleModifier}
                  onStrokes={sendStrokes}
                  onOpenShortcuts={() => setShortcutsOpen(true)}
                />
              </div>
            ) : null}
            {activeSession ? (
              <div className="vnc-statusbar">
                <span className="vnc-statusbar-item">
                  <DesktopOutlined />{" "}
                  {activeSession.targetName ??
                    selectedTarget?.name ??
                    selectedTargetKey}
                </span>
                <span className="vnc-statusbar-item">
                  <span
                    className={`vnc-status-dot is-${activeSession.status}`}
                  />
                  {statusLabel(activeSession.status)}
                </span>
                {activeSession.errorMessage ? (
                  <span className="vnc-statusbar-item is-error">
                    {activeSession.errorMessage}
                  </span>
                ) : null}
              </div>
            ) : null}
          </section>
        </div>
      </div>
      <input
        ref={mobileKeyboardRef}
        className="vnc-mobile-keyboard-input"
        type="text"
        inputMode="text"
        autoCapitalize="none"
        autoCorrect="off"
        autoComplete="off"
        aria-label={t("VNC 远程键盘输入")}
        onBeforeInput={handleMobileKeyboardInput}
        onKeyDown={handleMobileKeyboardKeyDown}
        onChange={(event) => {
          event.currentTarget.value = "";
        }}
      />
      <Drawer
        title={t("VNC 控制")}
        open={mobileControlsOpen}
        placement="bottom"
        height="auto"
        zIndex={1600}
        rootClassName="vnc-mobile-controls-drawer-root"
        className="vnc-mobile-controls-drawer"
        onClose={() => setMobileControlsOpen(false)}
      >
        <Space direction="vertical" size={12} className="full-width">
          <Select
            className="full-width"
            value={selectedTargetKey}
            options={targetOptions}
            loading={targetsQuery.isLoading}
            onChange={setSelectedTargetKey}
          />
          <div className="vnc-mobile-control-grid">
            <Button
              type="primary"
              icon={<DesktopOutlined />}
              loading={createSessionMutation.isPending}
              onClick={() => {
                setMobileControlsOpen(false);
                connectActiveSession();
              }}
            >
              {t("连接当前")}
            </Button>
            <PermissionButton
              permission="system:vnc:connect"
              icon={<PlusOutlined />}
              loading={createSessionMutation.isPending}
              onClick={() => {
                setMobileControlsOpen(false);
                createSessionMutation.mutate({
                  target_key: selectedTargetKey,
                });
              }}
            >
              {t("新建连接")}
            </PermissionButton>
            <Button icon={<LinkOutlined />} onClick={addBlankSession}>
              {t("新建标签")}
            </Button>
            <Button
              icon={<ReloadOutlined />}
              onClick={() => void sessionsQuery.refetch()}
            >
              {t("刷新会话")}
            </Button>
            {selectedCredentialTarget?.credentials_saved ? (
              <Button
                danger
                icon={<DeleteOutlined />}
                loading={deleteCredentialsMutation.isPending}
                onClick={() => {
                  deleteCredentialsMutation.mutate(selectedTargetKey);
                  setMobileControlsOpen(false);
                }}
              >
                {t("清除保存凭据")}
              </Button>
            ) : null}
            <Button
              icon={<DisconnectOutlined />}
              danger
              disabled={!activeSession?.serverSessionId}
              loading={closeSessionMutation.isPending}
              onClick={() => {
                if (activeSession) {
                  void closeServerSession(activeSession);
                }
                setMobileControlsOpen(false);
              }}
            >
              {t("关闭共享会话")}
            </Button>
            <Button
              icon={<CloseOutlined />}
              disabled={!activeSession}
              onClick={() => {
                if (activeSession) {
                  removeLocalTab(activeSession.id);
                }
                setMobileControlsOpen(false);
              }}
            >
              {t("关闭标签")}
            </Button>
          </div>
        </Space>
      </Drawer>
      <VncShortcutsDrawer
        open={shortcutsOpen}
        connected={activeConnected}
        trackpad={trackpad}
        showLocalPointer={showLocalPointer}
        remoteClipboard={
          activeSession ? (remoteClipboards[activeSession.id] ?? "") : ""
        }
        onClose={() => setShortcutsOpen(false)}
        onStrokes={sendStrokes}
        onToggleTrackpad={toggleTrackpad}
        onToggleLocalPointer={toggleLocalPointer}
        onPasteToRemote={pasteToRemote}
        onTypeText={typeText}
      />
      <Modal
        title={
          <Space size={8}>
            <KeyOutlined />
            <span>{t("VNC 登录")}</span>
          </Space>
        }
        open={Boolean(credentialRequest)}
        zIndex={1700}
        rootClassName="vnc-credential-modal-root"
        okText={t("连接")}
        cancelText={t("取消")}
        onOk={submitCredentials}
        onCancel={closeCredentialPrompt}
        destroyOnClose
      >
        <Space direction="vertical" size="middle" className="full-width">
          {credentialRequest?.types.includes("username") ? (
            <Input
              autoFocus
              name="vnc-username"
              autoComplete="off"
              data-1p-ignore="true"
              data-lpignore="true"
              placeholder={t("用户名")}
              value={credentialUsername}
              onChange={(event) => setCredentialUsername(event.target.value)}
              onPressEnter={submitCredentials}
            />
          ) : null}
          {credentialRequest?.types.includes("password") ? (
            <Input.Password
              autoFocus={!credentialRequest.types.includes("username")}
              name="vnc-password"
              autoComplete="new-password"
              data-1p-ignore="true"
              data-lpignore="true"
              placeholder={t("VNC 密码")}
              value={credentialPassword}
              onChange={(event) => setCredentialPassword(event.target.value)}
              onPressEnter={submitCredentials}
            />
          ) : null}
          {credentialRequest?.types.includes("target") ? (
            <Input
              placeholder={t("目标")}
              value={credentialTarget}
              onChange={(event) => setCredentialTarget(event.target.value)}
              onPressEnter={submitCredentials}
            />
          ) : null}
          <Checkbox
            checked={rememberCredentials}
            onChange={(event) => setRememberCredentials(event.target.checked)}
          >
            {t("记住此 VNC 目标的凭据")}
          </Checkbox>
        </Space>
      </Modal>
    </div>
  );
}
