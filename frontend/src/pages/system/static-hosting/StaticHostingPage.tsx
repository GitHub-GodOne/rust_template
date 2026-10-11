import {
  AppstoreOutlined,
  CheckOutlined,
  CloudUploadOutlined,
  CopyOutlined,
  DeleteOutlined,
  DownOutlined,
  EditOutlined,
  ExportOutlined,
  FileAddOutlined,
  FolderAddOutlined,
  FolderFilled,
  FolderOpenOutlined,
  GlobalOutlined,
  HomeOutlined,
  PlusOutlined,
  ReloadOutlined,
  SearchOutlined,
  TableOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Breadcrumb,
  Button,
  Dropdown,
  Empty,
  Form,
  Grid,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Progress,
  Segmented,
  Spin,
  Switch,
  Typography,
  message,
} from "antd";
import type { MenuProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import {
  type ChangeEvent,
  type DragEvent,
  type InputHTMLAttributes,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type SaveStaticSiteParams,
  type StaticFileRecord,
  type StaticSiteRecord,
  createStaticFolder,
  createStaticSite,
  deleteStaticFile,
  deleteStaticSite,
  fetchStaticBrowser,
  fetchStaticSites,
  updateStaticSite,
  uploadStaticFile,
} from "../../../api/admin/staticHosting";
import { CrudPage } from "../../../components/admin/CrudPage";
import { DataTable } from "../../../components/admin/DataTable";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { StandardTablePagination } from "../../../components/admin/StandardTablePagination";
import { useAuthStore } from "../../../stores/auth";
import { fileIcon, formatBytes } from "../uploads/uploadTaskManager";

type SiteFormValues = SaveStaticSiteParams;

type FolderFormValues = {
  name: string;
};

type UploadEntry = {
  file: File;
  relativePath: string;
};

type UploadState = {
  active: boolean;
  completed: number;
  failed: number;
  total: number;
};

type DirectoryInputAttributes = InputHTMLAttributes<HTMLInputElement> & {
  directory?: string;
  webkitdirectory?: string;
};

type ViewMode = "table" | "card";

const STATIC_TABLE_MIN_WIDTH = 680;
const MAX_UPLOAD_FILES = 2000;
const directoryInputAttributes: DirectoryInputAttributes = {
  directory: "",
  webkitdirectory: "",
};

const defaultSiteValues: SiteFormValues = {
  key: "",
  name: "",
  url_path: "/site",
  local_root: "storage/static-hosting/site",
  index_file: "index.html",
  spa_fallback: false,
  directory_listing: false,
  enabled: true,
  cache_seconds: 300,
};

function joinRelativePath(prefix: string, name: string) {
  const cleanName = name.trim().replace(/^\/+|\/+$/g, "");
  if (!cleanName) {
    return prefix;
  }
  return prefix ? `${prefix}/${cleanName}` : cleanName;
}

function pathSegments(path: string) {
  const segments = path.split("/").filter(Boolean);
  return segments.map((name, index) => ({
    name,
    path: segments.slice(0, index + 1).join("/"),
  }));
}

function absoluteUrl(path: string) {
  return new URL(path, window.location.origin).toString();
}

function formatTime(value?: string | null) {
  if (!value) {
    return "—";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

function fileTypeLabel(record: StaticFileRecord) {
  if (record.is_dir) {
    return "文件夹";
  }
  return record.extension ? record.extension.toUpperCase() : "文件";
}

function fileFromEntry(entry: FileSystemFileEntry) {
  return new Promise<File>((resolve, reject) => entry.file(resolve, reject));
}

async function readDirectoryEntries(entry: FileSystemDirectoryEntry) {
  const reader = entry.createReader();
  const entries: FileSystemEntry[] = [];
  while (true) {
    const batch = await new Promise<FileSystemEntry[]>((resolve, reject) =>
      reader.readEntries(resolve, reject),
    );
    if (batch.length === 0) {
      break;
    }
    entries.push(...batch);
  }
  return entries;
}

async function collectEntryFiles(
  entry: FileSystemEntry,
  prefix = "",
): Promise<UploadEntry[]> {
  if (entry.isFile) {
    const file = await fileFromEntry(entry as FileSystemFileEntry);
    return [{ file, relativePath: joinRelativePath(prefix, entry.name) }];
  }
  if (!entry.isDirectory) {
    return [];
  }
  const directoryPrefix = joinRelativePath(prefix, entry.name);
  const children = await readDirectoryEntries(
    entry as FileSystemDirectoryEntry,
  );
  const nested = await Promise.all(
    children.map((child) => collectEntryFiles(child, directoryPrefix)),
  );
  return nested.flat();
}

async function collectDroppedFiles(dataTransfer: DataTransfer) {
  const entries = Array.from(dataTransfer.items)
    .filter((item) => item.kind === "file")
    .map((item) => item.webkitGetAsEntry())
    .filter((entry): entry is FileSystemEntry => Boolean(entry));
  if (entries.length > 0) {
    const files = await Promise.all(
      entries.map((entry) => collectEntryFiles(entry)),
    );
    return files.flat();
  }
  return Array.from(dataTransfer.files).map((file) => ({
    file,
    relativePath: file.webkitRelativePath || file.name,
  }));
}

function uniqueUploadEntries(entries: UploadEntry[]) {
  const paths = new Set<string>();
  return entries.filter((entry) => {
    const path = entry.relativePath.replace(/^\/+/, "");
    if (!path || paths.has(path)) {
      return false;
    }
    paths.add(path);
    entry.relativePath = path;
    return true;
  });
}

function hasDraggedFiles(event: DragEvent<HTMLElement>) {
  return Array.from(event.dataTransfer.types).includes("Files");
}

function SiteFact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="sh-fact">
      <span className="sh-fact-label">{label}</span>
      <span className="sh-fact-value">{children}</span>
    </div>
  );
}

function SiteOption({
  name,
  title,
  description,
}: {
  name: keyof SiteFormValues;
  title: string;
  description: string;
}) {
  return (
    <div className="sh-option">
      <div className="sh-option-copy">
        <span className="sh-option-title">{title}</span>
        <span className="sh-option-description">{description}</span>
      </div>
      <Form.Item name={name} valuePropName="checked" noStyle>
        <Switch aria-label={title} />
      </Form.Item>
    </div>
  );
}

export function StaticHostingPage() {
  const [selectedSiteKey, setSelectedSiteKey] = useState<string>();
  const [currentPath, setCurrentPath] = useState("");
  const [keyword, setKeyword] = useState("");
  const [viewMode, setViewMode] = useState<ViewMode>("table");
  const [siteModalOpen, setSiteModalOpen] = useState(false);
  const [editingSite, setEditingSite] = useState<StaticSiteRecord | null>(null);
  const [folderModalOpen, setFolderModalOpen] = useState(false);
  const [overwrite, setOverwrite] = useState(true);
  const [dragActive, setDragActive] = useState(false);
  const [tablePage, setTablePage] = useState(1);
  const [tablePageSize, setTablePageSize] = useState(20);
  const [uploadState, setUploadState] = useState<UploadState>({
    active: false,
    completed: 0,
    failed: 0,
    total: 0,
  });
  const [siteForm] = Form.useForm<SiteFormValues>();
  const [folderForm] = Form.useForm<FolderFormValues>();
  const fileInputRef = useRef<HTMLInputElement>(null);
  const directoryInputRef = useRef<HTMLInputElement>(null);
  const dragDepthRef = useRef(0);
  const queryClient = useQueryClient();
  const canUpload = useAuthStore((state) =>
    state.hasPermission("system:static_host:upload"),
  );
  const screens = Grid.useBreakpoint();
  const effectiveViewMode: ViewMode = screens.md === false ? "card" : viewMode;

  const sitesQuery = useQuery({
    queryKey: ["admin-static-sites"],
    queryFn: fetchStaticSites,
  });
  const sites = sitesQuery.data ?? [];

  useEffect(() => {
    if (sites.length === 0) {
      setSelectedSiteKey(undefined);
      return;
    }
    if (
      !selectedSiteKey ||
      !sites.some((site) => site.key === selectedSiteKey)
    ) {
      setSelectedSiteKey(sites[0].key);
      setCurrentPath("");
      setTablePage(1);
    }
  }, [selectedSiteKey, sites]);

  const selectedSite = sites.find((site) => site.key === selectedSiteKey);
  const browserQuery = useQuery({
    queryKey: ["admin-static-browser", selectedSiteKey, currentPath],
    enabled: Boolean(selectedSiteKey),
    queryFn: () =>
      fetchStaticBrowser({
        site_key: selectedSiteKey ?? "",
        path: currentPath || undefined,
      }),
  });

  const refresh = () => {
    queryClient.invalidateQueries({ queryKey: ["admin-static-sites"] });
    queryClient.invalidateQueries({ queryKey: ["admin-static-browser"] });
  };

  const saveSiteMutation = useMutation({
    mutationFn: (values: SiteFormValues) =>
      editingSite
        ? updateStaticSite(editingSite.key, values)
        : createStaticSite(values),
    onSuccess: (site) => {
      message.success(editingSite ? "站点配置已更新" : "静态站点已创建");
      setSelectedSiteKey(site.key);
      setCurrentPath("");
      setSiteModalOpen(false);
      setEditingSite(null);
      siteForm.resetFields();
      refresh();
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "保存站点失败");
    },
  });

  const deleteSiteMutation = useMutation({
    mutationFn: (site: StaticSiteRecord) => deleteStaticSite(site.key),
    onSuccess: () => {
      message.success("托管映射已删除，服务器文件已保留");
      setSelectedSiteKey(undefined);
      setCurrentPath("");
      refresh();
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "删除站点失败");
    },
  });

  const createFolderMutation = useMutation({
    mutationFn: (values: FolderFormValues) => {
      if (!selectedSiteKey) {
        throw new Error("请先选择静态站点");
      }
      return createStaticFolder({
        site_key: selectedSiteKey,
        path: joinRelativePath(currentPath, values.name),
      });
    },
    onSuccess: () => {
      message.success("目录已创建");
      setFolderModalOpen(false);
      folderForm.resetFields();
      refresh();
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "创建目录失败");
    },
  });

  const deleteFileMutation = useMutation({
    mutationFn: (record: StaticFileRecord) => {
      if (!selectedSiteKey) {
        throw new Error("请先选择静态站点");
      }
      return deleteStaticFile({
        site_key: selectedSiteKey,
        path: record.path,
      });
    },
    onSuccess: (_, record) => {
      message.success(record.is_dir ? "目录已删除" : "文件已删除");
      refresh();
    },
    onError: (error) => {
      message.error(error instanceof Error ? error.message : "删除失败");
    },
  });

  const openSiteModal = (site?: StaticSiteRecord) => {
    setEditingSite(site ?? null);
    siteForm.resetFields();
    siteForm.setFieldsValue(site ?? defaultSiteValues);
    setSiteModalOpen(true);
  };

  const selectSite = (key: string) => {
    setSelectedSiteKey(key);
    setCurrentPath("");
    setKeyword("");
    setTablePage(1);
  };

  const openPath = (path: string) => {
    setCurrentPath(path);
    setKeyword("");
    setTablePage(1);
  };

  const openPublicUrl = (path: string) => {
    window.open(absoluteUrl(path), "_blank", "noopener,noreferrer");
  };

  const copyUrl = async (path: string) => {
    try {
      await navigator.clipboard.writeText(absoluteUrl(path));
      message.success("链接已复制");
    } catch {
      message.error("复制失败，请检查浏览器权限");
    }
  };

  const uploadEntries = async (rawEntries: UploadEntry[]) => {
    if (!canUpload) {
      message.error("当前账号没有上传静态托管文件的权限");
      return;
    }
    if (!selectedSiteKey) {
      message.error("请先选择静态站点");
      return;
    }
    if (uploadState.active) {
      message.warning("已有上传任务正在进行");
      return;
    }
    const entries = uniqueUploadEntries(rawEntries);
    if (entries.length === 0) {
      return;
    }
    if (entries.length > MAX_UPLOAD_FILES) {
      message.error(`单次最多上传 ${MAX_UPLOAD_FILES} 个文件`);
      return;
    }

    setUploadState({
      active: true,
      completed: 0,
      failed: 0,
      total: entries.length,
    });
    let cursor = 0;
    let completed = 0;
    let failed = 0;
    let firstError: string | undefined;

    const worker = async () => {
      while (cursor < entries.length) {
        const index = cursor;
        cursor += 1;
        const entry = entries[index];
        try {
          await uploadStaticFile(entry.file, {
            site_key: selectedSiteKey,
            path: joinRelativePath(currentPath, entry.relativePath),
            overwrite,
          });
        } catch (error) {
          failed += 1;
          firstError ??=
            error instanceof Error
              ? error.message
              : `${entry.relativePath} 上传失败`;
        } finally {
          completed += 1;
          setUploadState({
            active: true,
            completed,
            failed,
            total: entries.length,
          });
        }
      }
    };

    await Promise.all(
      Array.from({ length: Math.min(3, entries.length) }, () => worker()),
    );
    setUploadState({ active: false, completed, failed, total: entries.length });
    refresh();
    if (failed > 0) {
      message.error(
        `${failed} 个文件上传失败：${firstError ?? "请检查文件路径"}`,
      );
    } else {
      message.success(`${completed} 个托管文件上传完成`);
    }
  };

  const handleInputFiles = (event: ChangeEvent<HTMLInputElement>) => {
    const entries = Array.from(event.target.files ?? []).map((file) => ({
      file,
      relativePath: file.webkitRelativePath || file.name,
    }));
    event.target.value = "";
    void uploadEntries(entries);
  };

  const canDropFiles = canUpload && !uploadState.active;

  const handleDragEnter = (event: DragEvent<HTMLElement>) => {
    if (!canDropFiles || !hasDraggedFiles(event)) {
      return;
    }
    event.preventDefault();
    dragDepthRef.current += 1;
    setDragActive(true);
  };

  const handleDragOver = (event: DragEvent<HTMLElement>) => {
    if (!canDropFiles || !hasDraggedFiles(event)) {
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  };

  const handleDragLeave = (event: DragEvent<HTMLElement>) => {
    if (!dragActive) {
      return;
    }
    event.preventDefault();
    dragDepthRef.current = Math.max(0, dragDepthRef.current - 1);
    if (dragDepthRef.current === 0) {
      setDragActive(false);
    }
  };

  const handleDrop = async (event: DragEvent<HTMLElement>) => {
    if (!canDropFiles || !hasDraggedFiles(event)) {
      return;
    }
    event.preventDefault();
    dragDepthRef.current = 0;
    setDragActive(false);
    try {
      const entries = await collectDroppedFiles(event.dataTransfer);
      await uploadEntries(entries);
    } catch (error) {
      message.error(
        error instanceof Error ? error.message : "读取拖拽文件夹失败",
      );
    }
  };

  const allRows = useMemo(
    () => [
      ...(browserQuery.data?.directories ?? []),
      ...(browserQuery.data?.files ?? []),
    ],
    [browserQuery.data],
  );
  const rows = useMemo(() => {
    const normalizedKeyword = keyword.trim().toLowerCase();
    if (!normalizedKeyword) {
      return allRows;
    }
    return allRows.filter((record) =>
      record.name.toLowerCase().includes(normalizedKeyword),
    );
  }, [allRows, keyword]);
  const directoryCount = allRows.filter((record) => record.is_dir).length;
  const fileCount = allRows.length - directoryCount;

  useEffect(() => {
    const lastPage = Math.max(1, Math.ceil(rows.length / tablePageSize));
    if (tablePage > lastPage) {
      setTablePage(lastPage);
    }
  }, [rows.length, tablePage, tablePageSize]);

  const pagedRows = useMemo(() => {
    const start = (tablePage - 1) * tablePageSize;
    return rows.slice(start, start + tablePageSize);
  }, [rows, tablePage, tablePageSize]);

  const openRecord = (record: StaticFileRecord) => {
    if (record.is_dir) {
      openPath(record.path);
    } else {
      openPublicUrl(record.url);
    }
  };

  const renderActions = (record: StaticFileRecord) => (
    <div className="sh-row-actions">
      {record.is_dir ? (
        <Button
          size="small"
          type="text"
          icon={<FolderOpenOutlined />}
          title="打开目录"
          onClick={() => openPath(record.path)}
        >
          打开
        </Button>
      ) : (
        <Button
          size="small"
          type="text"
          icon={<ExportOutlined />}
          title="在新标签页访问"
          onClick={() => openPublicUrl(record.url)}
        >
          访问
        </Button>
      )}
      <Button
        size="small"
        type="text"
        icon={<CopyOutlined />}
        title="复制公开链接"
        aria-label="复制公开链接"
        onClick={() => void copyUrl(record.url)}
      />
      <Popconfirm
        title={record.is_dir ? "删除该目录及其全部内容？" : "删除该文件？"}
        okButtonProps={{ danger: true }}
        okText="删除"
        onConfirm={() => deleteFileMutation.mutate(record)}
      >
        <PermissionButton
          size="small"
          type="text"
          danger
          icon={<DeleteOutlined />}
          title="删除"
          aria-label="删除"
          permission="system:static_host:delete"
        />
      </Popconfirm>
    </div>
  );

  const columns: ColumnsType<StaticFileRecord> = [
    {
      title: "名称",
      dataIndex: "name",
      ellipsis: true,
      render: (value: string, record) => (
        <button
          type="button"
          className="sh-name-cell"
          onClick={() => openRecord(record)}
          title={record.is_dir ? `打开 ${value}` : `访问 ${value}`}
        >
          <span
            className={`sh-name-icon${record.is_dir ? " is-folder" : ""}`}
            aria-hidden
          >
            {record.is_dir ? <FolderFilled /> : fileIcon(record)}
          </span>
          <span className="sh-name-text">{value}</span>
        </button>
      ),
    },
    {
      title: "类型",
      key: "type",
      width: 96,
      render: (_, record) => (
        <span className="sh-muted" title={record.mime_type ?? undefined}>
          {fileTypeLabel(record)}
        </span>
      ),
    },
    {
      title: "大小",
      key: "size",
      width: 100,
      align: "right",
      render: (_, record) => (
        <span className="sh-muted sh-num">
          {record.is_dir ? "—" : formatBytes(record.size_bytes)}
        </span>
      ),
    },
    {
      title: "修改时间",
      dataIndex: "updated_at",
      width: 160,
      render: (value?: string | null) => (
        <span className="sh-muted sh-num">{formatTime(value)}</span>
      ),
    },
    {
      title: "操作",
      key: "actions",
      width: 168,
      render: (_, record) => renderActions(record),
    },
  ];

  const uploadPercent =
    uploadState.total > 0
      ? Math.round((uploadState.completed / uploadState.total) * 100)
      : 0;
  const segments = pathSegments(currentPath);
  const currentLocation = currentPath ? `/${currentPath}` : "根目录";

  const uploadMenu: MenuProps = {
    items: [
      {
        key: "files",
        icon: <FileAddOutlined />,
        label: "上传文件",
      },
      {
        key: "directory",
        icon: <FolderOpenOutlined />,
        label: "上传文件夹",
      },
      { type: "divider" },
      {
        key: "overwrite",
        icon: overwrite ? (
          <CheckOutlined />
        ) : (
          <span className="sh-menu-check-placeholder" />
        ),
        label: "覆盖同名文件",
      },
    ],
    onClick: ({ key }) => {
      if (key === "files") {
        fileInputRef.current?.click();
      } else if (key === "directory") {
        directoryInputRef.current?.click();
      } else if (key === "overwrite") {
        setOverwrite((value) => !value);
      }
    },
  };

  const renderSiteList = () => {
    if (sitesQuery.isLoading) {
      return (
        <div className="sh-sites-placeholder">
          <Spin size="small" />
        </div>
      );
    }
    if (sites.length === 0) {
      return (
        <div className="sh-sites-placeholder">
          <Typography.Text type="secondary">暂无站点</Typography.Text>
        </div>
      );
    }
    return (
      <nav className="sh-site-list" aria-label="托管站点">
        {sites.map((site) => (
          <button
            key={site.key}
            type="button"
            className={`sh-site-item${selectedSiteKey === site.key ? " is-active" : ""}`}
            aria-current={selectedSiteKey === site.key ? "true" : undefined}
            onClick={() => selectSite(site.key)}
          >
            <span className="sh-site-item-main">
              <span className="sh-site-item-name">{site.name}</span>
              <span
                className={`sh-status-dot${site.enabled ? " is-on" : ""}`}
                title={site.enabled ? "已启用" : "已停用"}
              />
            </span>
            <span className="sh-site-item-path">{site.url_path}</span>
          </button>
        ))}
      </nav>
    );
  };

  const renderBrowserBody = () => {
    if (browserQuery.isError) {
      return (
        <div className="sh-browser-empty">
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              browserQuery.error instanceof Error
                ? browserQuery.error.message
                : "读取目录失败"
            }
          />
        </div>
      );
    }
    if (!browserQuery.isLoading && rows.length === 0) {
      return (
        <div className="sh-browser-empty">
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              keyword ? "没有匹配的文件" : "此目录为空，拖放文件到这里即可上传"
            }
          />
        </div>
      );
    }
    if (effectiveViewMode === "table") {
      return (
        <div className="sh-table">
          <DataTable<StaticFileRecord>
            rowKey="path"
            columns={columns}
            dataSource={pagedRows}
            loading={browserQuery.isLoading}
            pagination={false}
            tableLayout="fixed"
            scroll={{ x: STATIC_TABLE_MIN_WIDTH }}
            onRow={(record) => ({
              onDoubleClick: () => openRecord(record),
            })}
          />
        </div>
      );
    }
    return (
      <Spin spinning={browserQuery.isLoading}>
        <div className="sh-grid">
          {pagedRows.map((record) => (
            <div key={record.path} className="sh-tile">
              <button
                type="button"
                className="sh-tile-preview"
                onClick={() => openRecord(record)}
                title={record.is_dir ? "打开目录" : "访问文件"}
              >
                <span
                  className={`sh-tile-icon${record.is_dir ? " is-folder" : ""}`}
                >
                  {record.is_dir ? <FolderFilled /> : fileIcon(record)}
                </span>
                <span className="sh-tile-name" title={record.name}>
                  {record.name}
                </span>
                <span className="sh-tile-meta">
                  {record.is_dir
                    ? "文件夹"
                    : `${fileTypeLabel(record)} · ${formatBytes(record.size_bytes)}`}
                </span>
              </button>
              <div className="sh-tile-actions">{renderActions(record)}</div>
            </div>
          ))}
        </div>
      </Spin>
    );
  };

  return (
    <CrudPage
      className="static-host-page"
      title="静态托管"
      subtitle="把服务器目录映射到公开 URL 路径"
      breadcrumb={["系统管理", "静态托管"]}
      icon={<GlobalOutlined />}
      extra={
        <PermissionButton
          type="primary"
          icon={<PlusOutlined />}
          permission="system:static_host:create"
          onClick={() => openSiteModal()}
        >
          新建站点
        </PermissionButton>
      }
      toolbar={
        <Button icon={<ReloadOutlined />} onClick={refresh}>
          刷新
        </Button>
      }
    >
      <div className="sh-layout">
        <aside className="sh-panel sh-sites">
          <div className="sh-sites-heading">
            <span>站点</span>
            <span className="sh-count">{sites.length}</span>
          </div>
          {renderSiteList()}
        </aside>

        <div className="sh-main">
          {selectedSite ? (
            <>
              <section className="sh-panel sh-overview">
                <div className="sh-overview-head">
                  <div className="sh-overview-title">
                    <span className="sh-overview-icon">
                      <GlobalOutlined />
                    </span>
                    <div className="sh-overview-copy">
                      <h2>{selectedSite.name}</h2>
                      <div className="sh-pills">
                        <span
                          className={`sh-pill${selectedSite.enabled ? " is-success" : ""}`}
                        >
                          {selectedSite.enabled ? "运行中" : "已停用"}
                        </span>
                        {selectedSite.directory_listing ? (
                          <span className="sh-pill">目录列表</span>
                        ) : null}
                        {selectedSite.spa_fallback ? (
                          <span className="sh-pill">SPA 回退</span>
                        ) : null}
                        <span className="sh-pill is-quiet">
                          {selectedSite.key}
                        </span>
                      </div>
                    </div>
                  </div>
                  <div className="sh-overview-actions">
                    <Button
                      icon={<ExportOutlined />}
                      disabled={!selectedSite.enabled}
                      onClick={() => openPublicUrl(selectedSite.url_path)}
                    >
                      打开站点
                    </Button>
                    <PermissionButton
                      icon={<EditOutlined />}
                      permission="system:static_host:update"
                      onClick={() => openSiteModal(selectedSite)}
                    >
                      编辑
                    </PermissionButton>
                    <Popconfirm
                      title="删除这个托管映射？"
                      description="只移除 URL 映射，服务器上的目录和文件会保留。"
                      okButtonProps={{ danger: true }}
                      okText="删除映射"
                      onConfirm={() => deleteSiteMutation.mutate(selectedSite)}
                    >
                      <PermissionButton
                        danger
                        icon={<DeleteOutlined />}
                        aria-label="删除映射"
                        title="删除映射"
                        permission="system:static_host:delete"
                      />
                    </Popconfirm>
                  </div>
                </div>
                <div className="sh-facts">
                  <SiteFact label="访问地址">
                    <Typography.Text
                      className="sh-mono"
                      ellipsis
                      copyable={{
                        text: absoluteUrl(selectedSite.url_path),
                        tooltips: ["复制完整链接", "已复制"],
                      }}
                    >
                      {selectedSite.url_path}
                    </Typography.Text>
                  </SiteFact>
                  <SiteFact label="服务器目录">
                    <Typography.Text
                      className="sh-mono"
                      ellipsis={{ tooltip: selectedSite.local_root }}
                    >
                      {selectedSite.local_root}
                    </Typography.Text>
                  </SiteFact>
                  <SiteFact label="默认首页">
                    <span className="sh-mono">{selectedSite.index_file}</span>
                  </SiteFact>
                  <SiteFact label="资源缓存">
                    <span className="sh-num">
                      {selectedSite.cache_seconds} 秒
                    </span>
                  </SiteFact>
                </div>
              </section>

              <section
                className={`sh-panel sh-browser${dragActive ? " is-dragging" : ""}`}
                aria-label="站点文件"
                onDragEnter={handleDragEnter}
                onDragOver={handleDragOver}
                onDragLeave={handleDragLeave}
                onDrop={(event) => void handleDrop(event)}
              >
                <div className="sh-browser-toolbar">
                  <Breadcrumb
                    className="sh-breadcrumb"
                    items={[
                      {
                        title: (
                          <button
                            type="button"
                            className="sh-crumb"
                            onClick={() => openPath("")}
                          >
                            <HomeOutlined />
                            <span>根目录</span>
                          </button>
                        ),
                      },
                      ...segments.map((segment, index) => ({
                        title:
                          index === segments.length - 1 ? (
                            <span className="sh-crumb is-current">
                              {segment.name}
                            </span>
                          ) : (
                            <button
                              type="button"
                              className="sh-crumb"
                              onClick={() => openPath(segment.path)}
                            >
                              {segment.name}
                            </button>
                          ),
                      })),
                    ]}
                  />
                  <div className="sh-browser-tools">
                    <Input
                      allowClear
                      value={keyword}
                      prefix={<SearchOutlined />}
                      placeholder="筛选当前目录"
                      className="sh-filter"
                      onChange={(event) => {
                        setKeyword(event.target.value);
                        setTablePage(1);
                      }}
                    />
                    {screens.md !== false ? (
                      <Segmented<ViewMode>
                        value={viewMode}
                        onChange={setViewMode}
                        options={[
                          {
                            value: "table",
                            title: "列表视图",
                            icon: <TableOutlined />,
                          },
                          {
                            value: "card",
                            title: "图标视图",
                            icon: <AppstoreOutlined />,
                          },
                        ]}
                      />
                    ) : null}
                    <PermissionButton
                      icon={<FolderAddOutlined />}
                      permission="system:static_host:upload"
                      onClick={() => setFolderModalOpen(true)}
                    >
                      新建目录
                    </PermissionButton>
                    {canUpload ? (
                      <Dropdown
                        menu={uploadMenu}
                        trigger={["click"]}
                        placement="bottomRight"
                        disabled={uploadState.active}
                      >
                        <Button
                          type="primary"
                          icon={<CloudUploadOutlined />}
                          loading={uploadState.active}
                        >
                          上传 <DownOutlined className="sh-caret" />
                        </Button>
                      </Dropdown>
                    ) : null}
                  </div>
                </div>

                <input
                  ref={fileInputRef}
                  className="sh-hidden-input"
                  type="file"
                  multiple
                  onChange={handleInputFiles}
                />
                <input
                  {...directoryInputAttributes}
                  ref={directoryInputRef}
                  className="sh-hidden-input"
                  type="file"
                  multiple
                  onChange={handleInputFiles}
                />

                {uploadState.total > 0 ? (
                  <div className="sh-upload-status">
                    <span>
                      {uploadState.active ? "正在上传" : "上传完成"}{" "}
                      <span className="sh-num">
                        {uploadState.completed}/{uploadState.total}
                      </span>
                      {uploadState.failed > 0
                        ? ` · ${uploadState.failed} 个失败`
                        : ""}
                    </span>
                    <Progress
                      percent={uploadPercent}
                      size="small"
                      status={
                        uploadState.failed > 0
                          ? "exception"
                          : uploadState.active
                            ? "active"
                            : "success"
                      }
                      showInfo={false}
                    />
                  </div>
                ) : null}

                <div className="sh-browser-body">{renderBrowserBody()}</div>

                <footer className="sh-browser-footer">
                  <span className="sh-browser-summary">
                    {directoryCount} 个文件夹 · {fileCount} 个文件
                    {canUpload ? (
                      <span className="sh-drop-hint">
                        {" "}
                        · 拖放文件或文件夹到此处上传
                        {overwrite ? "（覆盖同名）" : ""}
                      </span>
                    ) : null}
                  </span>
                  {rows.length > tablePageSize ? (
                    <StandardTablePagination
                      current={tablePage}
                      pageSize={tablePageSize}
                      total={rows.length}
                      onChange={(nextPage, nextPageSize) => {
                        const sizeChanged = nextPageSize !== tablePageSize;
                        setTablePageSize(nextPageSize);
                        setTablePage(sizeChanged ? 1 : nextPage);
                      }}
                    />
                  ) : null}
                </footer>

                {dragActive ? (
                  <div className="sh-drop-overlay" aria-hidden>
                    <CloudUploadOutlined />
                    <strong>松开即可上传</strong>
                    <span>上传到 {currentLocation}</span>
                  </div>
                ) : null}
              </section>
            </>
          ) : (
            <section className="sh-panel sh-empty">
              <span className="sh-empty-icon">
                <GlobalOutlined />
              </span>
              <h3>
                {sitesQuery.isLoading ? "正在加载站点…" : "还没有托管站点"}
              </h3>
              <p>
                新建一个站点，把服务器上的目录发布到指定 URL
                路径，之后可以在这里上传和管理文件。
              </p>
              {sitesQuery.isLoading ? null : (
                <PermissionButton
                  type="primary"
                  icon={<PlusOutlined />}
                  permission="system:static_host:create"
                  onClick={() => openSiteModal()}
                >
                  新建站点
                </PermissionButton>
              )}
            </section>
          )}
        </div>
      </div>

      <Modal
        title={editingSite ? "编辑静态站点" : "新建静态站点"}
        open={siteModalOpen}
        onCancel={() => {
          setSiteModalOpen(false);
          setEditingSite(null);
        }}
        onOk={() => siteForm.submit()}
        okText={editingSite ? "保存" : "创建"}
        confirmLoading={saveSiteMutation.isPending}
        width="min(640px, 96vw)"
        className="sh-site-modal"
        forceRender
      >
        <Form
          form={siteForm}
          layout="vertical"
          requiredMark={false}
          initialValues={defaultSiteValues}
          onFinish={(values) => saveSiteMutation.mutate(values)}
        >
          <div className="sh-form-grid">
            <Form.Item
              name="name"
              label="站点名称"
              rules={[{ required: true, message: "请输入站点名称" }]}
            >
              <Input placeholder="例如 产品文档" />
            </Form.Item>
            <Form.Item
              name="key"
              label="站点标识"
              rules={[
                { required: true, message: "请输入站点标识" },
                {
                  pattern: /^[a-z0-9_-]+$/,
                  message: "仅支持小写字母、数字、短横线和下划线",
                },
              ]}
            >
              <Input disabled={Boolean(editingSite)} placeholder="docs" />
            </Form.Item>
          </div>
          <Form.Item
            name="url_path"
            label="URL 路径"
            rules={[{ required: true, message: "请输入 URL 路径" }]}
          >
            <Input
              prefix={
                <span className="sh-url-origin">{window.location.origin}</span>
              }
              placeholder="/docs"
            />
          </Form.Item>
          <Form.Item
            name="local_root"
            label="服务器目录"
            rules={[{ required: true, message: "请输入服务器目录" }]}
            extra="目录中的文件会通过公开 URL 访问，请勿指向包含密钥或配置文件的目录。"
          >
            <Input placeholder="storage/static-hosting/docs" />
          </Form.Item>
          <div className="sh-form-grid">
            <Form.Item
              name="index_file"
              label="默认首页"
              rules={[{ required: true, message: "请输入默认首页文件" }]}
            >
              <Input placeholder="index.html" />
            </Form.Item>
            <Form.Item name="cache_seconds" label="资源缓存（秒）">
              <InputNumber min={0} max={31_536_000} className="full-width" />
            </Form.Item>
          </div>
          <div className="sh-options">
            <SiteOption
              name="enabled"
              title="启用托管"
              description="关闭后该 URL 路径不再对外提供访问"
            />
            <SiteOption
              name="directory_listing"
              title="目录列表"
              description="目录中没有默认首页时，列出其中的文件和子目录"
            />
            <SiteOption
              name="spa_fallback"
              title="SPA 回退"
              description="找不到的页面路径统一返回默认首页，适合前端路由应用"
            />
          </div>
        </Form>
      </Modal>

      <Modal
        title="新建目录"
        open={folderModalOpen}
        onCancel={() => setFolderModalOpen(false)}
        onOk={() => folderForm.submit()}
        okText="创建"
        confirmLoading={createFolderMutation.isPending}
      >
        <Form
          form={folderForm}
          layout="vertical"
          requiredMark={false}
          onFinish={(values) => createFolderMutation.mutate(values)}
        >
          <Form.Item
            name="name"
            label={`在 ${currentLocation} 下新建`}
            rules={[{ required: true, message: "请输入目录名称" }]}
          >
            <Input placeholder="assets" />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
