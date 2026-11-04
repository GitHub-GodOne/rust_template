import {
  DownloadOutlined,
  FileOutlined,
  LoadingOutlined,
  PictureOutlined,
  SearchOutlined,
  TeamOutlined,
} from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import {
  Button,
  Checkbox,
  Empty,
  Image,
  Input,
  Modal,
  Spin,
  Typography,
  message as toast,
} from "antd";
import { useEffect, useMemo, useState } from "react";
import {
  type ImContact,
  type ImMessage,
  fetchImContacts,
  fetchImFile,
} from "../../../api/admin/im";
import { formatBytes } from "../../../components/admin/finder/finderModel";
import { t } from "../../../i18n";
import { avatarColor, initial } from "./imFormat";

export function ImAvatar({
  id,
  name,
  group,
  size = 40,
}: {
  id: number;
  name?: string | null;
  group?: boolean;
  size?: number;
}) {
  return (
    <span
      className={`im-avatar${group ? " is-group" : ""}`}
      style={{
        width: size,
        height: size,
        fontSize: size * 0.42,
        background: group ? undefined : avatarColor(id),
      }}
      aria-hidden="true"
    >
      {group ? <TeamOutlined /> : initial(name)}
    </span>
  );
}

// Object URLs of fetched images, kept for the page's lifetime so scrolling
// back and forth does not download them again.
const imageUrls = new Map<number, Promise<string>>();

function imageUrl(messageId: number) {
  let url = imageUrls.get(messageId);
  if (!url) {
    url = fetchImFile(messageId).then((blob) => URL.createObjectURL(blob));
    url.catch(() => imageUrls.delete(messageId));
    imageUrls.set(messageId, url);
  }
  return url;
}

function ImImage({
  message,
  onLoad,
}: {
  message: ImMessage;
  onLoad?: () => void;
}) {
  const [src, setSrc] = useState<string>();
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    imageUrl(message.id)
      .then((url) => alive && setSrc(url))
      .catch(() => alive && setFailed(true));
    return () => {
      alive = false;
    };
  }, [message.id]);
  if (failed) {
    return (
      <span className="im-image-placeholder">
        <PictureOutlined /> {t("图片加载失败")}
      </span>
    );
  }
  if (!src) {
    return (
      <span className="im-image-placeholder">
        <LoadingOutlined />
      </span>
    );
  }
  return (
    <Image
      className="im-image"
      src={src}
      alt={message.file_name ?? t("图片")}
      onLoad={onLoad}
    />
  );
}

export async function downloadImFile(message: ImMessage) {
  try {
    const blob = await fetchImFile(message.id, true);
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = message.file_name ?? "file";
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 30_000);
  } catch {
    toast.error(t("文件下载失败"));
  }
}

/** The body of a chat bubble. */
export function MessageBody({
  message,
  onMediaLoad,
}: {
  message: ImMessage;
  onMediaLoad?: () => void;
}) {
  if (message.kind === "image") {
    return <ImImage message={message} onLoad={onMediaLoad} />;
  }
  if (message.kind === "file") {
    return (
      <button
        type="button"
        className="im-file"
        title={t("下载 {0}", message.file_name ?? "")}
        onClick={() => void downloadImFile(message)}
      >
        <span className="im-file-text">
          <span className="im-file-name">{message.file_name}</span>
          <span className="im-file-size">
            {formatBytes(message.size_bytes ?? 0)}
          </span>
        </span>
        <span className="im-file-icon">
          <FileOutlined />
          <DownloadOutlined className="im-file-download" />
        </span>
      </button>
    );
  }
  return <span className="im-text">{message.content}</span>;
}

type PickerProps = {
  open: boolean;
  title: string;
  /** Pick several people (group) or one (direct chat). */
  multiple: boolean;
  /** Already in the conversation; shown checked and disabled. */
  excludeIds?: number[];
  /** Pre-selected people, e.g. when a direct chat grows into a group. */
  initialIds?: number[];
  okText?: string;
  withName?: boolean;
  confirmLoading?: boolean;
  onCancel: () => void;
  onOk: (ids: number[], name: string) => void;
};

