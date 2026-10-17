import {
  AppstoreOutlined,
  ArrowUpOutlined,
  BarsOutlined,
  LeftOutlined,
  MenuOutlined,
  RightOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import {
  Button,
  Drawer,
  Grid,
  Input,
  Progress,
  Segmented,
  Spin,
  Tooltip,
  Tree,
} from "antd";
import type { TreeProps } from "antd";
import {
  type ReactNode,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useLocation } from "react-router-dom";
import { useDockReservesBottom } from "../../../stores/dock";
import { useUiStore } from "../../../stores/ui";
import "./finder.css";

export type FinderView = "grid" | "list";

export type FinderShellProps = {
  /** Accessible name of the window, e.g. "文件管理". */
  label: string;
  /** Root crumb label (the selected location). */
  rootLabel: string;
  crumbs: { key: string; label: string }[];
  onCrumbClick: (key: string | null) => void;
  nav: {
    canBack: boolean;
    canForward: boolean;
    canUp: boolean;
    onBack: () => void;
    onForward: () => void;
    onUp: () => void;
  };
  view: FinderView;
  onViewChange: (view: FinderView) => void;
  search?: {
    value: string;
    placeholder: string;
    onChange: (value: string) => void;
  };
  /** Buttons on the right of the toolbar (new, refresh, more…). */
  actions?: ReactNode;
  sidebar: ReactNode;
  children: ReactNode;
  status?: ReactNode;
  statusAside?: ReactNode;
  /** Floating content over the item area, e.g. upload progress. */
  overlay?: ReactNode;
};

/** macOS Finder-like window: toolbar, sidebar, item area and status bar. */
export function FinderShell({
  label,
  rootLabel,
  crumbs,
  onCrumbClick,
  nav,
  view,
  onViewChange,
  search,
  actions,
  sidebar,
  children,
  status,
  statusAside,
  overlay,
}: FinderShellProps) {
  const screens = Grid.useBreakpoint();
  const isMobile = screens.md === false;
  const location = useLocation();
  const dockVisible = useDockReservesBottom(
    useUiStore((state) => state.dockVisible),
    location.pathname,
  );
  const windowRef = useRef<HTMLElement | null>(null);
  const crumbsRef = useRef<HTMLDivElement | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchDraft, setSearchDraft] = useState(search?.value ?? "");

  // The window fills the space left below the admin header.
  useLayoutEffect(() => {
    const element = windowRef.current;
    if (!element) return;
    const measure = () => {
      const top = Math.max(0, Math.round(element.getBoundingClientRect().top));
      element.style.setProperty("--finder-top", `${top}px`);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  // Keep the deepest folder visible when the path is long.
  const crumbTrail = crumbs.map((crumb) => crumb.key).join("/");
  useEffect(() => {
    const element = crumbsRef.current;
    if (element && crumbTrail !== undefined) {
      element.scrollLeft = element.scrollWidth;
    }
  }, [crumbTrail]);

  useEffect(() => {
    setSearchDraft(search?.value ?? "");
  }, [search?.value]);

  const currentTitle = crumbs.at(-1)?.label ?? rootLabel;
  const searchField = search ? (
    <Input
      allowClear
      size="small"
      className="finder-search"
      prefix={<SearchOutlined />}
      placeholder={search.placeholder}
      value={searchDraft}
      autoFocus={isMobile && searchOpen}
      onChange={(event) => {
        setSearchDraft(event.target.value);
        if (!event.target.value) search.onChange("");
      }}
      onPressEnter={() => search.onChange(searchDraft.trim())}
      onBlur={() => search.onChange(searchDraft.trim())}
    />
  ) : null;

  return (
    <section
      ref={windowRef}
      className={`finder${dockVisible ? " has-dock" : ""}`}
      aria-label={label}
    >
      <header className="finder-toolbar">
        <div className="finder-toolbar-nav">
          {isMobile ? (
            <Button
              type="text"
              size="small"
              icon={<MenuOutlined />}
              aria-label="位置与目录"
              onClick={() => setSidebarOpen(true)}
            />
          ) : null}
          <Tooltip title="后退">
            <Button
              type="text"
              size="small"
              icon={<LeftOutlined />}
              aria-label="后退"
              disabled={!nav.canBack}
              onClick={nav.onBack}
            />
          </Tooltip>
          {isMobile ? null : (
            <>
              <Tooltip title="前进">
                <Button
                  type="text"
                  size="small"
                  icon={<RightOutlined />}
                  aria-label="前进"
                  disabled={!nav.canForward}
                  onClick={nav.onForward}
                />
              </Tooltip>
              <Tooltip title="上一级">
                <Button
                  type="text"
                  size="small"
                  icon={<ArrowUpOutlined />}
                  aria-label="上一级"
                  disabled={!nav.canUp}
                  onClick={nav.onUp}
                />
              </Tooltip>
            </>
          )}
        </div>
        {isMobile ? (
          <strong className="finder-mobile-title" title={currentTitle}>
            {currentTitle}
          </strong>
        ) : (
          <nav className="finder-crumbs" ref={crumbsRef} aria-label="当前路径">
            <button
              type="button"
              className={`finder-crumb${crumbs.length === 0 ? " is-current" : ""}`}
              onClick={() => onCrumbClick(null)}
            >
              {rootLabel}
            </button>
            {crumbs.map((crumb, index) => (
              <span key={crumb.key} className="finder-crumb-item">
                <RightOutlined className="finder-crumb-separator" />
                <button
                  type="button"
                  className={`finder-crumb${index === crumbs.length - 1 ? " is-current" : ""}`}
                  onClick={() => onCrumbClick(crumb.key)}
                >
                  {crumb.label}
                </button>
              </span>
            ))}
          </nav>
        )}
        <div className="finder-toolbar-actions">
          {isMobile ? (
            <>
              {search ? (
                <Button
                  type="text"
                  size="small"
                  icon={<SearchOutlined />}
                  aria-label="搜索"
                  onClick={() => setSearchOpen((open) => !open)}
                />
              ) : null}
              <Button
                type="text"
                size="small"
                icon={view === "grid" ? <BarsOutlined /> : <AppstoreOutlined />}
                aria-label={view === "grid" ? "列表视图" : "图标视图"}
                onClick={() => onViewChange(view === "grid" ? "list" : "grid")}
              />
            </>
          ) : (
            <>
              <Segmented<FinderView>
                size="small"
                className="finder-view-toggle"
                value={view}
                onChange={onViewChange}
                options={[
                  {
                    value: "grid",
                    icon: <AppstoreOutlined />,
                    title: "图标视图",
                  },
                  { value: "list", icon: <BarsOutlined />, title: "列表视图" },
                ]}
              />
              {searchField}
            </>
          )}
          {actions}
        </div>
      </header>
      {isMobile && searchOpen && search ? (
        <div className="finder-mobile-search">{searchField}</div>
      ) : null}
      <div className="finder-body">
        {isMobile ? (
          <Drawer
            open={sidebarOpen}
            placement="left"
            width={280}
            title={label}
            className="finder-sidebar-drawer"
            onClose={() => setSidebarOpen(false)}
          >
            {/* Picking a location or folder closes the drawer. */}
            <div
              className="finder-sidebar-scroll"
              onClickCapture={(event) => {
                const target = event.target as HTMLElement;
                if (
                  target.closest(
                    ".finder-sidebar-item, .ant-tree-node-content-wrapper",
                  )
                ) {
                  window.setTimeout(() => setSidebarOpen(false), 120);
                }
              }}
            >
              {sidebar}
            </div>
          </Drawer>
        ) : (
          <aside className="finder-sidebar" aria-label="位置与目录">
            <div className="finder-sidebar-scroll">{sidebar}</div>
          </aside>
        )}
        <div className="finder-main">
          {children}
          {overlay}
        </div>
      </div>
      {status || statusAside ? (
        <footer className="finder-statusbar">
          <span className="finder-status-text">{status}</span>
          <span className="finder-status-aside">{statusAside}</span>
        </footer>
      ) : null}
    </section>
  );
}

export function FinderSidebarSection({
  title,
  extra,
  children,
}: {
  title: string;
  extra?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="finder-sidebar-section">
      <div className="finder-sidebar-heading">
        <span>{title}</span>
        {extra}
      </div>
      {children}
    </div>
  );
}

export function FinderSidebarItem({
  icon,
  label,
  hint,
  active,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  hint?: string;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`finder-sidebar-item${active ? " is-active" : ""}`}
      title={hint ? `${label}\n${hint}` : label}
      aria-current={active ? "true" : undefined}
      onClick={onClick}
    >
      <span className="finder-sidebar-icon">{icon}</span>
      <span className="finder-sidebar-label">{label}</span>
    </button>
  );
}

/** Folder tree whose long names scroll sideways instead of wrapping. */
export function FinderTree(props: TreeProps) {
  return <Tree showIcon blockNode={false} className="finder-tree" {...props} />;
}

/** Floating upload progress chip shown over the item area. */
export function FinderProgress({
  name,
  stage,
  percent,
}: {
  name: string;
  stage?: string;
  percent: number;
}) {
  return (
    <div className="finder-progress" aria-live="polite">
      <Spin size="small" />
      <span className="finder-progress-name" title={name}>
        {name}
      </span>
      <span className="finder-progress-percent">{percent}%</span>
      <span className="finder-progress-stage">{stage ?? "正在上传"}</span>
      <Progress
        percent={percent}
        size="small"
        status="active"
        showInfo={false}
      />
    </div>
  );
}
