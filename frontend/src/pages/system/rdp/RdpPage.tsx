import {
  AppstoreOutlined,
  ArrowLeftOutlined,
  CloseOutlined,
  CompressOutlined,
  DesktopOutlined,
  DisconnectOutlined,
  FontSizeOutlined,
  FullscreenExitOutlined,
  FullscreenOutlined,
  LinkOutlined,
  LockOutlined,
  MoreOutlined,
  OneToOneOutlined,
  PlusOutlined,
  ReloadOutlined,
  SettingOutlined,
  SnippetsOutlined,
  ThunderboltOutlined,
  WindowsOutlined,
} from "@ant-design/icons";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Checkbox,
  Drawer,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tooltip,
  Typography,
  message,
} from "antd";
import Guacamole from "guacamole-common-js";
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
  type RdpGatewayStatus,
  type RdpLogin,
  type RdpTarget,
  createRdpTicket,
  deleteRdpCredentials,
  fetchRdpCredentialStatus,
  fetchRdpGatewayStatus,
  fetchRdpTargets,
  rdpSocketUrl,
  saveRdpTargets,
} from "../../../api/admin/rdp";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";
import { useAuthStore } from "../../../stores/auth";
import { createClientId } from "../../../utils/clientId";
import "../vnc/vnc.css";
import {
  RemoteClipboardDrawer,
  TrackpadCursor,
  VncQuickBar,
  VncShortcutsDrawer,
} from "../vnc/VncQuickBar";
import { useVncPointerModes } from "../vnc/useVncPointerModes";
import {
  prefersTrackpad,
  storeFlag,
  storeValue,
  storedFlag,
  storedValue,
} from "../vnc/viewerPrefs";
import {
  KEYS,
  type KeyStroke,
  type ModifierName,
  type Point,
  centroid,
  charStroke,
  comboSequence,
  letterKey,
  spread,
  textStrokes,
  withModifiers,
} from "../vnc/vncInput";
import { attachGuacamoleInput, guacamoleSurface } from "./guacamolePointer";
import { RemoteViewport, type ViewportState } from "./remoteViewport";

type SessionStatus = "connecting" | "connected" | "disconnected" | "error";

type Session = {
  id: string;
  target: RdpTarget;
  ticket: string;
  status: SessionStatus;
  error?: string;
};

const MAX_SESSIONS = 8;
/** Project CLI page with the gateway installer selected. */
const INSTALL_GATEWAY_PATH = "/admin/system/project-cli?cmd=rdp%20install";
const KEY_CTRL_ALT_DEL = [KEYS.ctrl, KEYS.alt, KEYS.delete];
const KEY_WINDOWS = [KEYS.meta];
const rdpTrackpadStorageKey = "gpt-images-admin-rdp-trackpad";
const rdpLocalPointerStorageKey = "gpt-images-admin-rdp-local-pointer";
const rdpQuickBarStorageKey = "gpt-images-admin-rdp-quickbar";
const rdpResolutionStorageKey = "gpt-images-admin-rdp-resolution";
/** "auto" sizes the remote desktop to the window; the rest are fixed sizes. */
const RESOLUTIONS = [
  "auto",
  "1280x720",
  "1366x768",
  "1600x900",
  "1920x1080",
  "2560x1440",
] as const;

/**
 * Phones get a real desktop resolution to zoom around in; a window-sized
 * session would squeeze the remote desktop into the phone's shape.
 */
function defaultResolution() {
  const narrow =
    typeof window !== "undefined" &&
    Math.min(window.innerWidth, window.innerHeight) < 600;
  return prefersTrackpad() || narrow ? "1920x1080" : "auto";
}

function resolutionSize(resolution: string, area: HTMLElement | null) {
  const [width, height] = resolution.split("x").map(Number);
  if (width && height) {
    return { width, height };
  }
  return {
    width: Math.min(area?.clientWidth || window.innerWidth, 1920),
    height: Math.min(Math.max((area?.clientHeight ?? 900) - 64, 600), 1200),
  };
}

// Not in the typings; it is the AudioContext guacamole-common-js plays sound into.
const audioContexts = (
  Guacamole as unknown as {
    AudioContextFactory: { getAudioContext(): AudioContext | null };
  }
).AudioContextFactory;

// Browsers keep audio suspended until the user interacts with the page.
const resumeAudio = () => {
  const context = audioContexts.getAudioContext();
  if (context?.state === "suspended") void context.resume();
};

function statusLabel(status: SessionStatus) {
  if (status === "connected") {
    return t("已连接");
  }
  if (status === "connecting") {
    return t("连接中");
  }
  if (status === "disconnected") {
    return t("已断开");
  }
  return t("异常");
}

function gatewayHint(gateway: RdpGatewayStatus) {
  if (!gateway.local) {
    return t("RDP_GUACD_ADDRESS 指向其他机器，请在那台机器上启动 guacd。");
  }
  if (gateway.runtime) {
    return t("网关由后台自动启动和守护，退出后会自动重试。");
  }
  if (gateway.platform === "windows") {
    return t(
      "Windows 会把 guacd 装进 WSL 中的 Alpine Linux，装好后由后台自动启动，无需 Docker。",
    );
  }
  if (gateway.platform === "macos") {
    return t(
      "macOS 没有现成的 guacd 安装包：请从源码编译 guacamole-server，或把 RDP_GUACD_ADDRESS 指向其他机器的 guacd。",
    );
  }
  return t(
    "Linux 使用系统软件包（apk / apt / dnf）安装 guacd，装好后由后台自动启动。",
  );
}

type DesktopProps = {
  session: Session;
  active: boolean;
  trackpad: boolean;
  rightClickArmed: boolean;
  showLocalPointer: boolean;
  onStatus: (id: string, status: SessionStatus, error?: string) => void;
  onClient: (id: string, client?: Guacamole.Client) => void;
  onClipboard: (id: string, text: string) => void;
  onReconnect: (session: Session) => void;
  onRightClickUsed: () => void;
  onViewport: (id: string, viewport?: RemoteViewport) => void;
  onZoomedChange: (id: string, zoomed: boolean) => void;
};

/** Finger travel (px) before a one-finger touch pans instead of clicking. */
const PAN_SLOP = 8;

function touchPoints(list: TouchList): Point[] {
  return Array.from(list, (touch) => ({ x: touch.clientX, y: touch.clientY }));
}

