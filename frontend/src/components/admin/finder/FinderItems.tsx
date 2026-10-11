import {
  AudioOutlined,
  CaretDownOutlined,
  CaretUpOutlined,
  CodeOutlined,
  FileExcelOutlined,
  FileImageOutlined,
  FileOutlined,
  FilePdfOutlined,
  FileTextOutlined,
  FileWordOutlined,
  FileZipOutlined,
  FolderFilled,
  MoreOutlined,
  PlayCircleOutlined,
} from "@ant-design/icons";
import { Drawer, Dropdown, Empty, Menu, Spin } from "antd";
import type { MenuProps } from "antd";
import {
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  useMemo,
  useState,
} from "react";
import type { FinderView } from "./FinderShell";
import {
  type FinderItem,
  type FinderKind,
  type FinderSort,
  type FinderSortKey,
  formatBytes,
  formatModified,
  kindLabel,
  sortFinderItems,
} from "./finderModel";

const KIND_ICONS: Record<FinderKind, ReactNode> = {
  folder: <FolderFilled />,
  image: <FileImageOutlined />,
  video: <PlayCircleOutlined />,
  audio: <AudioOutlined />,
  pdf: <FilePdfOutlined />,
  text: <FileTextOutlined />,
  code: <CodeOutlined />,
  archive: <FileZipOutlined />,
  sheet: <FileExcelOutlined />,
  doc: <FileWordOutlined />,
  file: <FileOutlined />,
};

export function FinderKindIcon({ kind }: { kind: FinderKind }) {
  return (
    <span className={`finder-kind-icon is-${kind}`} aria-hidden="true">
      {KIND_ICONS[kind]}
    </span>
  );
}

const SORT_COLUMNS: { key: FinderSortKey; label: string }[] = [
  { key: "name", label: "名称" },
  { key: "modified", label: "修改日期" },
  { key: "size", label: "大小" },
  { key: "kind", label: "种类" },
];

function isTouchDevice() {
  return (
    typeof window !== "undefined" &&
    Boolean(window.matchMedia?.("(pointer: coarse)").matches)
  );
}

export type FinderItemsProps = {
  items: FinderItem[];
  view: FinderView;
  loading?: boolean;
  empty?: ReactNode;
  selectedKey: string | null;
  onSelect: (key: string | null) => void;
  /** Double click (or tap on touch screens): open folder / preview file. */
  onOpen: (key: string) => void;
  menuItems: (key: string) => MenuProps["items"];
};

