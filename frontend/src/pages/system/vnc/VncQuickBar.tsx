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
import { useState } from "react";
import {
  KEYS,
  type KeyStroke,
  MODIFIER_LABELS,
  MODIFIER_ORDER,
  type ModifierName,
  SHORTCUT_GROUP_LABELS,
  VNC_SHORTCUTS,
  type VncShortcutGroup,
} from "./vncInput";

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
  onOpenShortcuts: () => void;
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
  onOpenShortcuts,
}: QuickBarProps) {
  return (
    <div
      className="vnc-quickbar"
      role="toolbar"
      aria-label="VNC 快捷操作"
      // Keep focus (and the phone keyboard) where it is while tapping keys.
      onMouseDown={(event) => event.preventDefault()}
    >
      <Tooltip title="打开键盘">
        <Button
          type="text"
          className="vnc-quickbar-keyboard"
          icon={<FontSizeOutlined />}
          aria-label="打开键盘"
          disabled={!connected}
          onClick={onOpenKeyboard}
        />
      </Tooltip>
      <Tooltip title="下一次点击发送右键">
        <Button
          type="text"
          className={rightClickArmed ? "is-active" : undefined}
          icon={<RightClickIcon />}
          aria-label="右键"
          aria-pressed={rightClickArmed}
          disabled={!connected}
          onClick={onToggleRightClick}
        >
          <span className="vnc-quickbar-label">右键</span>
        </Button>
      </Tooltip>
      <Tooltip
        title={
          trackpad
            ? "触控板模式：拖动移动光标，轻点单击"
            : "触摸模式：点哪里就点击哪里"
        }
      >
        <Button
          type="text"
          className={trackpad ? "is-active" : undefined}
          icon={<AimOutlined />}
          aria-label="触控板模式"
          aria-pressed={trackpad}
          onClick={onToggleTrackpad}
        >
          <span className="vnc-quickbar-label">
            {trackpad ? "触控板" : "触摸"}
          </span>
        </Button>
      </Tooltip>
      <span className="vnc-quickbar-divider" aria-hidden="true" />
      {MODIFIER_ORDER.map((name) => {
        const active = latched.includes(name);
        return (
          <Tooltip
            key={name}
            title={`按住 ${MODIFIER_LABELS[name]}，与下一个按键组合`}
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
      <Tooltip title="更多快捷键与文本发送">
        <Button
          type="text"
          icon={<AppstoreOutlined />}
          aria-label="更多快捷键"
          onClick={onOpenShortcuts}
        >
          <span className="vnc-quickbar-label">更多</span>
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
  remoteClipboard: string;
  onClose: () => void;
  onStrokes: (strokes: KeyStroke[]) => void;
  onToggleTrackpad: () => void;
  onToggleLocalPointer: () => void;
  onPasteToRemote: (text: string) => boolean;
  onTypeText: (text: string) => boolean;
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
  remoteClipboard,
  onClose,
  onStrokes,
  onToggleTrackpad,
  onToggleLocalPointer,
  onPasteToRemote,
  onTypeText,
}: ShortcutsDrawerProps) {
  const [text, setText] = useState("");

  const readLocalClipboard = async () => {
    try {
      setText(await navigator.clipboard.readText());
    } catch {
      message.warning("浏览器未允许读取剪贴板，请直接粘贴到输入框");
    }
  };

  const copyRemoteClipboard = async () => {
    try {
      await navigator.clipboard.writeText(remoteClipboard);
      message.success("已复制远程剪贴板内容");
    } catch {
      message.warning("浏览器未允许写入剪贴板");
    }
  };

  return (
    <Drawer
      title="快捷操作"
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
            <strong>触控板模式</strong>
            <Typography.Text type="secondary">
              拖动移动光标，轻点单击，双指轻点右键，双指滑动滚动，长按拖拽
            </Typography.Text>
          </span>
          <Switch
            checked={trackpad}
            aria-label="触控板模式"
            onChange={onToggleTrackpad}
          />
        </div>
        <div className="vnc-shortcut-setting">
          <span>
            <strong>显示本地指针光圈</strong>
            <Typography.Text type="secondary">
              在鼠标或手指位置显示跟随的光圈
            </Typography.Text>
          </span>
          <Switch
            checked={showLocalPointer}
            aria-label="显示本地指针光圈"
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
        <h4>发送文本</h4>
        <Input.TextArea
          value={text}
          autoSize={{ minRows: 2, maxRows: 6 }}
          placeholder="输入或粘贴要发送到远程电脑的文字"
          onChange={(event) => setText(event.target.value)}
        />
        <Space wrap className="vnc-shortcut-text-actions">
          <Button
            icon={<SnippetsOutlined />}
            onClick={() => void readLocalClipboard()}
          >
            读取本机剪贴板
          </Button>
          <Button
            icon={<CopyOutlined />}
            disabled={!connected || !text}
            onClick={() => {
              if (onPasteToRemote(text)) {
                message.success("已写入远程剪贴板，可在远程按 Ctrl+V 粘贴");
              }
            }}
          >
            写入远程剪贴板
          </Button>
          <Button
            type="primary"
            icon={<SendOutlined />}
            disabled={!connected || !text}
            onClick={() => {
              if (onTypeText(text)) {
                setText("");
                onClose();
              }
            }}
          >
            模拟键入
          </Button>
        </Space>
        {remoteClipboard ? (
          <div className="vnc-remote-clipboard">
            <Typography.Text type="secondary">远程剪贴板</Typography.Text>
            <Typography.Paragraph
              ellipsis={{ rows: 2 }}
              className="vnc-remote-clipboard-text"
            >
              {remoteClipboard}
            </Typography.Paragraph>
            <Button
              size="small"
              icon={<CopyOutlined />}
              onClick={() => void copyRemoteClipboard()}
            >
              复制到本机
            </Button>
          </div>
        ) : null}
      </section>
    </Drawer>
  );
}
