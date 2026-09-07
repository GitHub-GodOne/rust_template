import {
  AppstoreOutlined,
  CloudUploadOutlined,
  DeleteOutlined,
  EditOutlined,
  FileAddOutlined,
  FolderAddOutlined,
  FolderOpenOutlined,
  GlobalOutlined,
  HomeOutlined,
  LinkOutlined,
  PlusOutlined,
  ReloadOutlined,
  TableOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Badge,
  Breadcrumb,
  Button,
  Card,
  Empty,
  Form,
  Grid,
  Input,
  InputNumber,
  List,
  Modal,
  Popconfirm,
  Progress,
  Segmented,
  Space,
  Spin,
  Switch,
  Tag,
  Typography,
  message,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import {
  type ChangeEvent,
  type DragEvent,
  type InputHTMLAttributes,
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
import { TableScrollArea } from "../../../components/admin/TableScrollArea";
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

const STATIC_TABLE_WIDTH = 1320;
const MAX_UPLOAD_FILES = 2000;
const directoryInputAttributes: DirectoryInputAttributes = {
  directory: "",
  webkitdirectory: "",
};

function joinRelativePath(prefix: string, name: string) {
  const cleanName = name.trim().replace(/^\/+|\/+$/g, "");
  if (!cleanName) {
    return prefix;
  }
  return prefix ? `${prefix}/${cleanName}` : cleanName;
}

function parentPath(path: string) {
  const index = path.lastIndexOf("/");
  return index > -1 ? path.slice(0, index) : "";
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

export function StaticHostingPage() {
  const [selectedSiteKey, setSelectedSiteKey] = useState<string>();
  const [currentPath, setCurrentPath] = useState("");
  const [keyword, setKeyword] = useState("");
  const [viewMode, setViewMode] = useState<"table" | "card">("table");
  const [siteModalOpen, setSiteModalOpen] = useState(false);
  const [editingSite, setEditingSite] = useState<StaticSiteRecord | null>(null);
  const [folderModalOpen, setFolderModalOpen] = useState(false);
  const [overwrite, setOverwrite] = useState(true);
  const [dragActive, setDragActive] = useState(false);
  const [tablePage, setTablePage] = useState(1);
  const [tablePageSize, setTablePageSize] = useState(10);
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
  const queryClient = useQueryClient();
  const canUpload = useAuthStore((state) =>
    state.hasPermission("system:static_host:upload"),
  );
  const screens = Grid.useBreakpoint();
  const effectiveViewMode = screens.md === false ? "card" : viewMode;

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
    siteForm.setFieldsValue(
      site ?? {
        key: "",
        name: "",
        url_path: "/site",
        local_root: "storage/static-hosting/site",
        index_file: "index.html",
        spa_fallback: false,
        enabled: true,
        cache_seconds: 300,
      },
    );
    setSiteModalOpen(true);
  };

  const openPath = (path: string) => {
    setCurrentPath(path);
    setTablePage(1);
  };

  const copyUrl = async (path: string) => {
    try {
      await navigator.clipboard.writeText(absoluteUrl(path));
      message.success("URL 已复制");
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

  const handleDrop = async (event: DragEvent<HTMLButtonElement>) => {
    event.preventDefault();
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
    return allRows.filter(
      (record) =>
        record.name.toLowerCase().includes(normalizedKeyword) ||
        record.path.toLowerCase().includes(normalizedKeyword),
    );
  }, [allRows, keyword]);

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

  const renderActions = (record: StaticFileRecord) => (
    <Space size={4}>
      {record.is_dir ? (
        <Button
          size="small"
          icon={<FolderOpenOutlined />}
          onClick={() => openPath(record.path)}
        >
          打开
        </Button>
      ) : (
        <Button
          size="small"
          icon={<LinkOutlined />}
          href={absoluteUrl(record.url)}
          target="_blank"
          rel="noreferrer"
        >
          访问
        </Button>
      )}
      <Popconfirm
        title={
          record.is_dir ? "确认删除该目录及全部内容？" : "确认删除该文件？"
        }
        onConfirm={() => deleteFileMutation.mutate(record)}
      >
        <PermissionButton
          size="small"
          danger
          icon={<DeleteOutlined />}
          permission="system:static_host:delete"
        />
      </Popconfirm>
    </Space>
  );

  const columns: ColumnsType<StaticFileRecord> = [
    {
      title: "名称",
      dataIndex: "name",
      width: 250,
      ellipsis: true,
      render: (value, record) => (
        <Space size={8}>
          <span className="static-host-file-icon">
            {record.is_dir ? <FolderOpenOutlined /> : fileIcon(record)}
          </span>
          <Typography.Text ellipsis>{value}</Typography.Text>
        </Space>
      ),
    },
    { title: "路径", dataIndex: "path", width: 280, ellipsis: true },
    {
      title: "类型",
      key: "type",
      width: 150,
      render: (_, record) =>
        record.is_dir ? (
          <Tag color="blue">目录</Tag>
        ) : (
          (record.mime_type ?? "文件")
        ),
    },
    {
      title: "大小",
      key: "size",
      width: 110,
      render: (_, record) =>
        record.is_dir ? "-" : formatBytes(record.size_bytes),
    },
    { title: "更新时间", dataIndex: "updated_at", width: 220 },
    {
      title: "公开 URL",
      dataIndex: "url",
      width: 190,
      ellipsis: true,
      render: (value) => (
        <Typography.Text copyable={{ text: absoluteUrl(value) }} ellipsis>
          {value}
        </Typography.Text>
      ),
    },
    {
      title: "操作",
      key: "actions",
      width: 120,
      render: (_, record) => renderActions(record),
    },
  ];

  const pagination = (
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
  );

  const uploadPercent =
    uploadState.total > 0
      ? Math.round((uploadState.completed / uploadState.total) * 100)
      : 0;
  const segments = pathSegments(currentPath);

  return (
    <CrudPage
      title="静态托管"
      subtitle="管理 URL 路径与服务器静态目录映射"
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
        <Space wrap>
          <Input.Search
            allowClear
            placeholder="搜索文件名、路径"
            className="admin-search-input"
            onSearch={(value) => {
              setKeyword(value);
              setTablePage(1);
            }}
          />
          <Button icon={<ReloadOutlined />} onClick={refresh}>
            刷新
          </Button>
        </Space>
      }
    >
      <div className="static-hosting-layout">
        <aside className="static-host-sites-panel">
          <div className="static-host-panel-heading">
            <Typography.Text strong>托管站点</Typography.Text>
            <Tag>{sites.length}</Tag>
          </div>
          <Spin spinning={sitesQuery.isLoading}>
            {sites.length > 0 ? (
              <List
                dataSource={sites}
                renderItem={(site) => (
                  <List.Item className="static-host-site-list-item">
                    <button
                      type="button"
                      className={[
                        "static-host-site-button",
                        selectedSiteKey === site.key ? "is-active" : undefined,
                      ]
                        .filter(Boolean)
                        .join(" ")}
                      onClick={() => {
                        setSelectedSiteKey(site.key);
                        setCurrentPath("");
                        setTablePage(1);
                      }}
                    >
                      <span className="static-host-site-icon">
                        <GlobalOutlined />
                      </span>
                      <span className="static-host-site-copy">
                        <span className="static-host-site-name">
                          {site.name}
                        </span>
                        <span className="static-host-site-path">
                          {site.url_path}
                        </span>
                      </span>
                      <Badge status={site.enabled ? "success" : "default"} />
                    </button>
                  </List.Item>
                )}
              />
            ) : (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description="暂无托管站点"
              />
            )}
          </Spin>
        </aside>

        <main className="static-host-content-panel">
          {selectedSite ? (
            <>
              <section className="static-host-site-header">
                <div className="static-host-site-summary">
                  <Space wrap size={8}>
                    <Typography.Title level={4}>
                      {selectedSite.name}
                    </Typography.Title>
                    <Tag color={selectedSite.enabled ? "green" : "default"}>
                      {selectedSite.enabled ? "已启用" : "已停用"}
                    </Tag>
                    {selectedSite.spa_fallback ? <Tag>SPA 回退</Tag> : null}
                  </Space>
                  <Typography.Text type="secondary" ellipsis copyable>
                    {selectedSite.local_root}
                  </Typography.Text>
                </div>
                <Space wrap className="static-host-site-actions">
                  <Button
                    icon={<LinkOutlined />}
                    href={absoluteUrl(selectedSite.url_path)}
                    target="_blank"
                    rel="noreferrer"
                    disabled={!selectedSite.enabled}
                  >
                    打开站点
                  </Button>
                  <Button
                    icon={<LinkOutlined />}
                    onClick={() => copyUrl(selectedSite.url_path)}
                  >
                    复制 URL
                  </Button>
                  <PermissionButton
                    icon={<EditOutlined />}
                    permission="system:static_host:update"
                    onClick={() => openSiteModal(selectedSite)}
                  >
                    编辑
                  </PermissionButton>
                  <Popconfirm
                    title="确认删除托管映射？服务器目录和文件会保留。"
                    onConfirm={() => deleteSiteMutation.mutate(selectedSite)}
                  >
                    <PermissionButton
                      danger
                      icon={<DeleteOutlined />}
                      permission="system:static_host:delete"
                    >
                      删除映射
                    </PermissionButton>
                  </Popconfirm>
                </Space>
              </section>

              <section className="static-host-path-toolbar">
                <Breadcrumb
                  items={[
                    {
                      title: (
                        <Button
                          type="link"
                          size="small"
                          icon={<HomeOutlined />}
                          onClick={() => openPath("")}
                        >
                          根目录
                        </Button>
                      ),
                    },
                    ...segments.map((segment) => ({
                      title: (
                        <Button
                          type="link"
                          size="small"
                          onClick={() => openPath(segment.path)}
                        >
                          {segment.name}
                        </Button>
                      ),
                    })),
                  ]}
                />
                <Space wrap className="static-host-upload-actions">
                  <Space size={6}>
                    <Typography.Text type="secondary">
                      覆盖同名文件
                    </Typography.Text>
                    <Switch checked={overwrite} onChange={setOverwrite} />
                  </Space>
                  <PermissionButton
                    icon={<FolderAddOutlined />}
                    permission="system:static_host:upload"
                    onClick={() => setFolderModalOpen(true)}
                  >
                    新建目录
                  </PermissionButton>
                  <PermissionButton
                    icon={<FileAddOutlined />}
                    permission="system:static_host:upload"
                    disabled={uploadState.active}
                    onClick={() => fileInputRef.current?.click()}
                  >
                    上传文件
                  </PermissionButton>
                  <PermissionButton
                    icon={<FolderOpenOutlined />}
                    permission="system:static_host:upload"
                    disabled={uploadState.active}
                    onClick={() => directoryInputRef.current?.click()}
                  >
                    上传文件夹
                  </PermissionButton>
                  <Segmented
                    value={viewMode}
                    onChange={(value) => setViewMode(value as "table" | "card")}
                    options={[
                      {
                        value: "table",
                        icon: <TableOutlined />,
                        label: "表格",
                      },
                      {
                        value: "card",
                        icon: <AppstoreOutlined />,
                        label: "文件",
                      },
                    ]}
                  />
                </Space>
              </section>

              <input
                ref={fileInputRef}
                className="static-host-hidden-input"
                type="file"
                multiple
                onChange={handleInputFiles}
              />
              <input
                {...directoryInputAttributes}
                ref={directoryInputRef}
                className="static-host-hidden-input"
                type="file"
                multiple
                onChange={handleInputFiles}
              />

              <button
                type="button"
                className={[
                  "static-host-drop-zone",
                  dragActive ? "is-dragging" : undefined,
                  uploadState.active ? "is-uploading" : undefined,
                  !canUpload ? "is-disabled" : undefined,
                ]
                  .filter(Boolean)
                  .join(" ")}
                onClick={() => {
                  if (canUpload) {
                    fileInputRef.current?.click();
                  }
                }}
                onDragEnter={(event) => {
                  event.preventDefault();
                  setDragActive(true);
                }}
                onDragOver={(event) => event.preventDefault()}
                onDragLeave={(event) => {
                  if (
                    !event.currentTarget.contains(event.relatedTarget as Node)
                  ) {
                    setDragActive(false);
                  }
                }}
                onDrop={(event) => void handleDrop(event)}
                disabled={!canUpload || uploadState.active}
              >
                <CloudUploadOutlined />
                <div>
                  <Typography.Text strong>拖放站点文件</Typography.Text>
                  <Typography.Text type="secondary">
                    {currentPath || "根目录"}
                  </Typography.Text>
                </div>
              </button>

              {uploadState.total > 0 ? (
                <div className="static-host-upload-progress">
                  <div>
                    <Typography.Text strong>
                      {uploadState.active ? "正在上传" : "上传完成"}
                    </Typography.Text>
                    <Typography.Text type="secondary">
                      {uploadState.completed}/{uploadState.total}
                      {uploadState.failed > 0
                        ? `，失败 ${uploadState.failed}`
                        : ""}
                    </Typography.Text>
                  </div>
                  <Progress
                    percent={uploadPercent}
                    status={uploadState.failed > 0 ? "exception" : "active"}
                    showInfo={false}
                  />
                </div>
              ) : null}

              {effectiveViewMode === "table" ? (
                <TableScrollArea
                  contentWidth={STATIC_TABLE_WIDTH}
                  label="静态托管文件表格"
                  footer={pagination}
                >
                  <DataTable<StaticFileRecord>
                    rowKey="path"
                    columns={columns}
                    dataSource={pagedRows}
                    loading={browserQuery.isLoading}
                    pagination={false}
                    tableLayout="fixed"
                    scroll={{ x: STATIC_TABLE_WIDTH }}
                    onRow={(record) => ({
                      onDoubleClick: () =>
                        record.is_dir
                          ? openPath(record.path)
                          : window.open(
                              absoluteUrl(record.url),
                              "_blank",
                              "noopener,noreferrer",
                            ),
                    })}
                  />
                </TableScrollArea>
              ) : (
                <Spin spinning={browserQuery.isLoading}>
                  {pagedRows.length > 0 ? (
                    <>
                      <div className="static-host-file-grid">
                        {pagedRows.map((record) => (
                          <Card
                            key={record.path}
                            className="static-host-file-card"
                            hoverable
                          >
                            <button
                              type="button"
                              className="static-host-file-preview"
                              onClick={() =>
                                record.is_dir
                                  ? openPath(record.path)
                                  : window.open(
                                      absoluteUrl(record.url),
                                      "_blank",
                                      "noopener,noreferrer",
                                    )
                              }
                            >
                              {record.is_dir ? (
                                <FolderOpenOutlined />
                              ) : (
                                fileIcon(record)
                              )}
                            </button>
                            <Typography.Text
                              strong
                              ellipsis
                              title={record.name}
                            >
                              {record.name}
                            </Typography.Text>
                            <div className="static-host-file-meta">
                              <span>
                                {record.is_dir
                                  ? "目录"
                                  : (record.mime_type ?? "文件")}
                              </span>
                              <span>
                                {record.is_dir
                                  ? "-"
                                  : formatBytes(record.size_bytes)}
                              </span>
                            </div>
                            <Typography.Text
                              type="secondary"
                              ellipsis
                              title={record.path}
                            >
                              {record.path}
                            </Typography.Text>
                            <div className="static-host-file-card-actions">
                              {renderActions(record)}
                            </div>
                          </Card>
                        ))}
                      </div>
                      <div className="static-host-card-pagination">
                        {pagination}
                      </div>
                    </>
                  ) : (
                    <Empty description="当前目录暂无文件" />
                  )}
                </Spin>
              )}
            </>
          ) : (
            <div className="static-host-empty-state">
              <Empty description="创建一个静态站点开始托管" />
              <PermissionButton
                type="primary"
                icon={<PlusOutlined />}
                permission="system:static_host:create"
                onClick={() => openSiteModal()}
              >
                新建站点
              </PermissionButton>
            </div>
          )}
        </main>
      </div>

      <Modal
        title={editingSite ? "编辑静态站点" : "新建静态站点"}
        open={siteModalOpen}
        onCancel={() => {
          setSiteModalOpen(false);
          setEditingSite(null);
        }}
        onOk={() => siteForm.submit()}
        confirmLoading={saveSiteMutation.isPending}
        width="min(680px, 96vw)"
      >
        <Form
          form={siteForm}
          layout="vertical"
          onFinish={(values) => saveSiteMutation.mutate(values)}
        >
          <div className="static-host-site-form-grid">
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
            <Input addonBefore={window.location.origin} placeholder="/docs" />
          </Form.Item>
          <Form.Item
            name="local_root"
            label="服务器目录"
            rules={[{ required: true, message: "请输入服务器目录" }]}
            extra="此目录中的文件会通过公开 URL 访问，请勿指向包含密钥或配置文件的目录。"
          >
            <Input placeholder="storage/static-hosting/docs" />
          </Form.Item>
          <div className="static-host-site-form-grid">
            <Form.Item
              name="index_file"
              label="默认首页"
              rules={[{ required: true, message: "请输入默认首页文件" }]}
            >
              <Input placeholder="index.html" />
            </Form.Item>
            <Form.Item name="cache_seconds" label="资源缓存秒数">
              <InputNumber min={0} max={31_536_000} className="full-width" />
            </Form.Item>
          </div>
          <div className="static-host-site-switches">
            <Form.Item name="enabled" valuePropName="checked" label="启用托管">
              <Switch checkedChildren="启用" unCheckedChildren="停用" />
            </Form.Item>
            <Form.Item
              name="spa_fallback"
              valuePropName="checked"
              label="SPA 回退"
            >
              <Switch checkedChildren="启用" unCheckedChildren="关闭" />
            </Form.Item>
          </div>
        </Form>
      </Modal>

      <Modal
        title="新建目录"
        open={folderModalOpen}
        onCancel={() => setFolderModalOpen(false)}
        onOk={() => folderForm.submit()}
        confirmLoading={createFolderMutation.isPending}
      >
        <Form
          form={folderForm}
          layout="vertical"
          onFinish={(values) => createFolderMutation.mutate(values)}
        >
          <Form.Item label="当前目录">
            <Typography.Text>{currentPath || "根目录"}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="name"
            label="目录名称"
            rules={[{ required: true, message: "请输入目录名称" }]}
          >
            <Input placeholder="assets" />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
