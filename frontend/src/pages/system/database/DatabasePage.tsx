import {
  ApiOutlined,
  CodeOutlined,
  ConsoleSqlOutlined,
  DatabaseOutlined,
  DisconnectOutlined,
  EyeOutlined,
  FieldNumberOutlined,
  FileTextOutlined,
  FolderOpenOutlined,
  FolderOutlined,
  FunctionOutlined,
  PartitionOutlined,
  PlusOutlined,
  ReloadOutlined,
  TableOutlined,
} from "@ant-design/icons";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Dropdown,
  Empty,
  Input,
  Modal,
  Spin,
  Tabs,
  Tag,
  Tooltip,
  Tree,
  Typography,
  message,
} from "antd";
import type { MenuProps } from "antd";
import type { DataNode } from "antd/es/tree";
import {
  type Key,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type DbConnectInfo,
  type DbConnection,
  type DbExportFormat,
  type DbObject,
  type DbObjectKind,
  type DbSavedQuery,
  connectDb,
  deleteDbConnection,
  deleteDbSavedQuery,
  disconnectDb,
  executeDbSql,
  exportDbRows,
  fetchDbConnections,
  fetchDbObjects,
  fetchDbRoutine,
  fetchDbSavedQueries,
  fetchDbSchemas,
} from "../../../api/admin/dbManager";
import { t } from "../../../i18n";
import { useAuthStore } from "../../../stores/auth";
import { ConnectionModal } from "./ConnectionModal";
import { QueryEditor } from "./QueryEditor";
import { DdlView, StructureView } from "./StructureView";
import { TableDataView } from "./TableDataView";
import {
  DB_TYPE_LABELS,
  dropStatement,
  formatBytes,
  isTableLike,
  newSessionKey,
  objectKindLabel,
  selectTemplate,
  truncateStatement,
} from "./dbUtils";
import "./database.css";

const QUERY_PERMISSION = "system:database:query";
const MANAGE_PERMISSION = "system:database:manage";

type ObjectGroup = "tables" | "views" | "functions" | "sequences";

/** Tree node identity, JSON-encoded as the node key. */
type NodeRef =
  | ["folder", string]
  | ["c", number]
  | ["d", number, string]
  | ["s", number, string, string]
  | ["g", number, string, string | null, ObjectGroup]
  | ["o", number, string, string | null, DbObjectKind, string, string | null]
  | ["qg", number]
  | ["q", number, number];

type TabBase = { key: string; title: string; connectionId: number };
type WorkTab =
  | (TabBase & {
      kind: "data" | "structure";
      database: string;
      schema?: string;
      table: string;
      objectKind: DbObjectKind;
    })
  | (TabBase & {
      kind: "query";
      database: string;
      schema?: string;
      sql?: string;
      savedQuery?: DbSavedQuery;
    })
  | (TabBase & {
      kind: "routine";
      database: string;
      schema?: string;
      name: string;
      objectKind: DbObjectKind;
      identity?: string | null;
    });

const nodeKey = (ref: NodeRef) => JSON.stringify(ref);
const parseKey = (key: Key) => JSON.parse(String(key)) as NodeRef;
const objectsKey = (id: number, database: string, schema?: string | null) =>
  JSON.stringify([id, database, schema ?? null]);

const GROUP_KINDS: Record<ObjectGroup, DbObjectKind[]> = {
  tables: ["table", "foreign_table"],
  views: ["view", "materialized_view"],
  functions: ["function", "procedure"],
  sequences: ["sequence"],
};

function groupLabel(group: ObjectGroup) {
  switch (group) {
    case "tables":
      return t("表");
    case "views":
      return t("视图");
    case "functions":
      return t("函数");
    case "sequences":
      return t("序列");
  }
}

function objectIcon(kind: DbObjectKind) {
  switch (kind) {
    case "view":
    case "materialized_view":
      return <EyeOutlined />;
    case "function":
    case "procedure":
      return <FunctionOutlined />;
    case "sequence":
      return <FieldNumberOutlined />;
    default:
      return <TableOutlined />;
  }
}

function copyText(text: string) {
  void navigator.clipboard
    ?.writeText(text)
    .then(() => message.success(t("已复制")));
}