const Desktop = memo(function Desktop({
  session,
  active,
  trackpad,
  rightClickArmed,
  showLocalPointer,
  onStatus,
  onClient,
  onClipboard,
  onReconnect,
  onRightClickUsed,
  onViewport,
  onZoomedChange,
}: DesktopProps) {
  const host = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const pointerRef = useRef<HTMLSpanElement>(null);
  const trackpadCursorRef = useRef<HTMLSpanElement>(null);
  const keyboardRef = useRef<Guacamole.Keyboard>();
  const clientRef = useRef<Guacamole.Client>();
  const viewportRef = useRef<RemoteViewport>();
  const trackpadRef = useRef(trackpad);
  trackpadRef.current = trackpad;
  const [view, setView] = useState<ViewportState>();
  const surface = useMemo(() => guacamoleSurface(() => clientRef.current), []);
  const { id, ticket, status } = session;
  const pinchView = useCallback(
    (x: number, y: number, scale: number, deltaX: number, deltaY: number) => {
      viewportRef.current?.zoomAt(x, y, scale);
      viewportRef.current?.panBy(deltaX, deltaY);
    },
    [],
  );
  const followCursor = useCallback((x: number, y: number) => {
    viewportRef.current?.follow(x, y);
  }, []);
  useVncPointerModes({
    stageRef,
    cursorRef: trackpadCursorRef,
    enabled: active && status === "connected",
    trackpad,
    rightClickArmed,
    onRightClickUsed,
    surface,
    onPinch: pinchView,
    onCursorMove: followCursor,
  });

  useEffect(() => {
    const container = host.current;
    if (!container) return;
    const tunnel = new Guacamole.WebSocketTunnel(rdpSocketUrl(ticket));
    const client = new Guacamole.Client(tunnel);
    clientRef.current = client;
    onClient(id, client);
    onStatus(id, "connecting");
    const display = client.getDisplay();
    const element = display.getElement();
    container.appendChild(element);
    let zoomed = false;
    const viewport = new RemoteViewport(container, display, (state) => {
      setView(state);
      if (state.zoomed !== zoomed) {
        zoomed = state.zoomed;
        onZoomedChange(id, zoomed);
      }
    });
    viewportRef.current = viewport;
    onViewport(id, viewport);
    const layout = () => viewport.layout();
    display.onresize = layout;
    const observer = new ResizeObserver(layout);
    observer.observe(container);
    const removeMouseInput = attachGuacamoleInput(element, surface, () =>
      container.focus({ preventScroll: true }),
    );
    // A mouse near the edge of a zoomed view scrolls it along.
    const handleMouseMove = (event: PointerEvent) => {
      if (event.pointerType === "mouse") {
        viewport.follow(event.clientX, event.clientY, 24);
      }
    };
    // Ctrl + wheel (and laptop trackpad pinches) zoom the view.
    const handleWheel = (event: WheelEvent) => {
      if (!event.ctrlKey) return;
      event.preventDefault();
      event.stopPropagation();
      viewport.zoomAt(
        event.clientX,
        event.clientY,
        Math.exp(-event.deltaY / 300),
      );
    };
    // Touch mode: taps click (Guacamole's touchscreen), one finger drags the
    // view and two fingers pinch-zoom it. Trackpad mode handles touches in
    // useVncPointerModes before they get here.
    let touchView: {
      last: Point;
      spread: number;
      travel: number;
      multi: boolean;
    } | null = null;
    const handleTouchStart = (event: TouchEvent) => {
      if (trackpadRef.current) return;
      const points = touchPoints(event.touches);
      touchView = {
        last: centroid(points),
        spread: spread(points),
        travel: touchView?.travel ?? 0,
        multi: (touchView?.multi ?? false) || points.length > 1,
      };
    };
    const handleTouchMove = (event: TouchEvent) => {
      if (trackpadRef.current || !touchView || event.touches.length === 0) {
        return;
      }
      const points = touchPoints(event.touches);
      const next = centroid(points);
      const deltaX = next.x - touchView.last.x;
      const deltaY = next.y - touchView.last.y;
      touchView.travel += Math.hypot(deltaX, deltaY);
      if (points.length > 1) {
        const nextSpread = spread(points);
        if (touchView.spread > 0) {
          viewport.zoomAt(next.x, next.y, nextSpread / touchView.spread);
        }
        viewport.panBy(deltaX, deltaY);
        touchView.spread = nextSpread;
      } else if (!touchView.multi && touchView.travel > PAN_SLOP) {
        viewport.panBy(deltaX, deltaY);
      }
      touchView.last = next;
    };
    const handleTouchEnd = (event: TouchEvent) => {
      if (!touchView) return;
      const points = touchPoints(event.touches);
      if (points.length === 0) {
        touchView = null;
        return;
      }
      touchView.last = centroid(points);
      touchView.spread = spread(points);
    };
    const captureOptions = { capture: true, passive: false } as const;
    container.addEventListener("pointermove", handleMouseMove);
    container.addEventListener("wheel", handleWheel, captureOptions);
    container.addEventListener("touchstart", handleTouchStart, captureOptions);
    container.addEventListener("touchmove", handleTouchMove, captureOptions);
    container.addEventListener("touchend", handleTouchEnd, captureOptions);
    container.addEventListener("touchcancel", handleTouchEnd, captureOptions);
    const keyboard = new Guacamole.Keyboard(container);
    keyboardRef.current = keyboard;
    keyboard.onkeydown = (key) => {
      resumeAudio();
      client.sendKeyEvent(1, key);
      return false;
    };
    keyboard.onkeyup = (key) => client.sendKeyEvent(0, key);
    const release = () => keyboard.reset();
    container.addEventListener("pointerdown", resumeAudio);
    container.addEventListener("blur", release);
    window.addEventListener("blur", release);
    client.onerror = (failure) => {
      onStatus(id, "error", failure.message ?? t("RDP 连接失败"));
      client.disconnect();
    };
    tunnel.onerror = (failure) => {
      onStatus(id, "error", failure.message ?? t("RDP 网关连接失败"));
      client.disconnect();
    };
    client.onstatechange = (state) => {
      if (state === 3) onStatus(id, "connected");
      if (state === 5) onStatus(id, "disconnected");
    };
    client.onclipboard = (stream, mimetype) => {
      if (!mimetype.startsWith("text/")) return;
      const reader = new Guacamole.StringReader(stream);
      let text = "";
      reader.ontext = (chunk) => {
        text += chunk;
      };
      reader.onend = () => onClipboard(id, text);
    };
    // React StrictMode may mount and clean up effects once before the real mount.
    const connectTimer = window.setTimeout(() => client.connect(), 0);
    return () => {
      window.clearTimeout(connectTimer);
      keyboard.reset();
      keyboard.onkeydown = null;
      keyboard.onkeyup = null;
      window.removeEventListener("blur", release);
      container.removeEventListener("blur", release);
      container.removeEventListener("pointerdown", resumeAudio);
      container.removeEventListener("pointermove", handleMouseMove);
      container.removeEventListener("wheel", handleWheel, captureOptions);
      container.removeEventListener(
        "touchstart",
        handleTouchStart,
        captureOptions,
      );
      container.removeEventListener(
        "touchmove",
        handleTouchMove,
        captureOptions,
      );
      container.removeEventListener("touchend", handleTouchEnd, captureOptions);
      container.removeEventListener(
        "touchcancel",
        handleTouchEnd,
        captureOptions,
      );
      observer.disconnect();
      removeMouseInput();
      display.onresize = null;
      client.onerror = null;
      client.onstatechange = null;
      client.onclipboard = null;
      tunnel.onerror = null;
      client.disconnect();
      element.remove();
      clientRef.current = undefined;
      viewportRef.current = undefined;
      onClient(id, undefined);
      onViewport(id, undefined);
      onZoomedChange(id, false);
    };
  }, [
    id,
    ticket,
    onClient,
    onClipboard,
    onStatus,
    onViewport,
    onZoomedChange,
    surface,
  ]);

  useEffect(() => {
    if (!active) keyboardRef.current?.reset();
  }, [active]);

  const updatePointer = (event: ReactPointerEvent<HTMLDivElement>) => {
    const pointer = pointerRef.current;
    if (!pointer || status !== "connected") return;
    // Trackpad mode draws its own cursor; the finger position is irrelevant.
    if (!showLocalPointer || (trackpad && event.pointerType === "touch")) {
      pointer.classList.remove("is-visible");
      return;
    }
    const bounds = event.currentTarget.getBoundingClientRect();
    pointer.style.transform = `translate3d(${event.clientX - bounds.left}px, ${event.clientY - bounds.top}px, 0) translate(-50%, -50%)`;
    pointer.classList.toggle("is-touch", event.pointerType === "touch");
    pointer.classList.add("is-visible");
  };
  const hidePointer = () => pointerRef.current?.classList.remove("is-visible");

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
        if (event.pointerType === "touch") hidePointer();
      }}
    >
      <span ref={pointerRef} className="vnc-local-pointer" aria-hidden="true" />
      <TrackpadCursor ref={trackpadCursorRef} />
      {status === "connected" && view?.zoomed ? (
        <div className="rdp-zoom-badge">
          <span>{Math.round(view.scale * 100)}%</span>
          <button type="button" onClick={() => viewportRef.current?.fit()}>
            {t("适应窗口")}
          </button>
        </div>
      ) : null}
      <div
        ref={host}
        // biome-ignore lint/a11y/noNoninteractiveTabindex: The remote desktop receives keyboard input.
        tabIndex={0}
        data-rdp-session={id}
        className="vnc-viewer-host rdp-desktop"
        role="application"
        aria-label={t(
          "{0} 远程桌面，点击后使用键盘和鼠标操作",
          session.target.name,
        )}
      />
      {status !== "connected" ? (
        <div className="vnc-viewer-state" aria-live="polite">
          <DesktopOutlined />
          <strong>
            {status === "connecting"
              ? t("正在建立 RDP 连接")
              : status === "error"
                ? t("RDP 连接失败")
                : t("RDP 连接已断开")}
          </strong>
          <span>
            {session.error ??
              (status === "connecting"
                ? t("正在登录 {0}", session.target.name)
                : t("可使用已保存的凭据重新连接"))}
          </span>
          {status === "error" ? (
            <span>
              {t("检查目标电脑的远程桌面、账号密码、证书和 RDP 网关。")}
            </span>
          ) : null}
          {status !== "connecting" ? (
            <PermissionButton
              permission="system:rdp:connect"
              type="primary"
              icon={<LinkOutlined />}
              className="vnc-viewer-connect"
              onClick={() => onReconnect(session)}
            >
              {status === "error" ? t("重试") : t("重新连接")}
            </PermissionButton>
          ) : null}
        </div>
      ) : null}
    </div>
  );
});

