import {
  ArrowLeftOutlined,
  CommentOutlined,
  EditOutlined,
  FolderOpenOutlined,
  MessageOutlined,
  MoreOutlined,
  PictureOutlined,
  PlusOutlined,
  SearchOutlined,
  SendOutlined,
  SmileOutlined,
  TeamOutlined,
  UserAddOutlined,
} from "@ant-design/icons";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Badge,
  Button,
  Drawer,
  Dropdown,
  Empty,
  Input,
  Popconfirm,
  Popover,
  Progress,
  Space,
  Spin,
  Tag,
  Tooltip,
  Typography,
  message as toast,
} from "antd";
import type { TextAreaRef } from "antd/es/input/TextArea";
import {
  type ClipboardEvent,
  type DragEvent,
  Fragment,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useSearchParams } from "react-router-dom";
import {
  IM_CONVERSATIONS_KEY,
  IM_EVENT,
  type ImConversation,
  type ImConversationDetail,
  type ImEvent,
  type ImMessage,
  createImGroup,
  dissolveImGroup,
  fetchImConversation,
  fetchImConversations,
  fetchImMessages,
  inviteImMembers,
  markImRead,
  openImDirect,
  removeImMember,
  renameImGroup,
  sendImFile,
  sendImText,
} from "../../../api/admin/im";
import { t } from "../../../i18n";
import { createClientId } from "../../../utils/clientId";
import { ContactPicker, ImAvatar, MessageBody } from "./ImParts";
import { dividerTime, listTime, messagePreview, systemText } from "./imFormat";
import "./im.css";

const PAGE_SIZE = 50;
const EMOJI =
  "😀 😁 😂 🤣 😊 😍 😘 😎 🤔 😅 😭 😡 😴 🙄 😮 🥳 👍 👎 👏 🙏 💪 👌 🤝 🎉 ❤️ 💔 🔥 ✨ 🌹 ☕ 🍺 🎂 ✅ ❌ ⭐ 📌 📎 ⏰ 💡 🚀".split(
    " ",
  );

function errorText(error: unknown, fallback: string) {
  return error instanceof Error && error.message ? error.message : fallback;
}

function mergeMessages(old: ImMessage[], incoming: ImMessage[]) {
  if (incoming.length === 0) return old;
  const byId = new Map(old.map((item) => [item.id, item]));
  for (const item of incoming) byId.set(item.id, item);
  return [...byId.values()].sort((a, b) => a.id - b.id);
}

type Upload = { key: string; name: string; percent: number };

type ChatPaneProps = {
  conversation: ImConversation;
  detail?: ImConversationDetail;
  onBack: () => void;
  onOpenSettings: () => void;
  onActivity: () => void;
};