function RoutineView({
  tab,
  dbType,
  onOpenQuery,
}: {
  tab: Extract<WorkTab, { kind: "routine" }>;
  dbType: DbConnection["db_type"];
  onOpenQuery: (sql: string) => void;
}) {
  const query = useQuery({
    queryKey: [
      "db-routine",
      tab.connectionId,
      tab.database,
      tab.name,
      tab.identity,
    ],
    queryFn: () =>
      fetchDbRoutine(tab.connectionId, {
        database: tab.database,
        schema: tab.schema,
        name: tab.name,
        kind: tab.objectKind,
        identity: tab.identity,
      }),
    retry: false,
  });
  if (query.error) {
    return (
      <Typography.Text type="danger">
        {(query.error as Error).message}
      </Typography.Text>
    );
  }
  if (query.data === undefined) return <Spin className="db-center-spin" />;
  return <DdlView ddl={query.data} dbType={dbType} onOpenQuery={onOpenQuery} />;
}

export function DatabasePage() {
  const queryClient = useQueryClient();
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const canQuery = hasPermission(QUERY_PERMISSION);
  const canManage = hasPermission(MANAGE_PERMISSION);
  const pageRef = useRef<HTMLDivElement>(null);

  const [connected, setConnected] = useState<Record<number, DbConnectInfo>>({});
  const [schemas, setSchemas] = useState<Record<string, string[]>>({});
  const [objects, setObjects] = useState<Record<string, DbObject[]>>({});
  const [expandedKeys, setExpandedKeys] = useState<Key[]>([]);
  const [loadedKeys, setLoadedKeys] = useState<Key[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [tabs, setTabs] = useState<WorkTab[]>([]);
  const [activeTab, setActiveTab] = useState<string>();
  const [modal, setModal] = useState<{
    open: boolean;
    editing?: DbConnection | null;
  }>({
    open: false,
  });
  const queryCounter = useRef(0);

  useEffect(() => {
    const root = pageRef.current;
    if (!root) return;
    const measure = () => {
      const top = Math.max(0, root.getBoundingClientRect().top);
      root.style.setProperty("--db-top", `${Math.round(top)}px`);
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, []);

  const connectionsQuery = useQuery({
    queryKey: ["db-connections"],
    queryFn: fetchDbConnections,
  });
  const savedQueriesQuery = useQuery({
    queryKey: ["db-saved-queries"],
    queryFn: () => fetchDbSavedQueries(),
  });
  const connections = connectionsQuery.data ?? [];
  const connectionById = useMemo(
    () => new Map(connections.map((connection) => [connection.id, connection])),
    [connections],
  );
  const groups = useMemo(
    () =>
      [
        ...new Set(connections.map((item) => item.group_name).filter(Boolean)),
      ] as string[],
    [connections],
  );

  // ---------------------------------------------------------------------
  // Loading tree levels
  // ---------------------------------------------------------------------

  const loadObjects = async (
    id: number,
    database: string,
    schema?: string | null,
  ) => {
    const list = await fetchDbObjects(id, {
      database,
      schema: schema ?? undefined,
    });
    setObjects((current) => ({
      ...current,
      [objectsKey(id, database, schema)]: list,
    }));
  };

  const loadNode = async (key: string) => {
    const ref = parseKey(key);
    try {
      if (ref[0] === "c") {
        const info = await connectDb(ref[1]);
        setConnected((current) => ({ ...current, [ref[1]]: info }));
        if (info.db_type === "sqlite") {
          await loadObjects(ref[1], "main");
        }
        void queryClient.invalidateQueries({ queryKey: ["db-connections"] });
      } else if (ref[0] === "d") {
        const info = connected[ref[1]];
        if (info?.has_schemas) {
          const list = await fetchDbSchemas(ref[1], ref[2]);
          setSchemas((current) => ({
            ...current,
            [objectsKey(ref[1], ref[2])]: list,
          }));
        } else {
          await loadObjects(ref[1], ref[2]);
        }
      } else if (ref[0] === "s") {
        await loadObjects(ref[1], ref[2], ref[3]);
      }
      setLoadedKeys((current) => [...new Set([...current, key])]);
    } catch (error) {
      message.error((error as Error).message);
      setExpandedKeys((current) => current.filter((item) => item !== key));
    }
  };

  /** Reloads a node and everything below it. */
  const refreshNode = (key: string) => {
    const ref = parseKey(key);
    const prefix = (keyText: string) => {
      const other = parseKey(keyText);
      if (ref[0] === "c") return other[0] !== "folder" && other[1] === ref[1];
      if (ref[0] === "d") return other[1] === ref[1] && other[2] === ref[2];
      if (ref[0] === "s")
        return (
          other[1] === ref[1] && other[2] === ref[2] && other[3] === ref[3]
        );
      return keyText === key;
    };
    setLoadedKeys((current) => current.filter((item) => !prefix(String(item))));
    if (ref[0] === "c" || ref[0] === "d" || ref[0] === "s") {
      if (!expandedKeys.includes(key)) {
        setExpandedKeys((current) => [...current, key]);
      }
      void loadNode(key);
    }
    if (ref[0] === "g") {
      const parent: NodeRef =
        ref[3] === null
          ? connected[ref[1]]?.db_type === "sqlite"
            ? ["c", ref[1]]
            : ["d", ref[1], ref[2]]
          : ["s", ref[1], ref[2], ref[3]];
      void loadObjects(ref[1], ref[2], ref[3]).catch((error: Error) =>
        message.error(error.message),
      );
      setLoadedKeys((current) => [...new Set([...current, nodeKey(parent)])]);
    }
    void savedQueriesQuery.refetch();
  };

  const disconnect = (id: number) => {
    void disconnectDb(id).catch(() => undefined);
    setConnected((current) => {
      const next = { ...current };
      delete next[id];
      return next;
    });
    const belongs = (key: Key) => {
      const ref = parseKey(key);
      return ref[0] !== "folder" && ref[1] === id;
    };
    setExpandedKeys((current) => current.filter((key) => !belongs(key)));
    setLoadedKeys((current) => current.filter((key) => !belongs(key)));
    const keep = <T,>(cache: Record<string, T>) =>
      Object.fromEntries(
        Object.entries(cache).filter(([key]) => JSON.parse(key)[0] !== id),
      );
    setSchemas(keep);
    setObjects(keep);
  };

  /** Expands a node and loads it; antd only lazy-loads on a user click. */
  const expandAndLoad = (key: string) => {
    setExpandedKeys((current) =>
      current.includes(key) ? current : [...current, key],
    );
    if (!loadedKeys.includes(key)) void loadNode(key);
  };

  // ---------------------------------------------------------------------
  // Tabs
  // ---------------------------------------------------------------------

  const openTab = (tab: WorkTab) => {
    setTabs((current) =>
      current.some((item) => item.key === tab.key)
        ? current
        : [...current, tab],
    );
    setActiveTab(tab.key);
  };

  const openQuery = (
    connectionId: number,
    options: {
      database?: string;
      schema?: string;
      sql?: string;
      savedQuery?: DbSavedQuery;
    } = {},
  ) => {
    const info = connected[connectionId];
    const connection = connectionById.get(connectionId);
    if (!connection) return;
    if (options.savedQuery) {
      openTab({
        key: `saved:${options.savedQuery.id}`,
        kind: "query",
        title: options.savedQuery.name,
        connectionId,
        database:
          options.savedQuery.database_name ?? info?.default_database ?? "main",
        savedQuery: options.savedQuery,
      });
      return;
    }
    queryCounter.current += 1;
    openTab({
      key: `query:${queryCounter.current}`,
      kind: "query",
      title: t("查询 {0}", queryCounter.current),
      connectionId,
      database:
        options.database ??
        info?.default_database ??
        connection.database_name ??
        (connection.db_type === "sqlite" ? "main" : ""),
      schema: options.schema,
      sql: options.sql,
    });
  };

  const openObject = (
    ref: Extract<NodeRef, ["o", ...unknown[]]>,
    mode: "data" | "structure",
  ) => {
    const [, id, database, schema, kind, name, identity] = ref;
    if (kind === "function" || kind === "procedure") {
      openTab({
        key: `routine:${JSON.stringify([id, database, schema, name, identity])}`,
        kind: "routine",
        title: name,
        connectionId: id,
        database,
        schema: schema ?? undefined,
        name,
        objectKind: kind,
        identity,
      });
      return;
    }
    const tabMode = kind === "sequence" ? "structure" : mode;
    openTab({
      key: `${tabMode}:${JSON.stringify([id, database, schema, name])}`,
      kind: tabMode,
      title: tabMode === "structure" ? t("{0}（结构）", name) : name,
      connectionId: id,
      database,
      schema: schema ?? undefined,
      table: name,
      objectKind: kind,
    });
  };

  const closeTab = (key: string) => {
    setTabs((current) => {
      const index = current.findIndex((tab) => tab.key === key);
      const next = current.filter((tab) => tab.key !== key);
      if (activeTab === key) {
        setActiveTab(next[Math.max(0, index - 1)]?.key);
      }
      return next;
    });
  };

  // Tabs of deleted connections go away.
  useEffect(() => {
    if (!connectionsQuery.data) return;
    setTabs((current) =>
      current.filter((tab) => connectionById.has(tab.connectionId)),
    );
  }, [connectionsQuery.data, connectionById]);

  // ---------------------------------------------------------------------
  // Actions
  // ---------------------------------------------------------------------

  const runStatement = (connectionId: number, database: string, sql: string) =>
    executeDbSql(connectionId, {
      database,
      session_key: newSessionKey(),
      sql,
    }).then(async (result) => {
      const error = result.statements.find(
        (statement) => statement.error,
      )?.error;
      if (error) throw new Error(error);
      return result;
    });

  const confirmDanger = (
    title: string,
    sql: string,
    onOk: () => Promise<unknown>,
  ) => {
    Modal.confirm({
      title,
      content: <code className="db-confirm-sql">{sql}</code>,
      okText: t("执行"),
      okButtonProps: { danger: true },
      onOk: () =>
        onOk().catch((error: Error) => {
          message.error(error.message);
          throw error;
        }),
    });
  };

  const exportObject = (
    ref: Extract<NodeRef, ["o", ...unknown[]]>,
    format: DbExportFormat,
  ) => {
    const [, id, database, schema, , name] = ref;
    const hide = message.loading(t("正在导出…"), 0);
    exportDbRows(
      id,
      { database, schema: schema ?? undefined, table: name },
      format,
    )
      .then((count) => message.success(t("已导出 {0} 行", count)))
      .catch((error: Error) => message.error(error.message || t("导出失败")))
      .finally(hide);
  };

  const deleteConnection = (connection: DbConnection) => {
    Modal.confirm({
      title: t("删除连接“{0}”？", connection.name),
      content: t(
        "只删除保存的连接信息和查询，不会影响数据库本身。可在回收站恢复。",
      ),
      okText: t("删除"),
      okButtonProps: { danger: true },
      onOk: async () => {
        await deleteDbConnection(connection.id);
        disconnect(connection.id);
        await queryClient.invalidateQueries({ queryKey: ["db-connections"] });
        message.success(t("连接已删除"));
      },
    });
  };

  const menuFor = (ref: NodeRef): MenuProps["items"] => {
    const key = nodeKey(ref);
    switch (ref[0]) {
      case "c": {
        const connection = connectionById.get(ref[1]);
        if (!connection) return [];
        const isOpen = Boolean(connected[ref[1]]);
        return [
          isOpen
            ? {
                key: "close",
                icon: <DisconnectOutlined />,
                label: t("关闭连接"),
                onClick: () => disconnect(ref[1]),
              }
            : {
                key: "open",
                icon: <ApiOutlined />,
                label: t("打开连接"),
                onClick: () => expandAndLoad(key),
              },
          {
            key: "query",
            icon: <ConsoleSqlOutlined />,
            label: t("新建查询"),
            disabled: !canQuery,
            onClick: () => openQuery(ref[1]),
          },
          {
            key: "refresh",
            icon: <ReloadOutlined />,
            label: t("刷新"),
            disabled: !isOpen,
            onClick: () => refreshNode(key),
          },
          { type: "divider" },
          {
            key: "edit",
            label: t("编辑连接"),
            disabled: !canManage,
            onClick: () => setModal({ open: true, editing: connection }),
          },
          {
            key: "delete",
            danger: true,
            label: t("删除连接"),
            disabled: !canManage,
            onClick: () => deleteConnection(connection),
          },
        ];
      }
      case "d":
      case "s":
        return [
          {
            key: "query",
            icon: <ConsoleSqlOutlined />,
            label: t("新建查询"),
            disabled: !canQuery,
            onClick: () =>
              openQuery(ref[1], {
                database: ref[2],
                schema: ref[0] === "s" ? ref[3] : undefined,
              }),
          },
          {
            key: "refresh",
            icon: <ReloadOutlined />,
            label: t("刷新"),
            onClick: () => refreshNode(key),
          },
          {
            key: "copy",
            label: t("复制名称"),
            onClick: () => copyText(ref[0] === "s" ? ref[3] : ref[2]),
          },
        ];
      case "g":
        return [
          {
            key: "refresh",
            icon: <ReloadOutlined />,
            label: t("刷新"),
            onClick: () => refreshNode(key),
          },
        ];
      case "o": {
        const [, id, database, schema, kind, name] = ref;
        const dbType = connected[id]?.db_type ?? "postgres";
        const parentGroup: NodeRef = [
          "g",
          id,
          database,
          schema,
          kind === "view" || kind === "materialized_view" ? "views" : "tables",
        ];
        if (kind === "function" || kind === "procedure") {
          return [
            {
              key: "definition",
              icon: <CodeOutlined />,
              label: t("查看定义"),
              onClick: () => openObject(ref, "structure"),
            },
            {
              key: "copy",
              label: t("复制名称"),
              onClick: () => copyText(name),
            },
          ];
        }
        if (kind === "sequence") {
          return [
            {
              key: "definition",
              icon: <CodeOutlined />,
              label: t("查看定义"),
              onClick: () => openObject(ref, "structure"),
            },
            {
              key: "copy",
              label: t("复制名称"),
              onClick: () => copyText(name),
            },
          ];
        }
        const writable = canQuery && !connected[id]?.read_only;
        return [
          {
            key: "open",
            icon: <TableOutlined />,
            label: kind === "table" ? t("打开表") : t("打开视图"),
            onClick: () => openObject(ref, "data"),
          },
          {
            key: "structure",
            icon: <PartitionOutlined />,
            label: t("查看结构"),
            onClick: () => openObject(ref, "structure"),
          },
          {
            key: "query",
            icon: <ConsoleSqlOutlined />,
            label: t("新建查询"),
            disabled: !canQuery,
            onClick: () =>
              openQuery(id, {
                database,
                schema: schema ?? undefined,
                sql: selectTemplate(
                  dbType,
                  name,
                  database,
                  schema ?? undefined,
                ),
              }),
          },
          { key: "copy", label: t("复制名称"), onClick: () => copyText(name) },
          {
            key: "export",
            label: t("导出"),
            children: [
              {
                key: "csv",
                label: "CSV",
                onClick: () => exportObject(ref, "csv"),
              },
              {
                key: "json",
                label: "JSON",
                onClick: () => exportObject(ref, "json"),
              },
              {
                key: "sql",
                label: t("SQL（INSERT 语句）"),
                onClick: () => exportObject(ref, "sql"),
              },
            ],
          },
          { type: "divider" },
          ...(kind === "table"
            ? [
                {
                  key: "truncate",
                  danger: true,
                  label: t("清空表"),
                  disabled: !writable,
                  onClick: () => {
                    const sql = truncateStatement(
                      dbType,
                      name,
                      database,
                      schema ?? undefined,
                    );
                    confirmDanger(
                      t("清空表“{0}”的全部数据？", name),
                      sql,
                      async () => {
                        await runStatement(id, database, sql);
                        message.success(t("表已清空"));
                        void queryClient.invalidateQueries({
                          queryKey: ["db-rows", id],
                        });
                      },
                    );
                  },
                },
              ]
            : []),
          {
            key: "drop",
            danger: true,
            label: kind === "table" ? t("删除表") : t("删除视图"),
            disabled: !writable,
            onClick: () => {
              const sql = dropStatement(
                dbType,
                kind,
                name,
                database,
                schema ?? undefined,
              );
              confirmDanger(
                t("删除“{0}”？此操作不能撤销。", name),
                sql,
                async () => {
                  await runStatement(id, database, sql);
                  message.success(t("已删除"));
                  setTabs((current) =>
                    current.filter(
                      (tab) =>
                        !(
                          tab.connectionId === id &&
                          "table" in tab &&
                          tab.table === name &&
                          tab.database === database
                        ),
                    ),
                  );
                  refreshNode(nodeKey(parentGroup));
                },
              );
            },
          },
        ];
      }
      case "qg":
        return [
          {
            key: "query",
            icon: <ConsoleSqlOutlined />,
            label: t("新建查询"),
            disabled: !canQuery,
            onClick: () => openQuery(ref[1]),
          },
        ];
      case "q": {
        const saved = savedQueriesQuery.data?.find(
          (item) => item.id === ref[2],
        );
        if (!saved) return [];
        return [
          {
            key: "open",
            label: t("打开查询"),
            onClick: () => openQuery(ref[1], { savedQuery: saved }),
          },
          {
            key: "delete",
            danger: true,
            label: t("删除查询"),
            disabled: !canQuery,
            onClick: () =>
              Modal.confirm({
                title: t("删除查询“{0}”？", saved.name),
                okText: t("删除"),
                okButtonProps: { danger: true },
                onOk: async () => {
                  await deleteDbSavedQuery(saved.id);
                  closeTab(`saved:${saved.id}`);
                  await savedQueriesQuery.refetch();
                },
              }),
          },
        ];
      }
      default:
        return [];
    }
  };

  // ---------------------------------------------------------------------
  // Tree data
  // ---------------------------------------------------------------------

  const keyword = filter.trim().toLowerCase();

  const treeData = useMemo<DataNode[]>(() => {
    const objectGroups = (
      id: number,
      database: string,
      schema: string | null,
    ): DataNode[] | undefined => {
      const list = objects[objectsKey(id, database, schema)];
      if (!list) return undefined;
      return (Object.keys(GROUP_KINDS) as ObjectGroup[]).flatMap((group) => {
        const items = list.filter(
          (item) =>
            GROUP_KINDS[group].includes(item.kind) &&
            (!keyword || item.name.toLowerCase().includes(keyword)),
        );
        if (!items.length && (group === "functions" || group === "sequences")) {
          return [];
        }
        return [
          {
            key: nodeKey(["g", id, database, schema, group]),
            title: `${groupLabel(group)} (${items.length})`,
            icon: <FolderOutlined />,
            children: items.map((item) => ({
              key: nodeKey([
                "o",
                id,
                database,
                schema,
                item.kind,
                item.name,
                item.identity ?? null,
              ]),
              title: item.name,
              icon: objectIcon(item.kind),
              isLeaf: true,
              data: item,
            })),
          },
        ];
      });
    };

    const savedQueries = savedQueriesQuery.data ?? [];
    const connectionNode = (connection: DbConnection): DataNode => {
      const info = connected[connection.id];
      const queries = savedQueries.filter(
        (query) => query.connection_id === connection.id,
      );
      let children: DataNode[] | undefined;
      if (info) {
        const queryGroup: DataNode = {
          key: nodeKey(["qg", connection.id]),
          title: `${t("查询")} (${queries.length})`,
          icon: <FolderOpenOutlined />,
          children: queries.map((query) => ({
            key: nodeKey(["q", connection.id, query.id]),
            title: query.name,
            icon: <FileTextOutlined />,
            isLeaf: true,
          })),
        };
        if (info.db_type === "sqlite") {
          children = [
            ...(objectGroups(connection.id, "main", null) ?? []),
            queryGroup,
          ];
        } else {
          children = [
            ...info.databases.map((database) => {
              const dbKey = nodeKey(["d", connection.id, database]);
              const schemaList = schemas[objectsKey(connection.id, database)];
              return {
                key: dbKey,
                title: database,
                icon: <DatabaseOutlined />,
                children: info.has_schemas
                  ? schemaList?.map((schema) => ({
                      key: nodeKey(["s", connection.id, database, schema]),
                      title: schema,
                      icon: <PartitionOutlined />,
                      children: objectGroups(connection.id, database, schema),
                    }))
                  : objectGroups(connection.id, database, null),
              } satisfies DataNode;
            }),
            queryGroup,
          ];
        }
      }
      return {
        key: nodeKey(["c", connection.id]),
        title: connection.name,
        icon: (
          <span className="db-connection-icon">
            <DatabaseOutlined className={info ? "is-connected" : undefined} />
            {connection.color ? (
              <span
                className="db-color-mark"
                style={{ background: connection.color }}
              />
            ) : null}
          </span>
        ),
        children,
      };
    };

    const grouped = new Map<string, DbConnection[]>();
    const loose: DbConnection[] = [];
    for (const connection of connections) {
      if (connection.group_name) {
        grouped.set(connection.group_name, [
          ...(grouped.get(connection.group_name) ?? []),
          connection,
        ]);
      } else {
        loose.push(connection);
      }
    }
    return [
      ...[...grouped.entries()].map(([group, items]) => ({
        key: nodeKey(["folder", group]),
        title: group,
        icon: <FolderOutlined />,
        children: items.map(connectionNode),
      })),
      ...loose.map(connectionNode),
    ];
  }, [
    connections,
    connected,
    schemas,
    objects,
    savedQueriesQuery.data,
    keyword,
  ]);

  // Group folders are always loaded.
  const allLoadedKeys = useMemo(
    () => [...loadedKeys, ...groups.map((group) => nodeKey(["folder", group]))],
    [loadedKeys, groups],
  );

  const titleFor = (node: DataNode) => {
    const ref = parseKey(node.key);
    let title: ReactNode = node.title as ReactNode;
    if (ref[0] === "c") {
      const connection = connectionById.get(ref[1]);
      title = (
        <span className="db-tree-connection">
          <span>{connection?.name}</span>
          <Tag className="db-type-tag" bordered={false}>
            {connection ? DB_TYPE_LABELS[connection.db_type] : ""}
          </Tag>
        </span>
      );
    } else if (ref[0] === "o") {
      const item = (node as DataNode & { data?: DbObject }).data;
      title = (
        <Tooltip
          placement="right"
          mouseEnterDelay={0.6}
          title={
            item ? (
              <div>
                <div>{objectKindLabel(item.kind)}</div>
                {item.rows !== null && item.rows !== undefined ? (
                  <div>{t("约 {0} 行", item.rows)}</div>
                ) : null}
                {item.size_bytes ? (
                  <div>{formatBytes(item.size_bytes)}</div>
                ) : null}
                {item.comment ? <div>{item.comment}</div> : null}
              </div>
            ) : null
          }
        >
          <span>{ref[5]}</span>
        </Tooltip>
      );
    }
    const items = ref[0] === "folder" ? [] : menuFor(ref);
    return items?.length ? (
      <Dropdown trigger={["contextMenu"]} menu={{ items }}>
        <span className="db-tree-title">{title}</span>
      </Dropdown>
    ) : (
      <span className="db-tree-title">{title}</span>
    );
  };

  const onDoubleClick = (key: string) => {
    const ref = parseKey(key);
    if (ref[0] === "o") {
      openObject(ref, isTableLike(ref[4]) ? "data" : "structure");
    } else if (ref[0] === "q") {
      const saved = savedQueriesQuery.data?.find((item) => item.id === ref[2]);
      if (saved) openQuery(ref[1], { savedQuery: saved });
    }
  };

  /** Connection of the selected node or the active tab, for 新建查询. */
  const currentConnection = () => {
    if (selectedKey) {
      const ref = parseKey(selectedKey);
      if (ref[0] !== "folder") {
        return {
          id: ref[1],
          database:
            ref[0] === "c" || ref[0] === "qg" || ref[0] === "q"
              ? undefined
              : ref[2],
          schema:
            ref[0] === "s"
              ? ref[3]
              : ref[0] === "g" || ref[0] === "o"
                ? (ref[3] ?? undefined)
                : undefined,
        };
      }
    }
    const tab = tabs.find((item) => item.key === activeTab);
    return tab
      ? { id: tab.connectionId, database: tab.database, schema: tab.schema }
      : null;
  };

  const renderTab = (tab: WorkTab) => {
    const connection = connectionById.get(tab.connectionId);
    const info = connected[tab.connectionId];
    const dbType = connection?.db_type ?? "postgres";
    const openQueryHere = (sql: string) =>
      openQuery(tab.connectionId, {
        database: tab.database,
        schema: tab.schema,
        sql,
      });
    switch (tab.kind) {
      case "data":
        return (
          <TableDataView
            connectionId={tab.connectionId}
            dbType={dbType}
            database={tab.database}
            schema={tab.schema}
            table={tab.table}
            objectKind={tab.objectKind}
            canQuery={canQuery}
            onOpenQuery={openQueryHere}
          />
        );
      case "structure":
        return (
          <StructureView
            connectionId={tab.connectionId}
            dbType={dbType}
            database={tab.database}
            schema={tab.schema}
            table={tab.table}
            onOpenQuery={openQueryHere}
          />
        );
      case "routine":
        return (
          <RoutineView tab={tab} dbType={dbType} onOpenQuery={openQueryHere} />
        );
      case "query":
        return (
          <QueryEditor
            connectionId={tab.connectionId}
            dbType={dbType}
            databases={info?.databases ?? (tab.database ? [tab.database] : [])}
            initialDatabase={tab.database}
            schema={tab.schema}
            initialSql={tab.sql}
            savedQuery={tab.savedQuery}
            readOnly={connection?.read_only ?? false}
            onTitleChange={(title) =>
              setTabs((current) =>
                current.map((item) =>
                  item.key === tab.key ? { ...item, title } : item,
                ),
              )
            }
            onSaved={() => void savedQueriesQuery.refetch()}
          />
        );
    }
  };

  const tabIcon = (tab: WorkTab) =>
    tab.kind === "query" ? (
      <ConsoleSqlOutlined />
    ) : tab.kind === "structure" ? (
      <PartitionOutlined />
    ) : tab.kind === "routine" ? (
      <FunctionOutlined />
    ) : (
      objectIcon(tab.objectKind)
    );

  return (
    <div ref={pageRef} className="db-page">
      <h1 className="visually-hidden">{t("数据库管理")}</h1>
      <aside className="db-sidebar">
        <div className="db-sidebar-toolbar">
          {canManage ? (
            <Tooltip title={t("新建连接")}>
              <Button
                size="small"
                type="primary"
                icon={<PlusOutlined />}
                onClick={() => setModal({ open: true, editing: null })}
              >
                {t("连接")}
              </Button>
            </Tooltip>
          ) : null}
          {canQuery ? (
            <Tooltip title={t("新建查询")}>
              <Button
                size="small"
                icon={<ConsoleSqlOutlined />}
                onClick={() => {
                  const current = currentConnection();
                  if (!current) {
                    message.info(t("请先在左侧选择一个连接"));
                    return;
                  }
                  openQuery(current.id, {
                    database: current.database,
                    schema: current.schema,
                  });
                }}
              >
                {t("查询")}
              </Button>
            </Tooltip>
          ) : null}
          <Tooltip title={t("刷新")}>
            <Button
              size="small"
              icon={<ReloadOutlined />}
              onClick={() => {
                if (selectedKey && parseKey(selectedKey)[0] !== "folder") {
                  refreshNode(selectedKey);
                } else {
                  void connectionsQuery.refetch();
                  void savedQueriesQuery.refetch();
                }
              }}
            />
          </Tooltip>
        </div>
        <Input.Search
          size="small"
          allowClear
          className="db-sidebar-search"
          placeholder={t("筛选已加载的表和视图")}
          onChange={(event) => setFilter(event.target.value)}
        />
        <div className="db-tree">
          {connectionsQuery.isLoading ? (
            <Spin className="db-center-spin" />
          ) : connections.length ? (
            <Tree
              showIcon
              blockNode
              treeData={treeData}
              expandedKeys={expandedKeys}
              loadedKeys={allLoadedKeys}
              selectedKeys={selectedKey ? [selectedKey] : []}
              onExpand={(keys) => setExpandedKeys(keys)}
              onSelect={(keys) =>
                setSelectedKey(keys[0] ? String(keys[0]) : null)
              }
              loadData={(node) => loadNode(String(node.key))}
              onDoubleClick={(_, node) => onDoubleClick(String(node.key))}
              titleRender={titleFor}
            />
          ) : (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t("还没有数据库连接")}
            >
              {canManage ? (
                <Button
                  type="primary"
                  icon={<PlusOutlined />}
                  onClick={() => setModal({ open: true, editing: null })}
                >
                  {t("新建连接")}
                </Button>
              ) : null}
            </Empty>
          )}
        </div>
      </aside>
      <main className="db-workspace">
        {tabs.length ? (
          <Tabs
            type="editable-card"
            hideAdd
            size="small"
            className="db-tabs"
            activeKey={activeTab}
            onChange={setActiveTab}
            onEdit={(key, action) => {
              if (action === "remove") closeTab(String(key));
            }}
            items={tabs.map((tab) => {
              const connection = connectionById.get(tab.connectionId);
              return {
                key: tab.key,
                label: (
                  <Tooltip
                    mouseEnterDelay={0.6}
                    title={[connection?.name, tab.database, tab.schema]
                      .filter(Boolean)
                      .join(" / ")}
                  >
                    <span className="db-tab-label">
                      {tabIcon(tab)}
                      {tab.title}
                    </span>
                  </Tooltip>
                ),
                children: <div className="db-tab-body">{renderTab(tab)}</div>,
              };
            })}
          />
        ) : (
          <div className="db-welcome">
            <DatabaseOutlined className="db-welcome-icon" />
            <Typography.Title level={4}>{t("数据库管理")}</Typography.Title>
            <Typography.Paragraph type="secondary">
              {t(
                "在左侧打开连接，双击表查看和编辑数据，右键表或数据库获取更多操作；SQL 编辑器中按 Ctrl+Enter 执行当前语句。",
              )}
            </Typography.Paragraph>
            <div className="db-welcome-list">
              {connections.slice(0, 8).map((connection) => (
                <button
                  type="button"
                  key={connection.id}
                  className="db-welcome-item"
                  onClick={() => {
                    const key = nodeKey(["c", connection.id]);
                    setSelectedKey(key);
                    expandAndLoad(key);
                  }}
                >
                  <DatabaseOutlined />
                  <span className="db-welcome-name">{connection.name}</span>
                  <Typography.Text type="secondary">
                    {connection.db_type === "sqlite"
                      ? connection.database_name
                      : `${connection.host ?? ""}:${connection.port ?? ""}`}
                  </Typography.Text>
                </button>
              ))}
            </div>
          </div>
        )}
      </main>
      <ConnectionModal
        open={modal.open}
        editing={modal.editing}
        groups={groups}
        onClose={() => setModal({ open: false })}
        onSaved={(connection) => {
          setModal({ open: false });
          if (connected[connection.id]) disconnect(connection.id);
          void queryClient.invalidateQueries({ queryKey: ["db-connections"] });
        }}
      />
    </div>
  );
}