export function ContactPicker({
  open,
  title,
  multiple,
  excludeIds = [],
  initialIds,
  okText,
  withName,
  confirmLoading,
  onCancel,
  onOk,
}: PickerProps) {
  const [keyword, setKeyword] = useState("");
  const [selected, setSelected] = useState<number[]>([]);
  const [name, setName] = useState("");
  const contacts = useQuery({
    queryKey: ["im-contacts"],
    queryFn: () => fetchImContacts(),
    enabled: open,
    staleTime: 30_000,
  });
  useEffect(() => {
    if (open) {
      setKeyword("");
      setSelected(initialIds ?? []);
      setName("");
    }
  }, [open, initialIds]);
  const visible = useMemo(() => {
    const needle = keyword.trim().toLowerCase();
    return (contacts.data ?? []).filter(
      (contact) =>
        !needle ||
        contact.name.toLowerCase().includes(needle) ||
        contact.email.toLowerCase().includes(needle),
    );
  }, [contacts.data, keyword]);
  const byId = useMemo(
    () => new Map((contacts.data ?? []).map((item) => [item.id, item])),
    [contacts.data],
  );

  const toggle = (contact: ImContact) => {
    if (!multiple) {
      onOk([contact.id], "");
      return;
    }
    setSelected((current) =>
      current.includes(contact.id)
        ? current.filter((id) => id !== contact.id)
        : [...current, contact.id],
    );
  };

  return (
    <Modal
      title={title}
      open={open}
      onCancel={onCancel}
      okText={okText ?? t("确定")}
      okButtonProps={{ disabled: selected.length === 0 }}
      confirmLoading={confirmLoading}
      footer={multiple ? undefined : null}
      onOk={() => onOk(selected, name.trim())}
      className="im-picker"
      destroyOnClose
    >
      {withName ? (
        <Input
          className="im-picker-name"
          maxLength={60}
          placeholder={t("群名称（可选，默认使用成员名字）")}
          value={name}
          onChange={(event) => setName(event.target.value)}
        />
      ) : null}
      <Input
        allowClear
        autoFocus
        prefix={<SearchOutlined />}
        placeholder={t("搜索姓名或邮箱")}
        value={keyword}
        onChange={(event) => setKeyword(event.target.value)}
      />
      {multiple && selected.length > 0 ? (
        <div className="im-picker-chosen">
          {selected.map((id) => (
            <button
              type="button"
              key={id}
              className="im-picker-chip"
              title={t("移除")}
              onClick={() => setSelected((old) => old.filter((x) => x !== id))}
            >
              <ImAvatar id={id} name={byId.get(id)?.name} size={22} />
              {byId.get(id)?.name}
            </button>
          ))}
        </div>
      ) : null}
      <div className="im-picker-list">
        {contacts.isLoading ? (
          <Spin />
        ) : visible.length === 0 ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={t("没有可选择的用户")}
          />
        ) : (
          visible.map((contact) => {
            const joined = excludeIds.includes(contact.id);
            const checked = joined || selected.includes(contact.id);
            return (
              <button
                type="button"
                key={contact.id}
                className="im-picker-item"
                disabled={joined}
                onClick={() => toggle(contact)}
              >
                {multiple ? (
                  <Checkbox checked={checked} disabled={joined} tabIndex={-1} />
                ) : null}
                <ImAvatar id={contact.id} name={contact.name} size={34} />
                <span className="im-picker-text">
                  <Typography.Text strong ellipsis>
                    {contact.name}
                  </Typography.Text>
                  <Typography.Text type="secondary" ellipsis>
                    {joined ? t("已在群里") : contact.email}
                  </Typography.Text>
                </span>
              </button>
            );
          })
        )}
      </div>
      {!multiple ? (
        <div className="im-picker-footer">
          <Button onClick={onCancel}>{t("取消")}</Button>
        </div>
      ) : null}
    </Modal>
  );
}