function ChatPane({
  conversation,
  detail,
  onBack,
  onOpenSettings,
  onActivity,
}: ChatPaneProps) {
  const { id, viewer_id: viewerId } = conversation;
  const isGroup = conversation.kind === "group";
  const queryClient = useQueryClient();
  const [messages, setMessages] = useState<ImMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [uploads, setUploads] = useState<Upload[]>([]);
  const [dragging, setDragging] = useState(false);
  const listRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<TextAreaRef>(null);
  const imageInputRef = useRef<HTMLInputElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const stickToBottom = useRef(true);
  const keepOffset = useRef<number | null>(null);
  const lastIdRef = useRef<number>();
  lastIdRef.current = messages.at(-1)?.id;

  const merge = useCallback((incoming: ImMessage[]) => {
    setMessages((old) => mergeMessages(old, incoming));
  }, []);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    fetchImMessages(id, { limit: PAGE_SIZE })
      .then((rows) => {
        if (!alive) return;
        stickToBottom.current = true;
        setMessages(rows);
        setHasMore(rows.length === PAGE_SIZE);
      })
      .catch(() => undefined)
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [id]);

  const catchUp = useCallback(() => {
    const after = lastIdRef.current;
    void fetchImMessages(id, after ? { after_id: after, limit: 200 } : {})
      .then(merge)
      .catch(() => undefined);
  }, [id, merge]);

  // New messages arrive over the event stream; after a reconnect or when the
  // tab comes back, fetch whatever was missed.
  useEffect(() => {
    const handle = (event: Event) => {
      const data = (event as CustomEvent<ImEvent>).detail;
      if (data.conversation_id !== id) return;
      if (data.type === "message") merge([data.message]);
      else catchUp();
    };
    const handleVisible = () => {
      if (!document.hidden) catchUp();
    };
    window.addEventListener(IM_EVENT, handle);
    document.addEventListener("visibilitychange", handleVisible);
    return () => {
      window.removeEventListener(IM_EVENT, handle);
      document.removeEventListener("visibilitychange", handleVisible);
    };
  }, [catchUp, id, merge]);

  // Reading the open chat clears its unread badge.
  const lastId = messages.at(-1)?.id;
  const [visibleTick, setVisibleTick] = useState(0);
  useEffect(() => {
    const handle = () => setVisibleTick((tick) => tick + 1);
    document.addEventListener("visibilitychange", handle);
    return () => document.removeEventListener("visibilitychange", handle);
  }, []);
  useEffect(() => {
    void visibleTick;
    if (!lastId || document.hidden) return;
    void markImRead(id, lastId)
      .then(() =>
        queryClient.setQueryData<ImConversation[]>(
          IM_CONVERSATIONS_KEY,
          (old) =>
            old?.map((item) =>
              item.id === id ? { ...item, unread_count: 0 } : item,
            ),
        ),
      )
      .catch(() => undefined);
  }, [id, lastId, queryClient, visibleTick]);

  useLayoutEffect(() => {
    const list = listRef.current;
    if (!list) return;
    if (keepOffset.current !== null) {
      list.scrollTop = list.scrollHeight - keepOffset.current;
      keepOffset.current = null;
    } else if (stickToBottom.current) {
      list.scrollTop = list.scrollHeight;
    }
  });

  const scrollToBottomIfStuck = useCallback(() => {
    const list = listRef.current;
    if (list && stickToBottom.current) list.scrollTop = list.scrollHeight;
  }, []);

  const loadOlder = async () => {
    const first = messages[0]?.id;
    if (!first || loadingOlder || !hasMore) return;
    setLoadingOlder(true);
    try {
      const rows = await fetchImMessages(id, {
        before_id: first,
        limit: PAGE_SIZE,
      });
      const list = listRef.current;
      keepOffset.current = list ? list.scrollHeight - list.scrollTop : null;
      setHasMore(rows.length === PAGE_SIZE);
      merge(rows);
    } catch (error) {
      toast.error(errorText(error, t("加载更早的消息失败")));
    } finally {
      setLoadingOlder(false);
    }
  };

  const handleScroll = () => {
    const list = listRef.current;
    if (!list) return;
    stickToBottom.current =
      list.scrollHeight - list.scrollTop - list.clientHeight < 80;
    if (list.scrollTop < 40) void loadOlder();
  };

  const sendDraft = async () => {
    if (!draft.trim() || sending) return;
    setSending(true);
    try {
      const sent = await sendImText(id, draft);
      stickToBottom.current = true;
      merge([sent]);
      setDraft("");
      onActivity();
    } catch (error) {
      toast.error(errorText(error, t("发送失败")));
    } finally {
      setSending(false);
      inputRef.current?.focus();
    }
  };

  const sendFiles = async (files: File[]) => {
    for (const file of files) {
      const key = createClientId("im-upload");
      setUploads((old) => [...old, { key, name: file.name, percent: 0 }]);
      try {
        const sent = await sendImFile(id, file, (percent) =>
          setUploads((old) =>
            old.map((item) => (item.key === key ? { ...item, percent } : item)),
          ),
        );
        stickToBottom.current = true;
        merge([sent]);
      } catch (error) {
        toast.error(errorText(error, t("{0} 发送失败", file.name)));
      } finally {
        setUploads((old) => old.filter((item) => item.key !== key));
      }
    }
    onActivity();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (
      event.key === "Enter" &&
      !event.shiftKey &&
      !event.nativeEvent.isComposing
    ) {
      event.preventDefault();
      void sendDraft();
    }
  };

  const handlePaste = (event: ClipboardEvent<HTMLTextAreaElement>) => {
    const files = Array.from(event.clipboardData.files);
    if (files.length > 0) {
      event.preventDefault();
      void sendFiles(files);
    }
  };

  const handleDrop = (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDragging(false);
    const files = Array.from(event.dataTransfer.files);
    if (files.length > 0) void sendFiles(files);
  };

  const insertEmoji = (emoji: string) => {
    const area = inputRef.current?.resizableTextArea?.textArea;
    const start = area?.selectionStart ?? draft.length;
    const end = area?.selectionEnd ?? draft.length;
    setDraft(`${draft.slice(0, start)}${emoji}${draft.slice(end)}`);
    window.requestAnimationFrame(() => {
      area?.focus();
      area?.setSelectionRange(start + emoji.length, start + emoji.length);
    });
  };

  const peer = detail?.members.find((member) => member.user_id !== viewerId);
  const title = conversation.name || t("未知用户");

  return (
    <section
      className={`im-chat${dragging ? " is-dragging" : ""}`}
      onDragOver={(event) => {
        if (event.dataTransfer.types.includes("Files")) {
          event.preventDefault();
          setDragging(true);
        }
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node)) {
          setDragging(false);
        }
      }}
      onDrop={handleDrop}
    >
      <header className="im-chat-header">
        <Button
          type="text"
          className="im-back"
          icon={<ArrowLeftOutlined />}
          aria-label={t("返回会话列表")}
          onClick={onBack}
        />
        <div className="im-chat-title">
          <Typography.Text strong ellipsis>
            {title}
            {isGroup ? ` (${conversation.member_count})` : ""}
          </Typography.Text>
          {!isGroup && peer ? (
            <Typography.Text type="secondary" ellipsis>
              {peer.email}
            </Typography.Text>
          ) : null}
        </div>
        <Tooltip title={isGroup ? t("群聊信息") : t("聊天信息")}>
          <Button
            type="text"
            icon={<MoreOutlined />}
            aria-label={isGroup ? t("群聊信息") : t("聊天信息")}
            onClick={onOpenSettings}
          />
        </Tooltip>
      </header>
      <div
        ref={listRef}
        className="im-messages"
        onScroll={handleScroll}
        role="log"
        aria-live="polite"
        aria-label={t("聊天记录")}
      >
        {loading ? (
          <div className="im-center">
            <Spin />
          </div>
        ) : (
          <>
            {hasMore ? (
              <div className="im-more">
                <Button
                  type="link"
                  size="small"
                  loading={loadingOlder}
                  onClick={() => void loadOlder()}
                >
                  {t("查看更早的消息")}
                </Button>
              </div>
            ) : null}
            {messages.length === 0 ? (
              <div className="im-center im-muted">
                {t("还没有消息，打个招呼吧")}
              </div>
            ) : null}
            {messages.map((item, index) => {
              const divider = dividerTime(
                item.created_at,
                messages[index - 1]?.created_at,
              );
              const time = divider ? (
                <div className="im-time">{divider}</div>
              ) : null;
              if (item.kind === "system") {
                return (
                  <Fragment key={item.id}>
                    {time}
                    <div className="im-system">{systemText(item.content)}</div>
                  </Fragment>
                );
              }
              const mine = item.sender_id === viewerId;
              return (
                <Fragment key={item.id}>
                  {time}
                  <div className={`im-row${mine ? " is-mine" : ""}`}>
                    <ImAvatar
                      id={item.sender_id ?? 0}
                      name={item.sender_name}
                      size={36}
                    />
                    <div className="im-bubble-column">
                      {isGroup && !mine ? (
                        <span className="im-sender">
                          {item.sender_name ?? t("已删除用户")}
                        </span>
                      ) : null}
                      <div
                        className={`im-bubble is-${item.kind}`}
                        title={new Date(item.created_at).toLocaleString()}
                      >
                        <MessageBody
                          message={item}
                          onMediaLoad={scrollToBottomIfStuck}
                        />
                      </div>
                    </div>
                  </div>
                </Fragment>
              );
            })}
          </>
        )}
      </div>
      {uploads.length > 0 ? (
        <div className="im-uploads">
          {uploads.map((upload) => (
            <div key={upload.key} className="im-upload">
              <Typography.Text ellipsis>{upload.name}</Typography.Text>
              <Progress percent={upload.percent} size="small" />
            </div>
          ))}
        </div>
      ) : null}
      <footer className="im-composer">
        <div className="im-tools">
          <Popover
            trigger="click"
            placement="topLeft"
            content={
              <div className="im-emoji-grid">
                {EMOJI.map((emoji) => (
                  <button
                    type="button"
                    key={emoji}
                    onClick={() => insertEmoji(emoji)}
                  >
                    {emoji}
                  </button>
                ))}
              </div>
            }
          >
            <Tooltip title={t("表情")}>
              <Button
                type="text"
                icon={<SmileOutlined />}
                aria-label={t("表情")}
              />
            </Tooltip>
          </Popover>
          <Tooltip title={t("发送图片")}>
            <Button
              type="text"
              icon={<PictureOutlined />}
              aria-label={t("发送图片")}
              onClick={() => imageInputRef.current?.click()}
            />
          </Tooltip>
          <Tooltip title={t("发送文件")}>
            <Button
              type="text"
              icon={<FolderOpenOutlined />}
              aria-label={t("发送文件")}
              onClick={() => fileInputRef.current?.click()}
            />
          </Tooltip>
          <input
            ref={imageInputRef}
            type="file"
            accept="image/*"
            multiple
            hidden
            onChange={(event) => {
              const files = Array.from(event.target.files ?? []);
              event.target.value = "";
              void sendFiles(files);
            }}
          />
          <input
            ref={fileInputRef}
            type="file"
            multiple
            hidden
            onChange={(event) => {
              const files = Array.from(event.target.files ?? []);
              event.target.value = "";
              void sendFiles(files);
            }}
          />
        </div>
        <Input.TextArea
          ref={inputRef}
          className="im-input"
          value={draft}
          variant="borderless"
          autoSize={{ minRows: 2, maxRows: 6 }}
          maxLength={5000}
          placeholder={t(
            "输入消息，Enter 发送，Shift+Enter 换行；可粘贴或拖入图片和文件",
          )}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
        />
        <div className="im-send">
          <Button
            type="primary"
            icon={<SendOutlined />}
            loading={sending}
            disabled={!draft.trim()}
            onClick={() => void sendDraft()}
          >
            {t("发送")}
          </Button>
        </div>
      </footer>
      {dragging ? (
        <div className="im-drop-hint">{t("松开即可发送文件")}</div>
      ) : null}
    </section>
  );
}

