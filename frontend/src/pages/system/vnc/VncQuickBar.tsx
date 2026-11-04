import {
  AimOutlined,
  AppstoreOutlined,
  CopyOutlined,
  FontSizeOutlined,
  SendOutlined,
  SnippetsOutlined,
} from "@ant-design/icons";
import {
  Button,
  Drawer,
  Input,
  Space,
  Switch,
  Tooltip,
  Typography,
  message,
} from "antd";
import { forwardRef, useState } from "react";
import { t } from "../../../i18n";
import {
  KEYS,
  type KeyStroke,
  MODIFIER_LABELS,
  MODIFIER_ORDER,
  type ModifierName,
  SHORTCUT_GROUP_LABELS,
  VNC_SHORTCUTS,
  type VncShortcutGroup,
  letterKey,
} from "./vncInput";

/** Pressed on the remote computer once its clipboard holds the text. */
const PASTE_KEYS: KeyStroke[] = [KEYS.ctrl, letterKey("v")];
/** Gives the server time to apply the new clipboard before Ctrl+V. */
const PASTE_DELAY_MS = 200;

/** Puts `text` on the remote clipboard and presses Ctrl+V there. */
function pasteIntoRemote(
  text: string,
  onPasteToRemote: (text: string) => boolean,
  onStrokes: (strokes: KeyStroke[]) => void,
) {
  if (!onPasteToRemote(text)) {
    return false;
  }
  window.setTimeout(() => onStrokes(PASTE_KEYS), PASTE_DELAY_MS);
  return true;
}

/**
 * Text on this device's clipboard, or null when the browser refuses (no
 * permission, or plain HTTP where `navigator.clipboard` is missing). iOS asks
 * with its own "Paste" bubble, so call this straight from a tap.
 */
async function readLocalClipboard() {
  try {
    return (await navigator.clipboard?.readText()) ?? null;
  } catch {
    return null;
  }
}

type QuickBarProps = {
  connected: boolean;
  trackpad: boolean;
  rightClickArmed: boolean;
  latched: readonly ModifierName[];
  onOpenKeyboard: () => void;
  onToggleTrackpad: () => void;
  onToggleRightClick: () => void;
  onToggleModifier: (name: ModifierName) => void;
  onStrokes: (strokes: KeyStroke[]) => void;
  onPasteToRemote: (text: string) => boolean;
  onOpenClipboard: () => void;
  onOpenShortcuts: () => void;
  /** Toolbar name for screen readers; the RDP page passes its own. */
  label?: string;
};

function RightClickIcon() {
  return (
    <svg
      viewBox="0 0 16 20"
      width="1em"
      height="1em"
      aria-hidden="true"
      focusable="false"
    >
      <rect
        x="1.5"
        y="1.5"
        width="13"
        height="17"
        rx="6.5"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      />
      <path d="M8 1.5v7h6.5V8A6.5 6.5 0 0 0 8 1.5Z" fill="currentColor" />
    </svg>
  );
}

/** The arrow drawn over the remote screen in trackpad mode. */
export const TrackpadCursor = forwardRef<HTMLSpanElement>(
  function TrackpadCursor(_, ref) {
    return (
      <span ref={ref} className="vnc-trackpad-cursor" aria-hidden="true">
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
    );
  },
);

