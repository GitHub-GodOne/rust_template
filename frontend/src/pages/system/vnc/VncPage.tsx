import {
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
  Tabs,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import type { TabsProps } from "antd";
import type {
  FormEvent,
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import { CrudPage } from "../../../components/admin/CrudPage";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { createClientId } from "../../../utils/clientId";

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
  onBind: (sessionId: string, host: HTMLDivElement | null) => void;
};

const vncTargetKeyStorageKey = "gpt-images-admin-vnc-target";
const vncSessionsStorageKey = "gpt-images-admin-vnc-sessions";
const vncActiveSessionStorageKey = "gpt-images-admin-vnc-active-session";
const insecureVncContextMessage =
  "VNC 需要 HTTPS 安全环境，请使用 HTTPS 域名访问，或在服务器本机通过 localhost 打开";

function requireSecureVncContext() {
  if (!window.isSecureContext) {
    throw new Error(insecureVncContextMessage);
  }
}

function sessionId() {
  return createClientId("vnc-session");
}

function statusColor(status: VncStatus) {
  if (status === "connected") {
    return "green";
  }
  if (status === "connecting") {
    return "processing";
  }
  if (status === "disconnected") {
    return "blue";
  }
  if (status === "error") {
    return "red";
  }
  return "default";
}

function statusLabel(status: VncStatus) {
  if (status === "connected") {
    return "已连接";
  }
  if (status === "connecting") {
    return "连接中";
  }
  if (status === "disconnected") {
    return "已断开本机";
  }
  if (status === "closed") {
    return "已关闭";
  }
  if (status === "error") {
    return "异常";
  }
  return "未连接";
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
  onBind,
}: VncViewerHostProps) {
  const pointerRef = useRef<HTMLSpanElement | null>(null);
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
      const bounds = event.currentTarget.getBoundingClientRect();
      pointer.style.transform = `translate3d(${event.clientX - bounds.left}px, ${event.clientY - bounds.top}px, 0) translate(-50%, -50%)`;
      pointer.classList.toggle("is-touch", event.pointerType === "touch");
      pointer.classList.add("is-visible");
    },
    [status],
  );
  const hidePointer = useCallback(() => {
    pointerRef.current?.classList.remove("is-visible");
  }, []);

  return (
    <div
      className="vnc-viewer-stage"
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
      {status !== "connected" ? (
        <div className="vnc-viewer-state" aria-live="polite">
          <DesktopOutlined />
          <strong>
            {status === "connecting"
              ? "正在建立 VNC 连接"
              : status === "error"
                ? "VNC 连接失败"
                : "VNC 尚未连接"}
          </strong>
          <span>
            {errorMessage ??
              (status === "connecting"
                ? "正在等待目标响应或登录凭据"
                : "请选择目标后连接当前会话")}
          </span>
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
  const runtimesRef = useRef(new Map<string, RfbRuntime>());
  const pendingCredentialSavesRef = useRef(
    new Map<string, PendingVncCredentialSave>(),
  );
  const mobileKeyboardRef = useRef<HTMLInputElement | null>(null);
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
          target.credentials_saved ? " · 已保存凭据" : ""
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
    throw new Error("VNC 画布初始化超时，请切换标签后重试");
  }, []);

  const attachSession = useCallback(
    async (session: VncSession): Promise<AttachSessionResult> => {
      if (!session.serverSessionId) {
        message.warning("请先创建 VNC 会话");
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
                message.success("VNC 登录凭据已保存");
              })
              .catch((error: unknown) => {
                message.warning(
                  error instanceof Error ? error.message : "VNC 凭据保存失败",
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
                "VNC 握手失败；security type 262/X509Plain 不受支持，请调整目标 VNC 的认证配置"),
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
            ? `VNC 身份验证失败：${detail.reason}`
            : "VNC 身份验证失败，请检查用户名和密码";
          if (ticket.credentials_saved) {
            securityFailureMessage += "；已清除失效的已保存凭据，请重新连接";
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
        const reason = error instanceof Error ? error.message : "连接 VNC 失败";
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
        error instanceof Error ? error.message : "创建 VNC 会话失败",
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
      message.success("已清除当前 VNC 目标的保存凭据");
    },
    onError: (error) => {
      message.error(
        error instanceof Error ? error.message : "清除 VNC 凭据失败",
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
      for (const runtime of runtimesRef.current.values()) {
        runtime.rfb?.disconnect();
      }
      runtimesRef.current.clear();
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
        errorMessage: "未提供 VNC 登录凭据",
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
      message.warning("请填写 VNC 要求的登录凭据");
      return;
    }
    const rfb = runtimesRef.current.get(credentialRequest.sessionId)?.rfb;
    if (!rfb) {
      message.error("VNC 连接已经断开，请重新连接");
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
            error instanceof Error ? error.message : "VNC 凭据清除失败",
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

  const dispatchVirtualKey = useCallback(
    (key: string, code = "Unidentified") => {
      if (!activeSession || activeSession.status !== "connected") {
        return false;
      }
      const canvas = runtimesRef.current
        .get(activeSession.id)
        ?.host?.querySelector("canvas");
      if (!canvas) {
        return false;
      }
      const eventOptions: KeyboardEventInit = {
        key,
        code,
        bubbles: true,
        cancelable: true,
      };
      canvas.dispatchEvent(new KeyboardEvent("keydown", eventOptions));
      canvas.dispatchEvent(new KeyboardEvent("keyup", eventOptions));
      return true;
    },
    [activeSession],
  );

  const openMobileKeyboard = useCallback(() => {
    if (!activeSession || activeSession.status !== "connected") {
      message.warning("请先连接 VNC 会话");
      return;
    }
    mobileKeyboardRef.current?.focus({ preventScroll: true });
  }, [activeSession]);

  const handleMobileKeyboardInput = useCallback(
    (event: FormEvent<HTMLInputElement>) => {
      const inputEvent = event.nativeEvent as InputEvent;
      if (inputEvent.inputType === "deleteContentBackward") {
        event.preventDefault();
        dispatchVirtualKey("Backspace", "Backspace");
        return;
      }
      if (!inputEvent.data) {
        return;
      }
      event.preventDefault();
      for (const character of Array.from(inputEvent.data)) {
        dispatchVirtualKey(character);
      }
      event.currentTarget.value = "";
    },
    [dispatchVirtualKey],
  );

  const handleMobileKeyboardKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLInputElement>) => {
      const supportedKeys: Record<string, string> = {
        Enter: "Enter",
        Escape: "Escape",
        Tab: "Tab",
      };
      const code = supportedKeys[event.key];
      if (!code) {
        return;
      }
      event.preventDefault();
      dispatchVirtualKey(event.key, code);
    },
    [dispatchVirtualKey],
  );

  const tabItems: TabsProps["items"] = state.sessions.map((session, index) => {
    const serverSession = session.serverSessionId
      ? serverSessionsById.get(session.serverSessionId)
      : undefined;
    const title =
      session.targetName ?? serverSession?.target_name ?? `VNC ${index + 1}`;
    return {
      key: session.id,
      label: (
        <Space size={6} className="vnc-tab-label">
          <span>{title}</span>
          <Tag color={statusColor(session.status)}>
            {statusLabel(session.status)}
          </Tag>
        </Space>
      ),
      children: (
        <VncViewerHost
          sessionId={session.id}
          active={state.activeSessionId === session.id}
          status={session.status}
          errorMessage={session.errorMessage}
          onBind={bindHost}
        />
      ),
    };
  });

  return (
    <CrudPage
      title="VNC 管理"
      subtitle="集中管理本机和远程 VNC 连接，支持多会话标签和跨设备共享会话列表"
      breadcrumb={["远程管理", "VNC 管理"]}
      icon={<DesktopOutlined />}
      className="vnc-page"
      toolbar={
        <Space wrap>
          <Select
            className="vnc-target-select"
            value={selectedTargetKey}
            options={targetOptions}
            loading={targetsQuery.isLoading}
            onChange={setSelectedTargetKey}
          />
          {selectedCredentialTarget?.credentials_saved ? (
            <Popconfirm
              title="清除已保存的 VNC 凭据？"
              okText="清除"
              cancelText="取消"
              onConfirm={() =>
                deleteCredentialsMutation.mutate(selectedTargetKey)
              }
            >
              <Tooltip title="清除已保存凭据">
                <Button
                  danger
                  aria-label="清除已保存的 VNC 凭据"
                  icon={<DeleteOutlined />}
                  loading={deleteCredentialsMutation.isPending}
                />
              </Tooltip>
            </Popconfirm>
          ) : null}
          <PermissionButton
            permission="system:vnc:connect"
            type="primary"
            icon={<PlusOutlined />}
            loading={createSessionMutation.isPending}
            onClick={() =>
              createSessionMutation.mutate({ target_key: selectedTargetKey })
            }
          >
            新建连接
          </PermissionButton>
          <Button
            icon={<ReloadOutlined />}
            onClick={() => void sessionsQuery.refetch()}
          >
            刷新会话
          </Button>
        </Space>
      }
    >
      <div
        className={`vnc-viewer-shell${viewerExpanded ? " is-expanded" : ""}`}
      >
        <section className="vnc-viewer-card">
          <div className="vnc-mobile-header">
            <Button
              type="text"
              icon={<ArrowLeftOutlined />}
              aria-label="退出 VNC"
              onClick={() => navigate("/admin/dashboard")}
            />
            <div className="vnc-mobile-title">
              <Typography.Text strong>
                {selectedTarget?.name ?? activeSession?.targetName ?? "VNC"}
              </Typography.Text>
              <Typography.Text type="secondary">
                {activeSession ? statusLabel(activeSession.status) : "无会话"}
              </Typography.Text>
            </div>
            <Space size={2}>
              <Button
                type="text"
                icon={<DesktopOutlined />}
                aria-label="连接当前 VNC"
                loading={createSessionMutation.isPending}
                onClick={connectActiveSession}
              />
              <Button
                type="text"
                icon={<FontSizeOutlined />}
                aria-label="打开键盘"
                disabled={activeSession?.status !== "connected"}
                onClick={openMobileKeyboard}
              />
              <Button
                type="text"
                icon={<MoreOutlined />}
                aria-label="VNC 控制"
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
          <Space className="vnc-toolbar" wrap>
            <Button
              icon={<DesktopOutlined />}
              type="primary"
              onClick={connectActiveSession}
            >
              <span className="vnc-action-label">连接当前</span>
            </Button>
            <Button icon={<LinkOutlined />} onClick={addBlankSession}>
              <span className="vnc-action-label">新建标签</span>
            </Button>
            <Button
              icon={<DisconnectOutlined />}
              danger
              disabled={!activeSession?.serverSessionId}
              loading={closeSessionMutation.isPending}
              onClick={() =>
                activeSession && void closeServerSession(activeSession)
              }
            >
              <span className="vnc-action-label">关闭共享会话</span>
            </Button>
            <Button
              icon={<CloseOutlined />}
              disabled={!activeSession}
              onClick={() => activeSession && removeLocalTab(activeSession.id)}
            >
              <span className="vnc-action-label">关闭标签</span>
            </Button>
            <Button
              icon={
                viewerExpanded ? (
                  <FullscreenExitOutlined />
                ) : (
                  <FullscreenOutlined />
                )
              }
              aria-label={viewerExpanded ? "退出全屏" : "全屏显示"}
              onClick={() => setViewerExpanded((current) => !current)}
            >
              <span className="vnc-action-label">
                {viewerExpanded ? "退出全屏" : "全屏显示"}
              </span>
            </Button>
            {activeSession ? (
              <Space
                direction="vertical"
                size={0}
                className="vnc-toolbar-status"
              >
                <Typography.Text type="secondary">
                  当前目标：{activeSession.targetName ?? selectedTargetKey}
                </Typography.Text>
                {activeSession.errorMessage ? (
                  <Typography.Text type="danger">
                    {activeSession.errorMessage}
                  </Typography.Text>
                ) : null}
              </Space>
            ) : null}
          </Space>

          {state.sessions.length > 0 ? (
            <Tabs
              className="vnc-session-tabs"
              type="card"
              activeKey={state.activeSessionId}
              items={tabItems}
              onChange={(activeSessionId) =>
                setState((current) => ({ ...current, activeSessionId }))
              }
            />
          ) : (
            <Empty description="暂无 VNC 会话" />
          )}
        </section>
      </div>
      <input
        ref={mobileKeyboardRef}
        className="vnc-mobile-keyboard-input"
        type="text"
        inputMode="text"
        autoCapitalize="none"
        autoCorrect="off"
        autoComplete="off"
        aria-label="VNC 远程键盘输入"
        onBeforeInput={handleMobileKeyboardInput}
        onKeyDown={handleMobileKeyboardKeyDown}
        onChange={(event) => {
          event.currentTarget.value = "";
        }}
      />
      <Drawer
        title="VNC 控制"
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
              连接当前
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
              新建连接
            </PermissionButton>
            <Button icon={<LinkOutlined />} onClick={addBlankSession}>
              新建标签
            </Button>
            <Button
              icon={<ReloadOutlined />}
              onClick={() => void sessionsQuery.refetch()}
            >
              刷新会话
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
                清除保存凭据
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
              关闭共享会话
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
              关闭标签
            </Button>
          </div>
        </Space>
      </Drawer>
      <Modal
        title={
          <Space size={8}>
            <KeyOutlined />
            <span>VNC 登录</span>
          </Space>
        }
        open={Boolean(credentialRequest)}
        zIndex={1700}
        rootClassName="vnc-credential-modal-root"
        okText="连接"
        cancelText="取消"
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
              placeholder="用户名"
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
              placeholder="VNC 密码"
              value={credentialPassword}
              onChange={(event) => setCredentialPassword(event.target.value)}
              onPressEnter={submitCredentials}
            />
          ) : null}
          {credentialRequest?.types.includes("target") ? (
            <Input
              placeholder="目标"
              value={credentialTarget}
              onChange={(event) => setCredentialTarget(event.target.value)}
              onPressEnter={submitCredentials}
            />
          ) : null}
          <Checkbox
            checked={rememberCredentials}
            onChange={(event) => setRememberCredentials(event.target.checked)}
          >
            记住此 VNC 目标的凭据
          </Checkbox>
        </Space>
      </Modal>
    </CrudPage>
  );
}