type SettingsProps = {
  open: boolean;
  detail?: ImConversationDetail;
  onClose: () => void;
  onInvite: () => void;
  onChanged: () => void;
  onGone: () => void;
  onStartGroup: (ids: number[]) => void;
};

function ConversationSettings({
  open,
  detail,
  onClose,
  onInvite,
  onChanged,
  onGone,
  onStartGroup,
}: SettingsProps) {
  const [name, setName] = useState("");
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setName(detail?.name ?? "");
    setEditing(false);
  }, [detail?.name]);
  if (!detail) return null;
  const isGroup = detail.kind === "group";
  const isOwner = detail.owner_id === detail.viewer_id;
  const hasOtherMember = detail.members.some(
    (member) => member.user_id !== detail.viewer_id,
  );

  const run = async (action: () => Promise<unknown>, done?: () => void) => {
    setBusy(true);
    try {
      await action();
      done?.();
    } catch (error) {
      toast.error(errorText(error, t("操作失败")));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Drawer
      title={isGroup ? t("群聊信息") : t("聊天信息")}
      open={open}
      onClose={onClose}
      width={360}
      className="im-settings"
    >
      <div className="im-member-grid">
        {detail.members.map((member) => (
          <div key={member.user_id} className="im-member">
            <ImAvatar id={member.user_id} name={member.name} size={44} />
            <Typography.Text ellipsis className="im-member-name">
              {member.name}
            </Typography.Text>
            {member.role === "owner" && isGroup ? (
              <Tag color="orange" className="im-member-tag">
                {t("群主")}
              </Tag>
            ) : null}
            {isGroup && isOwner && member.user_id !== detail.viewer_id ? (
              <Popconfirm
                title={t("将 {0} 移出群聊？", member.name)}
                onConfirm={() =>
                  run(
                    () => removeImMember(detail.id, member.user_id),
                    onChanged,
                  )
                }
              >
                <Button type="link" size="small" danger>
                  {t("移出")}
                </Button>
              </Popconfirm>
            ) : null}
          </div>
        ))}
        {isGroup || hasOtherMember ? (
          <button
            type="button"
            className="im-member im-member-add"
            onClick={() => {
              if (isGroup) onInvite();
              else
                onStartGroup(
                  detail.members
                    .map((member) => member.user_id)
                    .filter((id) => id !== detail.viewer_id),
                );
            }}
          >
            <span className="im-member-add-icon">
              <PlusOutlined />
            </span>
            <span className="im-member-name">
              {isGroup ? t("邀请") : t("发起群聊")}
            </span>
          </button>
        ) : null}
      </div>
      {isGroup ? (
        <Space direction="vertical" size={16} className="full-width">
          <div>
            <Typography.Text type="secondary">{t("群名称")}</Typography.Text>
            {editing ? (
              <Space.Compact className="full-width im-rename">
                <Input
                  value={name}
                  maxLength={60}
                  autoFocus
                  onChange={(event) => setName(event.target.value)}
                  onPressEnter={() =>
                    run(
                      () => renameImGroup(detail.id, name),
                      () => {
                        setEditing(false);
                        onChanged();
                      },
                    )
                  }
                />
                <Button
                  type="primary"
                  loading={busy}
                  disabled={!name.trim()}
                  onClick={() =>
                    run(
                      () => renameImGroup(detail.id, name),
                      () => {
                        setEditing(false);
                        onChanged();
                      },
                    )
                  }
                >
                  {t("保存")}
                </Button>
              </Space.Compact>
            ) : (
              <button
                type="button"
                className="im-rename-display"
                onClick={() => setEditing(true)}
              >
                <span>{detail.name}</span>
                <EditOutlined />
              </button>
            )}
          </div>
          <Popconfirm
            title={t("退出后将不再收到此群消息，确定退出？")}
            onConfirm={() =>
              run(() => removeImMember(detail.id, detail.viewer_id), onGone)
            }
          >
            <Button danger block loading={busy}>
              {t("退出群聊")}
            </Button>
          </Popconfirm>
          {isOwner ? (
            <Popconfirm
              title={t(
                "解散后所有成员都会离开此群，可在回收站恢复。确定解散？",
              )}
              onConfirm={() => run(() => dissolveImGroup(detail.id), onGone)}
            >
              <Button danger type="primary" block loading={busy}>
                {t("解散群聊")}
              </Button>
            </Popconfirm>
          ) : null}
        </Space>
      ) : null}
    </Drawer>
  );
}