/** Bottom bar with the remote-control helpers phones lack a keyboard for. */
export function VncQuickBar({
  connected,
  trackpad,
  rightClickArmed,
  latched,
  onOpenKeyboard,
  onToggleTrackpad,
  onToggleRightClick,
  onToggleModifier,
  onStrokes,
  onPasteToRemote,
  onOpenClipboard,
  onOpenShortcuts,
  label = t("VNC 快捷操作"),
}: QuickBarProps) {
  const paste = async () => {
    const text = await readLocalClipboard();
    if (text && pasteIntoRemote(text, onPasteToRemote, onStrokes)) {
      message.success(t("已粘贴到远程电脑"));
    } else {
      onOpenClipboard();
    }
  };

  return (
    <div
      className="vnc-quickbar"
      role="toolbar"
      aria-label={label}
      // Keep focus (and the phone keyboard) where it is while tapping keys.
      onMouseDown={(event) => event.preventDefault()}
    >
      <Tooltip title={t("打开键盘")}>
        <Button
          type="text"
          className="vnc-quickbar-keyboard"
          icon={<FontSizeOutlined />}
          aria-label={t("打开键盘")}
          disabled={!connected}
          onClick={onOpenKeyboard}
        />
      </Tooltip>
      <Tooltip title={t("下一次点击发送右键")}>
        <Button
          type="text"
          className={rightClickArmed ? "is-active" : undefined}
          icon={<RightClickIcon />}
          aria-label={t("右键")}
          aria-pressed={rightClickArmed}
          disabled={!connected}
          onClick={onToggleRightClick}
        >
          <span className="vnc-quickbar-label">{t("右键")}</span>
        </Button>
      </Tooltip>
      <Tooltip
        title={
          trackpad
            ? t("触控板模式：拖动移动光标，轻点单击")
            : t("触摸模式：点哪里就点击哪里")
        }
      >
        <Button
          type="text"
          className={trackpad ? "is-active" : undefined}
          icon={<AimOutlined />}
          aria-label={t("触控板模式")}
          aria-pressed={trackpad}
          onClick={onToggleTrackpad}
        >
          <span className="vnc-quickbar-label">
            {trackpad ? t("触控板") : t("触摸")}
          </span>
        </Button>
      </Tooltip>
      <span className="vnc-quickbar-divider" aria-hidden="true" />
      {MODIFIER_ORDER.map((name) => {
        const active = latched.includes(name);
        return (
          <Tooltip
            key={name}
            title={t("按住 {0}，与下一个按键组合", MODIFIER_LABELS[name])}
          >
            <Button
              type="text"
              className={`vnc-quickbar-key${active ? " is-active" : ""}`}
              aria-pressed={active}
              disabled={!connected}
              onClick={() => onToggleModifier(name)}
            >
              {MODIFIER_LABELS[name]}
            </Button>
          </Tooltip>
        );
      })}
      <span className="vnc-quickbar-divider" aria-hidden="true" />
      <Button
        type="text"
        className="vnc-quickbar-key"
        disabled={!connected}
        onClick={() => onStrokes([KEYS.escape])}
      >
        Esc
      </Button>
      <Button
        type="text"
        className="vnc-quickbar-key"
        disabled={!connected}
        onClick={() => onStrokes([KEYS.tab])}
      >
        Tab
      </Button>
      <Button
        type="text"
        className="vnc-quickbar-key is-wide"
        disabled={!connected}
        onClick={() => onStrokes([KEYS.ctrl, KEYS.alt, KEYS.delete])}
      >
        Ctrl+Alt+Del
      </Button>
      <Tooltip title={t("把本机复制的文字粘贴到远程电脑")}>
        <Button
          type="text"
          icon={<SnippetsOutlined />}
          aria-label={t("粘贴到远程")}
          disabled={!connected}
          onClick={() => void paste()}
        >
          <span className="vnc-quickbar-label">{t("粘贴")}</span>
        </Button>
      </Tooltip>
      <Tooltip title={t("更多快捷键")}>
        <Button
          type="text"
          icon={<AppstoreOutlined />}
          aria-label={t("更多快捷键")}
          onClick={onOpenShortcuts}
        >
          <span className="vnc-quickbar-label">{t("更多")}</span>
        </Button>
      </Tooltip>
    </div>
  );
}

type ShortcutsDrawerProps = {
  open: boolean;
  connected: boolean;
  trackpad: boolean;
  showLocalPointer: boolean;
  onClose: () => void;
  onStrokes: (strokes: KeyStroke[]) => void;
  onToggleTrackpad: () => void;
  onToggleLocalPointer: () => void;
  onOpenClipboard: () => void;
};

const GROUP_ORDER: VncShortcutGroup[] = [
  "system",
  "edit",
  "navigation",
  "function",
];

export function VncShortcutsDrawer({
  open,
  connected,
  trackpad,
  showLocalPointer,
  onClose,
  onStrokes,
  onToggleTrackpad,
  onToggleLocalPointer,
  onOpenClipboard,
}: ShortcutsDrawerProps) {
  return (
    <Drawer
      title={t("快捷操作")}
      open={open}
      placement="bottom"
      height="auto"
      zIndex={1600}
      rootClassName="vnc-mobile-controls-drawer-root"
      className="vnc-shortcuts-drawer"
      onClose={onClose}
    >
      <div className="vnc-shortcut-settings">
        <div className="vnc-shortcut-setting">
          <span>
            <strong>{t("触控板模式")}</strong>
            <Typography.Text type="secondary">
              {t(
                "拖动移动光标，轻点单击，双指轻点右键，双指滑动滚动，长按拖拽",
              )}
            </Typography.Text>
          </span>
          <Switch
            checked={trackpad}
            aria-label={t("触控板模式")}
            onChange={onToggleTrackpad}
          />
        </div>
        <div className="vnc-shortcut-setting">
          <span>
            <strong>{t("显示本地指针光圈")}</strong>
            <Typography.Text type="secondary">
              {t("在鼠标或手指位置显示跟随的光圈")}
            </Typography.Text>
          </span>
          <Switch
            checked={showLocalPointer}
            aria-label={t("显示本地指针光圈")}
            onChange={onToggleLocalPointer}
          />
        </div>
      </div>
      {GROUP_ORDER.map((group) => (
        <section key={group} className="vnc-shortcut-group">
          <h4>{SHORTCUT_GROUP_LABELS[group]}</h4>
          <div className={`vnc-shortcut-grid is-${group}`}>
            {VNC_SHORTCUTS.filter((shortcut) => shortcut.group === group).map(
              (shortcut) => (
                <Button
                  key={shortcut.id}
                  disabled={!connected}
                  title={shortcut.description || shortcut.label}
                  onClick={() => onStrokes(shortcut.strokes)}
                >
                  <span className="vnc-shortcut-label">{shortcut.label}</span>
                  {shortcut.description ? (
                    <span className="vnc-shortcut-description">
                      {shortcut.description}
                    </span>
                  ) : null}
                </Button>
              ),
            )}
          </div>
        </section>
      ))}
      <section className="vnc-shortcut-group">
        <h4>{t("剪贴板")}</h4>
        <Button
          block
          icon={<SnippetsOutlined />}
          onClick={() => {
            onClose();
            onOpenClipboard();
          }}
        >
          {t("粘贴到远程 / 复制远程内容 / 发送文本")}
        </Button>
      </section>
    </Drawer>
  );
}