export function FinderItems({
  items,
  view,
  loading,
  empty,
  selectedKey,
  onSelect,
  onOpen,
  menuItems,
}: FinderItemsProps) {
  const [sort, setSort] = useState<FinderSort>({ key: "name", order: "asc" });
  const [sheetKey, setSheetKey] = useState<string | null>(null);
  const [menuKey, setMenuKey] = useState<string | null>(null);
  const sorted = useMemo(() => sortFinderItems(items, sort), [items, sort]);
  const sheetItem = items.find((item) => item.key === sheetKey);

  const activate = (item: FinderItem) => {
    onSelect(item.key);
    // Touch screens have no double click: a tap opens, like iOS Files.
    if (isTouchDevice()) onOpen(item.key);
  };

  const openMenu = (event: MouseEvent, item: FinderItem) => {
    event.stopPropagation();
    onSelect(item.key);
    if (isTouchDevice()) {
      setSheetKey(item.key);
    } else {
      setMenuKey(item.key);
    }
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const index = sorted.findIndex((item) => item.key === selectedKey);
    if (event.key === "Enter" && selectedKey) {
      event.preventDefault();
      onOpen(selectedKey);
    } else if (event.key === "Escape") {
      onSelect(null);
    } else if (
      ["ArrowDown", "ArrowRight", "ArrowUp", "ArrowLeft"].includes(event.key)
    ) {
      event.preventDefault();
      const step =
        event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : -1;
      const next =
        sorted[Math.min(sorted.length - 1, Math.max(0, index + step))];
      if (next) onSelect(next.key);
    }
  };

  const withMenu = (item: FinderItem, node: ReactNode) => (
    <Dropdown
      key={item.key}
      trigger={["contextMenu"]}
      open={menuKey === item.key}
      onOpenChange={(open) => {
        if (open) onSelect(item.key);
        setMenuKey(open ? item.key : null);
      }}
      menu={{
        items: menuItems(item.key),
        onClick: () => setMenuKey(null),
      }}
    >
      {node}
    </Dropdown>
  );

  const moreButton = (item: FinderItem) => (
    <button
      type="button"
      className="finder-item-more"
      aria-label={`更多操作：${item.name}`}
      onClick={(event) => openMenu(event, item)}
      onDoubleClick={(event) => event.stopPropagation()}
    >
      <MoreOutlined />
    </button>
  );

  const badge = (item: FinderItem) =>
    item.badge ? (
      <span className={`finder-badge is-${item.badge.tone}`}>
        {item.badge.label}
      </span>
    ) : null;

  let content: ReactNode;
  if (sorted.length === 0) {
    content = loading ? null : (
      <div className="finder-empty">
        {empty ?? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="空文件夹" />
        )}
      </div>
    );
  } else if (view === "grid") {
    content = (
      <div className="finder-grid">
        {sorted.map((item) =>
          withMenu(
            item,
            // biome-ignore lint/a11y/useKeyWithClickEvents: arrow keys and Enter are handled by the container
            <div
              className={`finder-tile${item.key === selectedKey ? " is-selected" : ""}${item.muted ? " is-muted" : ""}`}
              title={item.name}
              onClick={(event) => {
                event.stopPropagation();
                activate(item);
              }}
              onDoubleClick={() => onOpen(item.key)}
            >
              <span className="finder-tile-icon">
                <FinderKindIcon kind={item.kind} />
                {item.badge ? (
                  <span
                    className={`finder-tile-dot is-${item.badge.tone}`}
                    title={item.badge.label}
                  />
                ) : null}
              </span>
              <span className="finder-tile-name">{item.name}</span>
              {moreButton(item)}
            </div>,
          ),
        )}
      </div>
    );
  } else {
    content = (
      <div className="finder-list">
        <div className="finder-list-row finder-list-head">
          {SORT_COLUMNS.map((column) => (
            <button
              key={column.key}
              type="button"
              className={`finder-list-cell is-${column.key}${sort.key === column.key ? " is-sorted" : ""}`}
              onClick={() =>
                setSort((current) => ({
                  key: column.key,
                  order:
                    current.key === column.key && current.order === "asc"
                      ? "desc"
                      : "asc",
                }))
              }
            >
              {column.label}
              {sort.key === column.key ? (
                sort.order === "asc" ? (
                  <CaretUpOutlined />
                ) : (
                  <CaretDownOutlined />
                )
              ) : null}
            </button>
          ))}
          <span className="finder-list-cell is-actions" />
        </div>
        {sorted.map((item) =>
          withMenu(
            item,
            // biome-ignore lint/a11y/useKeyWithClickEvents: arrow keys and Enter are handled by the container
            <div
              className={`finder-list-row${item.key === selectedKey ? " is-selected" : ""}${item.muted ? " is-muted" : ""}`}
              onClick={(event) => {
                event.stopPropagation();
                activate(item);
              }}
              onDoubleClick={() => onOpen(item.key)}
            >
              <span className="finder-list-cell is-name" title={item.name}>
                <FinderKindIcon kind={item.kind} />
                <span className="finder-list-text">
                  <span className="finder-list-name">{item.name}</span>
                  <span className="finder-list-sub">
                    {item.kind === "folder"
                      ? kindLabel(item)
                      : `${formatBytes(item.sizeBytes)} · ${formatModified(item.modifiedAt)}`}
                  </span>
                </span>
                {badge(item)}
              </span>
              <span className="finder-list-cell is-modified">
                {formatModified(item.modifiedAt)}
              </span>
              <span className="finder-list-cell is-size">
                {item.kind === "folder" ? "--" : formatBytes(item.sizeBytes)}
              </span>
              <span className="finder-list-cell is-kind">
                {kindLabel(item)}
              </span>
              <span className="finder-list-cell is-actions">
                {moreButton(item)}
              </span>
            </div>,
          ),
        )}
      </div>
    );
  }

  return (
    <div
      className="finder-items"
      // biome-ignore lint/a11y/noNoninteractiveTabindex: the scrollable item area takes focus for arrow-key navigation
      tabIndex={0}
      onKeyDown={handleKeyDown}
      onClick={() => onSelect(null)}
    >
      <Spin spinning={Boolean(loading)} className="finder-items-spin">
        {content}
      </Spin>
      <Drawer
        open={Boolean(sheetItem)}
        placement="bottom"
        height="auto"
        title={
          sheetItem ? (
            <span className="finder-sheet-title">
              <FinderKindIcon kind={sheetItem.kind} />
              <span>{sheetItem.name}</span>
            </span>
          ) : null
        }
        className="finder-action-sheet"
        onClose={() => setSheetKey(null)}
      >
        {sheetItem ? (
          <Menu
            selectable={false}
            items={menuItems(sheetItem.key)}
            onClick={() => setSheetKey(null)}
          />
        ) : null}
      </Drawer>
    </div>
  );
}