export function ImPage() {
  const queryClient = useQueryClient();
  const [params, setParams] = useSearchParams();
  const activeId = Number(params.get("c")) || undefined;
  const [search, setSearch] = useState("");
  const [picker, setPicker] = useState<"direct" | "group" | "invite" | null>(
    null,
  );
  const [groupSeed, setGroupSeed] = useState<number[]>([]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const setActive = useCallback(
    (id?: number) => {
      setParams(id ? { c: String(id) } : {}, { replace: true });
    },
    [setParams],
  );

  const conversations = useQuery({
    queryKey: IM_CONVERSATIONS_KEY,
    queryFn: fetchImConversations,
    // The event stream keeps it fresh; this only covers missed events.
    refetchInterval: 60_000,
  });
  const detail = useQuery({
    queryKey: ["im-conversation", activeId],
    queryFn: () => fetchImConversation(activeId as number),
    enabled: Boolean(activeId),
    retry: false,
  });

  // The chat fills the space below the admin header.
  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    const measure = () => {
      const top = Math.max(0, root.getBoundingClientRect().top);
      root.style.setProperty("--im-top", `${Math.round(top)}px`);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  useEffect(() => {
    const handle = (event: Event) => {
      const data = (event as CustomEvent<ImEvent>).detail;
      if (data.type === "conversation" && data.conversation_id === activeId) {
        void detail.refetch();
      }
      if (
        data.type === "message" &&
        data.conversation_id === activeId &&
        data.message.kind === "system"
      ) {
        void detail.refetch();
      }
    };
    window.addEventListener(IM_EVENT, handle);
    return () => window.removeEventListener(IM_EVENT, handle);
  }, [activeId, detail]);

  // Removed from the group (or it was dissolved) while it was open.
  useEffect(() => {
    if (activeId && detail.isError) {
      toast.info(t("你已不在该会话中"));
      setActive(undefined);
      setSettingsOpen(false);
      void queryClient.invalidateQueries({ queryKey: IM_CONVERSATIONS_KEY });
    }
  }, [activeId, detail.isError, queryClient, setActive]);

  const refreshList = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: IM_CONVERSATIONS_KEY });
  }, [queryClient]);

  const openConversation = (created: ImConversationDetail) => {
    queryClient.setQueryData(["im-conversation", created.id], created);
    refreshList();
    setActive(created.id);
  };

  const list = conversations.data ?? [];
  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return needle
      ? list.filter((item) => item.name.toLowerCase().includes(needle))
      : list;
  }, [list, search]);
  const active =
    list.find((item) => item.id === activeId) ??
    (detail.data && detail.data.id === activeId ? detail.data : undefined);
  const totalUnread = list.reduce((sum, item) => sum + item.unread_count, 0);

  const handlePicked = async (ids: number[], name: string) => {
    setCreating(true);
    try {
      if (picker === "direct") {
        openConversation(await openImDirect(ids[0]));
      } else if (picker === "group") {
        openConversation(
          await createImGroup({ name: name || undefined, member_ids: ids }),
        );
        setSettingsOpen(false);
      } else if (picker === "invite" && activeId) {
        const updated = await inviteImMembers(activeId, ids);
        queryClient.setQueryData(["im-conversation", activeId], updated);
        refreshList();
      }
      setPicker(null);
    } catch (error) {
      toast.error(errorText(error, t("操作失败")));
    } finally {
      setCreating(false);
    }
  };

  return (
    <div ref={rootRef} className={`im-page${activeId ? " has-active" : ""}`}>
      <aside className="im-sidebar">
        <div className="im-sidebar-head">
          <Input
            allowClear
            prefix={<SearchOutlined />}
            placeholder={t("搜索会话")}
            value={search}
            onChange={(event) => setSearch(event.target.value)}
          />
          <Dropdown
            trigger={["click"]}
            menu={{
              items: [
                {
                  key: "direct",
                  icon: <MessageOutlined />,
                  label: t("发起单聊"),
                },
                { key: "group", icon: <TeamOutlined />, label: t("发起群聊") },
              ],
              onClick: ({ key }) => {
                setGroupSeed([]);
                setPicker(key as "direct" | "group");
              },
            }}
          >
            <Button icon={<PlusOutlined />} aria-label={t("新建会话")} />
          </Dropdown>
        </div>
        <nav className="im-conversations" aria-label={t("会话")}>
          {conversations.isLoading ? (
            <div className="im-center">
              <Spin />
            </div>
          ) : filtered.length === 0 ? (
            <Empty
              className="im-empty-list"
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={
                search ? t("没有匹配的会话") : t("还没有会话，点击 + 开始聊天")
              }
            />
          ) : (
            filtered.map((item) => (
              <button
                type="button"
                aria-current={item.id === activeId ? "true" : undefined}
                key={item.id}
                className={`im-conversation${item.id === activeId ? " is-active" : ""}`}
                onClick={() => setActive(item.id)}
              >
                <Badge
                  count={item.unread_count}
                  size="small"
                  overflowCount={99}
                >
                  <ImAvatar
                    id={item.peer_id ?? item.id}
                    name={item.name}
                    group={item.kind === "group"}
                  />
                </Badge>
                <span className="im-conversation-text">
                  <span className="im-conversation-top">
                    <span className="im-conversation-name">
                      {item.name || t("未知用户")}
                    </span>
                    <span className="im-conversation-time">
                      {listTime(item.last_message_at ?? item.created_at)}
                    </span>
                  </span>
                  <span className="im-conversation-preview">
                    {messagePreview(item.last_message, item)}
                  </span>
                </span>
              </button>
            ))
          )}
        </nav>
        {totalUnread > 0 ? (
          <div className="im-sidebar-foot">
            {t("{0} 条未读消息", totalUnread)}
          </div>
        ) : null}
      </aside>
      <main className="im-main">
        {active ? (
          <ChatPane
            key={active.id}
            conversation={active}
            detail={detail.data}
            onBack={() => setActive(undefined)}
            onOpenSettings={() => setSettingsOpen(true)}
            onActivity={refreshList}
          />
        ) : (
          <div className="im-placeholder">
            <CommentOutlined />
            <Typography.Text type="secondary">
              {activeId && detail.isLoading
                ? t("正在打开会话…")
                : t("选择一个会话，或点击 + 发起单聊、群聊")}
            </Typography.Text>
            <Space>
              <Button
                icon={<MessageOutlined />}
                onClick={() => setPicker("direct")}
              >
                {t("发起单聊")}
              </Button>
              <Button
                type="primary"
                icon={<UserAddOutlined />}
                onClick={() => {
                  setGroupSeed([]);
                  setPicker("group");
                }}
              >
                {t("发起群聊")}
              </Button>
            </Space>
          </div>
        )}
      </main>
      <ConversationSettings
        open={settingsOpen && Boolean(active)}
        detail={detail.data}
        onClose={() => setSettingsOpen(false)}
        onInvite={() => setPicker("invite")}
        onChanged={() => {
          void detail.refetch();
          refreshList();
        }}
        onGone={() => {
          setSettingsOpen(false);
          setActive(undefined);
          refreshList();
        }}
        onStartGroup={(ids) => {
          setGroupSeed(ids);
          setPicker("group");
        }}
      />
      <ContactPicker
        open={picker !== null}
        title={
          picker === "direct"
            ? t("发起单聊")
            : picker === "invite"
              ? t("邀请成员")
              : t("发起群聊")
        }
        multiple={picker !== "direct"}
        withName={picker === "group"}
        initialIds={picker === "group" ? groupSeed : undefined}
        excludeIds={
          picker === "invite"
            ? detail.data?.members.map((member) => member.user_id)
            : undefined
        }
        okText={picker === "invite" ? t("邀请") : t("创建")}
        confirmLoading={creating}
        onCancel={() => setPicker(null)}
        onOk={(ids, name) => void handlePicked(ids, name)}
      />
    </div>
  );
}