type ClipboardDrawerProps = {
  open: boolean;
  connected: boolean;
  /** Last text the remote computer copied. */
  remoteClipboard: string;
  onClose: () => void;
  onStrokes: (strokes: KeyStroke[]) => void;
  onPasteToRemote: (text: string) => boolean;
  onTypeText: (text: string) => boolean;
};

/** Moves text between this device's clipboard and the remote computer's. */
export function RemoteClipboardDrawer({
  open,
  connected,
  remoteClipboard,
  onClose,
  onStrokes,
  onPasteToRemote,
  onTypeText,
}: ClipboardDrawerProps) {
  const [text, setText] = useState("");

  const readClipboard = async () => {
    const local = await readLocalClipboard();
    if (local === null) {
      message.warning(t("浏览器未允许读取剪贴板，请长按输入框选择“粘贴”"));
    } else {
      setText(local);
    }
  };

  const copyRemoteClipboard = async () => {
    try {
      await navigator.clipboard.writeText(remoteClipboard);
      message.success(t("已复制远程剪贴板内容"));
    } catch {
      message.warning(t("浏览器未允许写入剪贴板"));
    }
  };

  const finish = () => {
    setText("");
    onClose();
  };

  return (
    <Drawer
      title={t("剪贴板")}
      open={open}
      placement="bottom"
      height="auto"
      zIndex={1600}
      rootClassName="vnc-mobile-controls-drawer-root"
      className="vnc-shortcuts-drawer"
      onClose={onClose}
    >
      <section className="vnc-shortcut-group">
        <h4>{t("发送到远程电脑")}</h4>
        <Input.TextArea
          value={text}
          autoSize={{ minRows: 3, maxRows: 8 }}
          placeholder={t(
            "粘贴或输入要发送到远程电脑的文字（长按此处选择“粘贴”）",
          )}
          onChange={(event) => setText(event.target.value)}
        />
        <Space wrap className="vnc-shortcut-text-actions">
          <Button
            icon={<SnippetsOutlined />}
            onClick={() => void readClipboard()}
          >
            {t("读取本机剪贴板")}
          </Button>
          <Button
            type="primary"
            icon={<SendOutlined />}
            disabled={!connected || !text}
            onClick={() => {
              if (pasteIntoRemote(text, onPasteToRemote, onStrokes)) {
                message.success(t("已粘贴到远程电脑"));
                finish();
              }
            }}
          >
            {t("粘贴到远程")}
          </Button>
          <Button
            icon={<CopyOutlined />}
            disabled={!connected || !text}
            onClick={() => {
              if (onPasteToRemote(text)) {
                message.success(t("已写入远程剪贴板，可在远程按 Ctrl+V 粘贴"));
              }
            }}
          >
            {t("仅写入远程剪贴板")}
          </Button>
          <Tooltip
            title={t("逐字输入，适合登录界面等不同步剪贴板的地方；只支持文字")}
          >
            <Button
              icon={<FontSizeOutlined />}
              disabled={!connected || !text}
              onClick={() => {
                if (onTypeText(text)) {
                  finish();
                }
              }}
            >
              {t("模拟键入")}
            </Button>
          </Tooltip>
        </Space>
      </section>
      <section className="vnc-shortcut-group">
        <h4>{t("远程剪贴板")}</h4>
        {remoteClipboard ? (
          <div className="vnc-remote-clipboard">
            <Typography.Paragraph
              ellipsis={{ rows: 3 }}
              className="vnc-remote-clipboard-text"
            >
              {remoteClipboard}
            </Typography.Paragraph>
            <Button
              size="small"
              icon={<CopyOutlined />}
              onClick={() => void copyRemoteClipboard()}
            >
              {t("复制到本机")}
            </Button>
          </div>
        ) : (
          <Typography.Text type="secondary">
            {t("在远程电脑上复制文字后，会显示在这里")}
          </Typography.Text>
        )}
      </section>
    </Drawer>
  );
}
