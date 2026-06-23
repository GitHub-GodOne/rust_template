import {
  CloseOutlined,
  DesktopOutlined,
  DisconnectOutlined,
  LinkOutlined,
  PlusOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import RFB from "@novnc/novnc";
import { useMutation, useQuery } from "@tanstack/react-query";
import {
  Button,
  Card,
  Empty,
  Select,
  Space,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import type { TabsProps } from "antd";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  buildVncSessionWebSocketUrl,
  closeVncSession,
  createVncSession,
  createVncSessionTicket,
  fetchVncSessions,
  fetchVncTargets,
} from "../../../api/admin/vnc";
import { CrudPage } from "../../../components/admin/CrudPage";
import { PermissionButton } from "../../../components/admin/PermissionButton";

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

type VncViewerHostProps = {
  sessionId: string;
  active: boolean;
  onBind: (sessionId: string, host: HTMLDivElement | null) => void;
};

const vncTargetKeyStorageKey = "gpt-images-admin-vnc-target";
const vncSessionsStorageKey = "gpt-images-admin-vnc-sessions";
const vncActiveSessionStorageKey = "gpt-images-admin-vnc-active-session";

function sessionId() {
  return `vnc-session-${crypto.randomUUID()}`;
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
  onBind,
}: VncViewerHostProps) {
  const bindRef = useCallback(
    (host: HTMLDivElement | null) => {
      onBind(sessionId, host);
    },
    [onBind, sessionId],
  );

  return (
    <div
      className="vnc-viewer-stage"
      style={{ display: active ? "block" : "none" }}
    >
      <div ref={bindRef} className="vnc-viewer-host" />
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
  const runtimesRef = useRef(new Map<string, RfbRuntime>());

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
        label: `${target.name} (${target.host}:${target.port})`,
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

  const attachSession = useCallback(
    async (session: VncSession) => {
      if (!session.serverSessionId) {
        message.warning("请先创建 VNC 会话");
        return;
      }
      const runtime = runtimesRef.current.get(session.id);
      if (!runtime?.host) {
        message.warning("VNC 画布尚未准备好");
        return;
      }
      disconnectRuntime(session.id);
      updateSession(session.id, { status: "connecting" });
      try {
        const ticket = await createVncSessionTicket(session.serverSessionId);
        const rfb = new RFB(
          runtime.host,
          buildVncSessionWebSocketUrl(session.serverSessionId, ticket.ticket),
          {
            credentials: ticket.password
              ? { password: ticket.password }
              : undefined,
          },
        );
        rfb.scaleViewport = true;
        rfb.resizeSession = true;
        rfb.background = "#0b1020";
        rfb.addEventListener("connect", () => {
          updateSession(session.id, { status: "connected" });
        });
        rfb.addEventListener("disconnect", (event: Event) => {
          const detail = (event as CustomEvent<{ clean?: boolean }>).detail;
          updateSession(session.id, {
            status: detail?.clean ? "disconnected" : "error",
          });
        });
        rfb.addEventListener("credentialsrequired", () => {
          if (ticket.password) {
            rfb.sendCredentials({ password: ticket.password });
          }
        });
        runtime.rfb = rfb;
      } catch (error) {
        updateSession(session.id, { status: "error" });
        message.error(error instanceof Error ? error.message : "连接 VNC 失败");
      }
    },
    [disconnectRuntime, updateSession],
  );

  const createSessionMutation = useMutation({
    mutationFn: ({ target_key }: CreateVncSessionVariables) =>
      createVncSession({ target_key }),
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
      window.setTimeout(() => {
        void attachSession(nextSession);
      }, 0);
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

  useEffect(() => {
    persistSessions(state.sessions, state.activeSessionId);
  }, [state]);

  useEffect(() => {
    if (typeof localStorage !== "undefined") {
      localStorage.setItem(vncTargetKeyStorageKey, selectedTargetKey);
    }
  }, [selectedTargetKey]);

  useEffect(() => {
    setState((current) => {
      const existingServerIds = new Set(
        current.sessions
          .map((session) => session.serverSessionId)
          .filter(Boolean),
      );
      const sharedSessions = (sessionsQuery.data ?? [])
        .filter((session) => !existingServerIds.has(session.id))
        .map((session) => ({
          id: sessionId(),
          serverSessionId: session.id,
          targetKey: session.target_key,
          targetName: session.target_name,
          status: session.status === "closed" ? "closed" : "idle",
        })) satisfies VncSession[];
      if (sharedSessions.length === 0) {
        return current;
      }
      return { ...current, sessions: [...current.sessions, ...sharedSessions] };
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

  const connectActiveSession = useCallback(() => {
    if (!activeSession) {
      return;
    }
    if (activeSession.serverSessionId) {
      void attachSession(activeSession);
      return;
    }
    createSessionMutation.mutate({
      target_key: selectedTargetKey,
      localSessionId: activeSession.id,
    });
  }, [activeSession, attachSession, createSessionMutation, selectedTargetKey]);

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
          onBind={bindHost}
        />
      ),
    };
  });

  return (
    <CrudPage
      title="VNC 管理"
      description="集中管理本机和远程 VNC 连接，支持多会话标签和跨设备共享会话列表。"
      extra={
        <Space wrap>
          <Select
            className="vnc-target-select"
            value={selectedTargetKey}
            options={targetOptions}
            loading={targetsQuery.isLoading}
            onChange={setSelectedTargetKey}
          />
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
      <Card className="vnc-viewer-card">
        <Space className="vnc-toolbar" wrap>
          <Button
            icon={<DesktopOutlined />}
            type="primary"
            onClick={connectActiveSession}
          >
            连接当前
          </Button>
          <Button icon={<LinkOutlined />} onClick={addBlankSession}>
            新建标签
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
            关闭共享会话
          </Button>
          <Button
            icon={<CloseOutlined />}
            disabled={!activeSession}
            onClick={() => activeSession && removeLocalTab(activeSession.id)}
          >
            关闭标签
          </Button>
          {activeSession ? (
            <Typography.Text type="secondary">
              当前目标：{activeSession.targetName ?? selectedTargetKey}
            </Typography.Text>
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
      </Card>
    </CrudPage>
  );
}