export function RdpPage({ visible = true }: { visible?: boolean }) {
  const navigate = useNavigate();
  const cache = useQueryClient();
  const targets = useQuery({
    queryKey: ["rdp-targets"],
    queryFn: fetchRdpTargets,
  });
  const canConnect = useAuthStore((s) => s.hasPermission("system:rdp:connect"));
  const canEdit = useAuthStore((s) => s.hasPermission("system:rdp:update"));
  const [selected, setSelected] = useState<string>();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<string>();
  const [reconnectId, setReconnectId] = useState<string>();
  const [loginOpen, setLoginOpen] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);
  const [mobileControlsOpen, setMobileControlsOpen] = useState(false);
  const [viewerExpanded, setViewerExpanded] = useState(false);
  const [editing, setEditing] = useState<RdpTarget | null>();
  const [busy, setBusy] = useState(false);
  const [manualLogin, setManualLogin] = useState(false);
  const [rememberCredentials, setRememberCredentials] = useState(true);
  const [trackpad, setTrackpad] = useState(() =>
    storedFlag(rdpTrackpadStorageKey, prefersTrackpad),
  );
  const [showLocalPointer, setShowLocalPointer] = useState(() =>
    storedFlag(rdpLocalPointerStorageKey, () => true),
  );
  const [showQuickBar, setShowQuickBar] = useState(() =>
    storedFlag(rdpQuickBarStorageKey, () => true),
  );
  const [resolution, setResolution] = useState(() => {
    const stored = storedValue(rdpResolutionStorageKey, defaultResolution);
    return (RESOLUTIONS as readonly string[]).includes(stored)
      ? stored
      : defaultResolution();
  });
  const [zoomedSessions, setZoomedSessions] = useState<string[]>([]);
  const viewports = useRef(new Map<string, RemoteViewport>());
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [clipboardOpen, setClipboardOpen] = useState(false);
  const [rightClickArmed, setRightClickArmed] = useState(false);
  const [latched, setLatchedState] = useState<ModifierName[]>([]);
  const latchedRef = useRef<ModifierName[]>([]);
  const [remoteClipboards, setRemoteClipboards] = useState<
    Record<string, string>
  >({});
  const clients = useRef(new Map<string, Guacamole.Client>());
  const workspaceRef = useRef<HTMLDivElement>(null);
  const mobileKeyboardRef = useRef<HTMLInputElement>(null);
  const gateway = useQuery({
    queryKey: ["rdp-gateway"],
    queryFn: fetchRdpGatewayStatus,
    enabled: visible,
    refetchOnWindowFocus: true,
    // Poll while the backend is still bringing the gateway up.
    refetchInterval: (query) =>
      query.state.data && !query.state.data.reachable ? 5000 : false,
  });
  const credentials = useQuery({
    queryKey: ["rdp-credentials", selected],
    queryFn: () => fetchRdpCredentialStatus(selected as string),
    enabled: loginOpen && !!selected && canConnect,
    staleTime: 0,
  });
  const useSavedCredentials = credentials.data?.saved === true && !manualLogin;
  const [loginForm] = Form.useForm<RdpLogin>();
  const [targetForm] = Form.useForm<RdpTarget>();
  const selectedTarget = targets.data?.find(
    (target) => target.key === selected,
  );
  const activeSession = sessions.find((session) => session.id === active);
  const activeConnected = activeSession?.status === "connected";
  const activeZoomed = !!active && zoomedSessions.includes(active);
  const permissionBlocked = canConnect
    ? undefined
    : t(
        "当前账号没有 RDP 连接权限，请管理员分配 system:rdp:connect 权限后重新登录。",
      );
  const connectionBlocked =
    permissionBlocked ??
    (sessions.length >= MAX_SESSIONS
      ? t("最多同时打开 8 个连接标签，请先关闭一个标签。")
      : selectedTarget && !selectedTarget.enabled
        ? t("此电脑已停用，请在管理电脑中编辑并启用。")
        : undefined);
  const targetOptions = targets.data?.map((target) => ({
    value: target.key,
    label: `${target.name} (${target.host}:${target.port})${target.enabled ? "" : t(" · 已停用")}`,
    disabled: !target.enabled,
  }));

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
    if (!visible || !viewerExpanded) {
      return;
    }
    document.body.classList.add("vnc-viewer-expanded");
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setViewerExpanded(false);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      document.body.classList.remove("vnc-viewer-expanded");
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [visible, viewerExpanded]);

  useEffect(() => {
    const list = targets.data;
    if (!list) return;
    setSelected((current) =>
      list.some((target) => target.key === current)
        ? current
        : list.find((target) => target.enabled)?.key,
    );
  }, [targets.data]);

  const handleStatus = useCallback(
    (id: string, status: SessionStatus, error?: string) => {
      setSessions((old) =>
        old.map((session) => {
          if (session.id !== id) return session;
          // Keep the reason when the failed connection closes afterwards.
          if (status === "disconnected" && session.status === "error") {
            return session;
          }
          return {
            ...session,
            status,
            error: status === "error" ? error : undefined,
          };
        }),
      );
    },
    [],
  );

  const handleClient = useCallback((id: string, client?: Guacamole.Client) => {
    if (client) clients.current.set(id, client);
    else clients.current.delete(id);
  }, []);

  const handleViewport = useCallback(
    (id: string, viewport?: RemoteViewport) => {
      if (viewport) viewports.current.set(id, viewport);
      else viewports.current.delete(id);
    },
    [],
  );

  const handleZoomedChange = useCallback((id: string, zoomed: boolean) => {
    setZoomedSessions((old) => {
      const has = old.includes(id);
      if (has === zoomed) return old;
      return zoomed ? [...old, id] : old.filter((item) => item !== id);
    });
  }, []);

  /** Switches the active desktop between "fit the window" and 1:1 pixels. */
  const toggleActualSize = () => {
    const viewport = active ? viewports.current.get(active) : undefined;
    if (!viewport) return;
    if (viewport.zoomed) viewport.fit();
    else viewport.actualSize();
  };

  const changeResolution = (value: string) => {
    storeValue(rdpResolutionStorageKey, value);
    setResolution(value);
  };

  const openLogin = (target: RdpTarget | undefined, reconnect?: string) => {
    if (!target) {
      message.warning(t("请先选择或添加一台远程电脑"));
      return;
    }
    if (!target.enabled) {
      message.warning(t("此电脑已停用，请先编辑并启用"));
      return;
    }
    if (permissionBlocked) {
      message.warning(permissionBlocked);
      return;
    }
    if (!reconnect && sessions.length >= MAX_SESSIONS) {
      message.warning(t("最多同时打开 8 个连接标签，请先关闭一个标签。"));
      return;
    }
    setSelected(target.key);
    setReconnectId(reconnect);
    setManualLogin(false);
    setRememberCredentials(true);
    loginForm.resetFields();
    setManageOpen(false);
    setMobileControlsOpen(false);
    setLoginOpen(true);
  };
  const openLoginRef = useRef(openLogin);
  openLoginRef.current = openLogin;

  // Stable for the memoized desktops; always calls the latest openLogin.
  const reconnect = useCallback(
    (session: Session) => {
      const target =
        targets.data?.find((item) => item.key === session.target.key) ??
        session.target;
      openLoginRef.current(target, session.id);
    },
    [targets.data],
  );

  const connect = async () => {
    const values = useSavedCredentials ? {} : await loginForm.validateFields();
    const target = targets.data?.find(
      (item) => item.key === selected && item.enabled,
    );
    if (!target) {
      message.warning(t("所选电脑已被删除或停用，请重新选择"));
      return;
    }
    setBusy(true);
    try {
      const area = workspaceRef.current;
      const result = await createRdpTicket({
        ...values,
        target_key: target.key,
        ...resolutionSize(resolution, area),
        use_saved_credentials: useSavedCredentials,
        remember_credentials: !useSavedCredentials && rememberCredentials,
      });
      await cache.invalidateQueries({
        queryKey: ["rdp-credentials", target.key],
      });
      const fresh = {
        target,
        ticket: result.ticket,
        status: "connecting" as const,
        error: undefined,
      };
      if (reconnectId && sessions.some((s) => s.id === reconnectId)) {
        setSessions((old) =>
          old.map((s) => (s.id === reconnectId ? { ...s, ...fresh } : s)),
        );
        setActive(reconnectId);
      } else {
        const session = { id: createClientId("rdp"), ...fresh };
        setSessions((old) => [...old, session]);
        setActive(session.id);
      }
      setReconnectId(undefined);
      setLoginOpen(false);
      loginForm.resetFields();
    } catch {
      message.error(t("创建 RDP 连接失败，请检查权限和目标配置"));
    } finally {
      setBusy(false);
    }
  };

  const closeTab = (id: string) => {
    clients.current.get(id)?.disconnect();
    const remaining = sessions.filter((session) => session.id !== id);
    setSessions(remaining);
    if (active === id) setActive(remaining[remaining.length - 1]?.id);
  };

  const disconnectActive = () => {
    if (active) clients.current.get(active)?.disconnect();
  };

  const handleClipboard = useCallback((id: string, text: string) => {
    setRemoteClipboards((old) => ({ ...old, [id]: text }));
  }, []);

  const activeClient = () => {
    const client =
      activeConnected && active ? clients.current.get(active) : undefined;
    if (!client) message.warning(t("请先连接 RDP 会话"));
    return client;
  };

  const setLatched = (next: ModifierName[]) => {
    latchedRef.current = next;
    setLatchedState(next);
  };

  /** Sends one key combination, applying and then clearing latched modifiers. */
  const sendStrokes = (strokes: KeyStroke[]) => {
    const client = activeClient();
    if (!client) return false;
    for (const { stroke, down } of comboSequence(
      withModifiers(latchedRef.current, strokes),
    )) {
      client.sendKeyEvent(down ? 1 : 0, stroke.keysym);
    }
    if (latchedRef.current.length > 0) setLatched([]);
    // Keep the phone keyboard open; otherwise give the remote screen focus
    // so a physical keyboard keeps typing into it.
    if (document.activeElement !== mobileKeyboardRef.current) {
      document
        .querySelector<HTMLElement>(`[data-rdp-session="${active}"]`)
        ?.focus({ preventScroll: true });
    }
    return true;
  };

  const sendCharacter = (character: string) => {
    const stroke =
      latchedRef.current.length > 0 && /^[a-z]$/i.test(character)
        ? letterKey(character)
        : charStroke(character);
    return stroke ? sendStrokes([stroke]) : false;
  };

  const toggleModifier = (name: ModifierName) => {
    const current = latchedRef.current;
    setLatched(
      current.includes(name)
        ? current.filter((item) => item !== name)
        : [...current, name],
    );
  };

  const toggleTrackpad = () => {
    const next = !trackpad;
    storeFlag(rdpTrackpadStorageKey, next);
    setTrackpad(next);
    message.info(
      next
        ? t(
            "触控板模式：拖动移动光标，轻点单击，双指轻点右键，长按拖拽，双指张合缩放",
          )
        : t("触摸模式：点哪里就点击哪里，单指拖动画面，双指张合缩放"),
    );
  };

  const toggleLocalPointer = () => {
    storeFlag(rdpLocalPointerStorageKey, !showLocalPointer);
    setShowLocalPointer(!showLocalPointer);
  };

  const toggleQuickBar = () => {
    storeFlag(rdpQuickBarStorageKey, !showQuickBar);
    setShowQuickBar(!showQuickBar);
  };

  const handleRightClickUsed = useCallback(() => setRightClickArmed(false), []);

  const pasteToRemote = (text: string) => {
    const client = activeClient();
    if (!client) return false;
    const writer = new Guacamole.StringWriter(
      client.createClipboardStream("text/plain"),
    );
    writer.sendText(text);
    writer.sendEnd();
    return true;
  };

  const typeText = (text: string) => {
    const client = activeClient();
    if (!client) return false;
    for (const stroke of textStrokes(text)) {
      client.sendKeyEvent(1, stroke.keysym);
      client.sendKeyEvent(0, stroke.keysym);
    }
    return true;
  };

  const openMobileKeyboard = () => {
    if (activeClient()) {
      mobileKeyboardRef.current?.focus({ preventScroll: true });
    }
  };

  const handleMobileKeyboardInput = (event: FormEvent<HTMLInputElement>) => {
    const inputEvent = event.nativeEvent as InputEvent;
    if (inputEvent.inputType === "deleteContentBackward") {
      event.preventDefault();
      sendStrokes([KEYS.backspace]);
      return;
    }
    if (!inputEvent.data) return;
    event.preventDefault();
    for (const character of Array.from(inputEvent.data)) {
      sendCharacter(character);
    }
    event.currentTarget.value = "";
  };

  const handleMobileKeyboardKeyDown = (
    event: ReactKeyboardEvent<HTMLInputElement>,
  ) => {
    const supportedKeys: Record<string, KeyStroke> = {
      Enter: KEYS.enter,
      Escape: KEYS.escape,
      Tab: KEYS.tab,
    };
    const stroke = supportedKeys[event.key];
    if (!stroke) return;
    event.preventDefault();
    sendStrokes([stroke]);
  };

  const refresh = () => {
    void targets.refetch();
    void gateway.refetch();
  };

  const save = async (list: RdpTarget[], selectKey?: string) => {
    setBusy(true);
    try {
      const result = await saveRdpTargets(list);
      cache.setQueryData(["rdp-targets"], result);
      setEditing(undefined);
      if (selectKey) {
        setSelected(selectKey);
        setManageOpen(false);
      }
      message.success(t("目标配置已保存"));
    } catch {
      message.error(t("保存失败，请检查权限、主机和端口"));
    } finally {
      setBusy(false);
    }
  };

  const gatewayDown = gateway.data && !gateway.data.reachable;
  const canInstallGateway =
    gateway.data?.local &&
    !gateway.data.runtime &&
    gateway.data.platform !== "macos";

  return (
    <div className="vnc-page rdp-page">
      <div ref={workspaceRef} className="vnc-workspace">
        <div
          className={`vnc-viewer-shell${viewerExpanded ? " is-expanded" : ""}`}
        >
          <section className="vnc-viewer-card">
            <div className="vnc-mobile-header">
              <Button
                type="text"
                icon={<ArrowLeftOutlined />}
                aria-label={t("退出 RDP")}
                onClick={() => navigate("/admin/dashboard")}
              />
              <div className="vnc-mobile-title">
                <Typography.Text strong>
                  {activeSession?.target.name ?? selectedTarget?.name ?? "RDP"}
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
                  aria-label={t("新建连接")}
                  disabled={!!connectionBlocked}
                  onClick={() => openLogin(selectedTarget)}
                />
                <Button
                  type="text"
                  icon={<FontSizeOutlined />}
                  aria-label={t("打开键盘")}
                  disabled={!activeConnected}
                  onClick={openMobileKeyboard}
                />
                <Button
                  type="text"
                  icon={<MoreOutlined />}
                  aria-label={t("RDP 控制")}
                  onClick={() => setMobileControlsOpen(true)}
                />
              </Space>
            </div>
            <div
              className={`vnc-window-bar${sessions.length <= 1 ? " is-single" : ""}`}
            >
              <div
                className="vnc-tabstrip"
                role="tablist"
                aria-label={t("RDP 会话")}
              >
                {sessions.map((session) => {
                  const isActive = session.id === active;
                  return (
                    <div
                      key={session.id}
                      className={`vnc-tab${isActive ? " is-active" : ""}`}
                    >
                      <button
                        type="button"
                        role="tab"
                        aria-selected={isActive}
                        className="vnc-tab-main"
                        title={`${session.target.name} · ${statusLabel(session.status)}`}
                        onClick={() => setActive(session.id)}
                      >
                        <span
                          className={`vnc-status-dot is-${session.status}`}
                          aria-label={statusLabel(session.status)}
                        />
                        <span className="vnc-tab-title">
                          {session.target.name}
                        </span>
                      </button>
                      <button
                        type="button"
                        className="vnc-tab-close"
                        aria-label={t("关闭标签")}
                        onClick={() => closeTab(session.id)}
                      >
                        <CloseOutlined />
                      </button>
                    </div>
                  );
                })}
                <Tooltip title={t("在新标签中连接所选电脑")}>
                  <Button
                    type="text"
                    size="small"
                    className="vnc-tab-add"
                    icon={<PlusOutlined />}
                    aria-label={t("新建标签")}
                    disabled={!!connectionBlocked}
                    onClick={() => openLogin(selectedTarget)}
                  />
                </Tooltip>
              </div>
              <div className="vnc-window-actions">
                <Select
                  size="small"
                  variant="borderless"
                  className="vnc-target-select"
                  popupMatchSelectWidth={false}
                  placeholder={t("选择远程电脑")}
                  aria-label={t("选择远程电脑")}
                  showSearch
                  optionFilterProp="label"
                  value={selected}
                  options={targetOptions}
                  loading={targets.isLoading}
                  onChange={setSelected}
                />
                <Tooltip
                  title={
                    activeSession ? t("在当前标签重新连接") : t("连接所选电脑")
                  }
                >
                  <PermissionButton
                    permission="system:rdp:connect"
                    type="text"
                    size="small"
                    icon={<LinkOutlined />}
                    aria-label={t("连接当前标签")}
                    disabled={!selectedTarget?.enabled}
                    onClick={() => openLogin(selectedTarget, activeSession?.id)}
                  />
                </Tooltip>
                <Tooltip title={t("在新标签中连接所选电脑")}>
                  <PermissionButton
                    permission="system:rdp:connect"
                    type="text"
                    size="small"
                    icon={<DesktopOutlined />}
                    aria-label={t("新建连接")}
                    disabled={!!connectionBlocked}
                    onClick={() => openLogin(selectedTarget)}
                  />
                </Tooltip>
                {canEdit ? (
                  <Tooltip title={t("管理电脑")}>
                    <Button
                      type="text"
                      size="small"
                      icon={<SettingOutlined />}
                      aria-label={t("管理电脑")}
                      onClick={() => setManageOpen(true)}
                    />
                  </Tooltip>
                ) : null}
                <Tooltip title={t("刷新")}>
                  <Button
                    type="text"
                    size="small"
                    icon={<ReloadOutlined />}
                    aria-label={t("刷新")}
                    onClick={refresh}
                  />
                </Tooltip>
                <Tooltip title={t("发送 Ctrl+Alt+Del")}>
                  <Button
                    type="text"
                    size="small"
                    icon={<LockOutlined />}
                    aria-label={t("发送 Ctrl+Alt+Del")}
                    disabled={!activeConnected}
                    onClick={() => sendStrokes(KEY_CTRL_ALT_DEL)}
                  />
                </Tooltip>
                <Tooltip title={t("发送 Windows 键")}>
                  <Button
                    type="text"
                    size="small"
                    icon={<WindowsOutlined />}
                    aria-label={t("发送 Windows 键")}
                    disabled={!activeConnected}
                    onClick={() => sendStrokes(KEY_WINDOWS)}
                  />
                </Tooltip>
                <Tooltip title={t("剪贴板：粘贴到远程 / 复制远程内容")}>
                  <Button
                    type="text"
                    size="small"
                    icon={<SnippetsOutlined />}
                    aria-label={t("剪贴板")}
                    disabled={!activeConnected}
                    onClick={() => setClipboardOpen(true)}
                  />
                </Tooltip>
                <Tooltip title={t("快捷键")}>
                  <Button
                    type="text"
                    size="small"
                    icon={<AppstoreOutlined />}
                    aria-label={t("快捷键")}
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
                <Tooltip
                  title={activeZoomed ? t("适应窗口") : t("原始大小 1:1")}
                >
                  <Button
                    type="text"
                    size="small"
                    icon={
                      activeZoomed ? <CompressOutlined /> : <OneToOneOutlined />
                    }
                    aria-label={
                      activeZoomed ? t("适应窗口") : t("原始大小 1:1")
                    }
                    disabled={!activeConnected}
                    onClick={toggleActualSize}
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
                <Tooltip title={t("断开连接")}>
                  <Button
                    type="text"
                    size="small"
                    danger
                    icon={<DisconnectOutlined />}
                    aria-label={t("断开连接")}
                    disabled={
                      !activeSession ||
                      activeSession.status === "disconnected" ||
                      activeSession.status === "error"
                    }
                    onClick={disconnectActive}
                  />
                </Tooltip>
              </div>
            </div>
            <div className="vnc-viewer-area">
              {gatewayDown || targets.isError ? (
                <div className="rdp-viewer-banners">
                  {gatewayDown && gateway.data ? (
                    <Alert
                      type="warning"
                      showIcon
                      message={gateway.data.message}
                      description={gatewayHint(gateway.data)}
                      action={
                        <Space direction="vertical" size={6}>
                          {canInstallGateway ? (
                            <Button
                              type="primary"
                              size="small"
                              onClick={() => navigate(INSTALL_GATEWAY_PATH)}
                            >
                              {t("一键安装")}
                            </Button>
                          ) : null}
                          <Button
                            size="small"
                            loading={gateway.isFetching}
                            onClick={() => void gateway.refetch()}
                          >
                            {t("重新检测")}
                          </Button>
                        </Space>
                      }
                    />
                  ) : null}
                  {targets.isError ? (
                    <Alert
                      type="error"
                      showIcon
                      message={t("读取 RDP 目标失败，请重试")}
                    />
                  ) : null}
                </div>
              ) : null}
              {sessions.length > 0 ? (
                sessions.map((session) => (
                  <Desktop
                    key={session.id}
                    session={session}
                    active={visible && active === session.id}
                    trackpad={trackpad}
                    rightClickArmed={rightClickArmed}
                    showLocalPointer={showLocalPointer}
                    onStatus={handleStatus}
                    onClient={handleClient}
                    onClipboard={handleClipboard}
                    onReconnect={reconnect}
                    onRightClickUsed={handleRightClickUsed}
                    onViewport={handleViewport}
                    onZoomedChange={handleZoomedChange}
                  />
                ))
              ) : (
                <div className="vnc-viewer-stage">
                  <div className="vnc-viewer-host" />
                  <div className="vnc-viewer-state">
                    <DesktopOutlined />
                    <strong>{t("RDP 尚未连接")}</strong>
                    <span>
                      {permissionBlocked ??
                        (targets.data?.length
                          ? t(
                              "选择一台已启用远程桌面的电脑，登录后即可在浏览器中操作桌面。",
                            )
                          : t(
                              "先添加一台已启用远程桌面的 Windows 电脑，登录后即可操作桌面。",
                            ))}
                    </span>
                    {targets.data?.length ? (
                      <PermissionButton
                        permission="system:rdp:connect"
                        type="primary"
                        icon={<LinkOutlined />}
                        className="vnc-viewer-connect"
                        disabled={!!connectionBlocked}
                        onClick={() => openLogin(selectedTarget)}
                      >
                        {t("连接")}
                      </PermissionButton>
                    ) : (
                      <PermissionButton
                        permission="system:rdp:update"
                        type="primary"
                        icon={<PlusOutlined />}
                        className="vnc-viewer-connect"
                        onClick={() => setManageOpen(true)}
                      >
                        {t("添加电脑")}
                      </PermissionButton>
                    )}
                  </div>
                </div>
              )}
            </div>
            {activeConnected ? (
              <div
                className={`vnc-quickbar-shell${showQuickBar ? "" : " is-collapsed"}`}
              >
                <VncQuickBar
                  label={t("RDP 快捷操作")}
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
                  onPasteToRemote={pasteToRemote}
                  onOpenClipboard={() => setClipboardOpen(true)}
                  onOpenShortcuts={() => setShortcutsOpen(true)}
                />
              </div>
            ) : null}
            {activeSession ? (
              <div className="vnc-statusbar">
                <span className="vnc-statusbar-item">
                  <DesktopOutlined /> {activeSession.target.name} ·{" "}
                  {activeSession.target.host}:{activeSession.target.port}
                </span>
                <span className="vnc-statusbar-item">
                  <span
                    className={`vnc-status-dot is-${activeSession.status}`}
                  />
                  {statusLabel(activeSession.status)}
                </span>
                {gateway.data?.runtime ? (
                  <span className="vnc-statusbar-item">
                    {t("网关：{0}", gateway.data.runtime)}
                  </span>
                ) : null}
                {activeSession.error ? (
                  <span className="vnc-statusbar-item is-error">
                    {activeSession.error}
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
        aria-label={t("RDP 远程键盘输入")}
        onBeforeInput={handleMobileKeyboardInput}
        onKeyDown={handleMobileKeyboardKeyDown}
        onChange={(event) => {
          event.currentTarget.value = "";
        }}
      />
      <VncShortcutsDrawer
        open={shortcutsOpen}
        connected={activeConnected}
        trackpad={trackpad}
        showLocalPointer={showLocalPointer}
        onClose={() => setShortcutsOpen(false)}
        onStrokes={sendStrokes}
        onToggleTrackpad={toggleTrackpad}
        onToggleLocalPointer={toggleLocalPointer}
        onOpenClipboard={() => setClipboardOpen(true)}
      />
      <RemoteClipboardDrawer
        open={clipboardOpen}
        connected={activeConnected}
        remoteClipboard={
          activeSession ? (remoteClipboards[activeSession.id] ?? "") : ""
        }
        onClose={() => setClipboardOpen(false)}
        onStrokes={sendStrokes}
        onPasteToRemote={pasteToRemote}
        onTypeText={typeText}
      />
      <Drawer
        title={t("RDP 控制")}
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
            placeholder={t("选择远程电脑")}
            value={selected}
            options={targetOptions}
            loading={targets.isLoading}
            onChange={setSelected}
          />
          <div className="vnc-mobile-control-grid">
            <PermissionButton
              permission="system:rdp:connect"
              type="primary"
              icon={<PlusOutlined />}
              disabled={!!connectionBlocked}
              onClick={() => openLogin(selectedTarget)}
            >
              {t("新建连接")}
            </PermissionButton>
            <PermissionButton
              permission="system:rdp:connect"
              icon={<LinkOutlined />}
              disabled={!activeSession || !selectedTarget?.enabled}
              onClick={() => openLogin(selectedTarget, activeSession?.id)}
            >
              {t("重新连接")}
            </PermissionButton>
            <Button
              icon={<LockOutlined />}
              disabled={!activeConnected}
              onClick={() => {
                sendStrokes(KEY_CTRL_ALT_DEL);
                setMobileControlsOpen(false);
              }}
            >
              Ctrl+Alt+Del
            </Button>
            <Button
              icon={<WindowsOutlined />}
              disabled={!activeConnected}
              onClick={() => {
                sendStrokes(KEY_WINDOWS);
                setMobileControlsOpen(false);
              }}
            >
              {t("Windows 键")}
            </Button>
            <Button
              icon={<SnippetsOutlined />}
              disabled={!activeConnected}
              onClick={() => {
                setMobileControlsOpen(false);
                setClipboardOpen(true);
              }}
            >
              {t("剪贴板")}
            </Button>
            <Button
              icon={activeZoomed ? <CompressOutlined /> : <OneToOneOutlined />}
              disabled={!activeConnected}
              onClick={() => {
                toggleActualSize();
                setMobileControlsOpen(false);
              }}
            >
              {activeZoomed ? t("适应窗口") : t("原始大小 1:1")}
            </Button>
            {canEdit ? (
              <Button
                icon={<SettingOutlined />}
                onClick={() => {
                  setMobileControlsOpen(false);
                  setManageOpen(true);
                }}
              >
                {t("管理电脑")}
              </Button>
            ) : null}
            <Button icon={<ReloadOutlined />} onClick={refresh}>
              {t("刷新")}
            </Button>
            <Button
              danger
              icon={<DisconnectOutlined />}
              disabled={!activeSession || activeSession.status !== "connected"}
              onClick={() => {
                disconnectActive();
                setMobileControlsOpen(false);
              }}
            >
              {t("断开连接")}
            </Button>
            <Button
              icon={<CloseOutlined />}
              disabled={!activeSession}
              onClick={() => {
                if (activeSession) closeTab(activeSession.id);
                setMobileControlsOpen(false);
              }}
            >
              {t("关闭标签")}
            </Button>
          </div>
        </Space>
      </Drawer>
      <Modal
        title={t("登录远程电脑")}
        open={loginOpen}
        zIndex={1700}
        confirmLoading={busy}
        okText={useSavedCredentials ? t("使用已保存凭据连接") : t("连接")}
        okButtonProps={{
          disabled: credentials.isFetching || credentials.isError,
        }}
        onOk={connect}
        onCancel={() => {
          setLoginOpen(false);
          setReconnectId(undefined);
          loginForm.resetFields();
        }}
      >
        {credentials.isFetching && (
          <Alert type="info" message={t("正在检查已保存凭据…")} />
        )}
        {credentials.isError && (
          <Alert
            type="error"
            message={t("无法读取凭据状态，请确认后端已升级并运行数据库迁移")}
            action={
              <Button onClick={() => credentials.refetch()}>{t("重试")}</Button>
            }
          />
        )}
        {credentials.data?.saved && (
          <Space
            direction="vertical"
            style={{ width: "100%", marginBottom: 16 }}
          >
            <Alert
              type="success"
              showIcon
              message={t("已保存此电脑的登录凭据，无需再次输入密码。")}
            />
            <Checkbox
              checked={useSavedCredentials}
              onChange={(event) => setManualLogin(!event.target.checked)}
            >
              {t("使用已保存的账号和密码")}
            </Checkbox>
            <Popconfirm
              title={t("清除当前账号为此电脑保存的凭据？")}
              zIndex={1800}
              onConfirm={async () => {
                try {
                  await deleteRdpCredentials(selected as string);
                  cache.setQueryData(["rdp-credentials", selected], {
                    saved: false,
                  });
                  setManualLogin(true);
                  loginForm.resetFields();
                  message.success(t("已清除保存的凭据"));
                } catch {
                  message.error(t("清除失败，请重试"));
                }
              }}
            >
              <Button danger>{t("清除已保存凭据")}</Button>
            </Popconfirm>
          </Space>
        )}
        <Form
          form={loginForm}
          layout="vertical"
          autoComplete="off"
          hidden={useSavedCredentials}
        >
          <Form.Item
            name="username"
            label={t("Windows 用户名")}
            rules={[{ required: true }]}
          >
            <Input
              placeholder={t("用户名或 MicrosoftAccount\\邮箱")}
              autoComplete="off"
            />
          </Form.Item>
          <Form.Item
            name="password"
            label={t("Windows 密码（不是 PIN）")}
            rules={[{ required: true }]}
          >
            <Input.Password autoComplete="new-password" />
          </Form.Item>
          <Form.Item name="domain" label={t("域（可选）")}>
            <Input />
          </Form.Item>
          <Checkbox
            checked={rememberCredentials}
            onChange={(event) => setRememberCredentials(event.target.checked)}
          >
            {t("记住此电脑的账号和密码（加密保存，仅当前后台账号可用）")}
          </Checkbox>
        </Form>
        <div className="rdp-resolution-field">
          <Typography.Text strong>{t("远程分辨率")}</Typography.Text>
          <Select
            className="full-width"
            value={resolution}
            aria-label={t("远程分辨率")}
            onChange={changeResolution}
            options={RESOLUTIONS.map((value) => ({
              value,
              label:
                value === "auto"
                  ? t("自动（与当前窗口一样大）")
                  : value.replace("x", " × "),
            }))}
          />
          <Typography.Text type="secondary">
            {t(
              "远程桌面按这个分辨率显示，画面先完整显示在窗口里，可双指张合或 Ctrl + 滚轮缩放、拖动查看细节。",
            )}
          </Typography.Text>
        </div>
      </Modal>
      <Modal
        title={t("管理远程电脑")}
        open={manageOpen}
        zIndex={1700}
        width={850}
        footer={null}
        onCancel={() => setManageOpen(false)}
      >
        {permissionBlocked && (
          <Alert
            type="warning"
            showIcon
            message={t(
              "当前账号只有管理权限，没有 RDP 连接权限。请管理员分配 system:rdp:connect 权限后重新登录。",
            )}
          />
        )}
        <Button
          icon={<PlusOutlined />}
          onClick={() => {
            setEditing(null);
            targetForm.resetFields();
            targetForm.setFieldsValue({
              port: 3389,
              enabled: true,
              ignore_certificate: false,
            });
          }}
        >
          {t("添加电脑")}
        </Button>
        <Table
          rowKey="key"
          dataSource={targets.data}
          pagination={false}
          scroll={{ x: 650 }}
          columns={[
            { title: t("名称"), dataIndex: "name" },
            { title: t("主机"), dataIndex: "host" },
            { title: t("端口"), dataIndex: "port" },
            {
              title: t("状态"),
              render: (_, target: RdpTarget) =>
                target.enabled ? t("启用") : t("停用"),
            },
            {
              title: t("操作"),
              render: (_, target: RdpTarget) => (
                <Space>
                  <Button
                    type="primary"
                    disabled={
                      !target.enabled ||
                      !canConnect ||
                      sessions.length >= MAX_SESSIONS
                    }
                    onClick={() => openLogin(target)}
                  >
                    {t("连接")}
                  </Button>
                  <Button
                    onClick={() => {
                      setEditing(target);
                      targetForm.setFieldsValue(target);
                    }}
                  >
                    {t("编辑")}
                  </Button>
                  <Popconfirm
                    title={t("删除此电脑？已连接桌面不会被关闭。")}
                    zIndex={1800}
                    onConfirm={() =>
                      save(
                        (targets.data ?? []).filter(
                          (item) => item.key !== target.key,
                        ),
                      )
                    }
                  >
                    <Button danger disabled={busy}>
                      {t("删除")}
                    </Button>
                  </Popconfirm>
                </Space>
              ),
            },
          ]}
        />
      </Modal>
      <Modal
        title={editing ? t("编辑电脑") : t("添加电脑")}
        open={editing !== undefined}
        zIndex={1750}
        confirmLoading={busy}
        onCancel={() => setEditing(undefined)}
        onOk={async () => {
          const value = await targetForm.validateFields();
          const key = editing?.key ?? createClientId("pc");
          await save(
            [
              ...(targets.data ?? []).filter(
                (item) => item.key !== editing?.key,
              ),
              { ...value, key },
            ],
            key,
          );
        }}
      >
        <Form form={targetForm} layout="vertical">
          <Form.Item
            name="name"
            label={t("名称")}
            rules={[{ required: true, whitespace: true, max: 200 }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="host"
            label={t("主机 IP / 域名（从网关访问）")}
            extra={t(
              "填 localhost 表示后台所在的这台电脑；网关在 WSL 中运行时会自动换成 Windows 主机地址。",
            )}
            rules={[{ required: true, whitespace: true, max: 255 }]}
          >
            <Input placeholder="192.168.1.10" />
          </Form.Item>
          <Form.Item name="port" label={t("端口")} rules={[{ required: true }]}>
            <InputNumber min={1} max={65535} precision={0} />
          </Form.Item>
          <Form.Item name="enabled" label={t("启用")} valuePropName="checked">
            <Switch />
          </Form.Item>
          <Form.Item
            name="ignore_certificate"
            label={t("允许自签名 / 不受信任证书")}
            valuePropName="checked"
            extra={t("仅在确认目标电脑可信时开启。")}
          >
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
