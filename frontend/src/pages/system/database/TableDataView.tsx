import {
  CodeOutlined,
  DeleteOutlined,
  DownOutlined,
  ExportOutlined,
  FilterOutlined,
  KeyOutlined,
  MinusCircleOutlined,
  PlusOutlined,
  ReloadOutlined,
  SaveOutlined,
  UndoOutlined,
} from "@ant-design/icons";
import { keepPreviousData, useMutation, useQuery } from "@tanstack/react-query";
import {
  Alert,
  Badge,
  Button,
  Dropdown,
  Empty,
  Input,
  Modal,
  Pagination,
  Select,
  Space,
  Table,
  Tooltip,
  Typography,
  message,
} from "antd";
import type { ColumnsType, TableProps } from "antd/es/table";
import {
  type MouseEvent as ReactMouseEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  type DbCell,
  type DbColumn,
  type DbExportFormat,
  type DbFilter,
  type DbFilterOp,
  type DbObjectKind,
  type DbRowChange,
  type DbSort,
  type DbType,
  applyDbChanges,
  exportDbRows,
  fetchDbRows,
} from "../../../api/admin/dbManager";
import { t } from "../../../i18n";
import { CellText, columnWidth } from "./ResultGrid";
import {
  deleteTemplate,
  formatCell,
  insertTemplate,
  selectTemplate,
  updateTemplate,
} from "./dbUtils";

type GridRow = {
  key: string;
  isNew: boolean;
  /** Original values (existing rows). */
  cells: DbCell[];
};

type EditingCell = { rowKey: string; column: string };
type ContextMenuState = {
  x: number;
  y: number;
  rowKey: string;
  column: string;
};

const FILTER_OPS: { value: DbFilterOp; label: string; noValue?: boolean }[] = [
  { value: "eq", label: "=" },
  { value: "ne", label: "≠" },
  { value: "gt", label: ">" },
  { value: "gte", label: "≥" },
  { value: "lt", label: "<" },
  { value: "lte", label: "≤" },
  { value: "contains", label: t("包含") },
  { value: "not_contains", label: t("不包含") },
  { value: "starts_with", label: t("开头是") },
  { value: "ends_with", label: t("结尾是") },
  { value: "is_null", label: t("为 NULL"), noValue: true },
  { value: "not_null", label: t("不为 NULL"), noValue: true },
  { value: "is_empty", label: t("为空"), noValue: true },
  { value: "not_empty", label: t("不为空"), noValue: true },
];

const PAGE_SIZES = [50, 100, 200, 500, 1000];

function sameValue(original: DbCell, next: DbCell) {
  if (original === null || next === null) return original === next;
  return formatCell(original) === formatCell(next);
}

export function TableDataView({
  connectionId,
  dbType,
  database,
  schema,
  table,
  objectKind,
  canQuery,
  onOpenQuery,
}: {
  connectionId: number;
  dbType: DbType;
  database: string;
  schema?: string;
  table: string;
  objectKind: DbObjectKind;
  canQuery: boolean;
  onOpenQuery: (sql: string) => void;
}) {
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(100);
  const [sort, setSort] = useState<DbSort[]>([]);
  const [filters, setFilters] = useState<DbFilter[]>([]);
  const [where, setWhere] = useState("");
  const [draftFilters, setDraftFilters] = useState<DbFilter[]>([]);
  const [draftWhere, setDraftWhere] = useState("");
  const [showFilters, setShowFilters] = useState(false);
  const [edits, setEdits] = useState<Record<string, Record<string, DbCell>>>(
    {},
  );
  const [inserted, setInserted] = useState<
    { key: string; values: Record<string, DbCell> }[]
  >([]);
  const [deleted, setDeleted] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<string[]>([]);
  const [editing, setEditing] = useState<EditingCell | null>(null);
  const [editingText, setEditingText] = useState("");
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const [gridHeight, setGridHeight] = useState(360);
  const newRowCounter = useRef(0);

  const rowsQuery = useQuery({
    queryKey: [
      "db-rows",
      connectionId,
      database,
      schema,
      table,
      page,
      pageSize,
      sort,
      filters,
      where,
    ],
    queryFn: () =>
      fetchDbRows(connectionId, {
        database,
        schema,
        table,
        page,
        page_size: pageSize,
        sort,
        filters,
        where,
      }),
    placeholderData: keepPreviousData,
    retry: false,
  });
  const data = rowsQuery.data;
  const columns = data?.columns ?? [];
  const primaryKey = data?.primary_key ?? [];
  const writable = Boolean(
    data && !data.read_only && canQuery && objectKind === "table",
  );
  const editable = writable && primaryKey.length > 0;
  const pending = Object.keys(edits).length + inserted.length + deleted.size;

  useEffect(() => {
    const host = gridRef.current;
    if (!host) return;
    const observer = new ResizeObserver(() =>
      setGridHeight(Math.max(160, host.clientHeight - 42)),
    );
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("scroll", close, true);
    };
  }, [contextMenu]);

  const columnIndex = useMemo(
    () => new Map(columns.map((column, index) => [column.name, index])),
    [columns],
  );

  const gridRows = useMemo<GridRow[]>(
    () => [
      ...inserted.map((row) => ({ key: row.key, isNew: true, cells: [] })),
      ...(data?.rows ?? []).map((cells, index) => ({
        key: primaryKey.length
          ? JSON.stringify(
              primaryKey.map((name) => cells[columnIndex.get(name) ?? 0]),
            )
          : `#${page}:${index}`,
        isNew: false,
        cells,
      })),
    ],
    [inserted, data?.rows, primaryKey, columnIndex, page],
  );
  const rowsByKey = useMemo(
    () => new Map(gridRows.map((row) => [row.key, row])),
    [gridRows],
  );

  const resetChanges = () => {
    setEdits({});
    setInserted([]);
    setDeleted(new Set());
    setSelected([]);
    setEditing(null);
  };

  /** Runs `action` now, or after confirming that unsaved edits are dropped. */
  const guardChanges = (action: () => void) => {
    if (!pending) {
      action();
      return;
    }
    Modal.confirm({
      title: t("放弃未保存的修改？"),
      content: t("有 {0} 处修改尚未保存。", pending),
      okText: t("放弃修改"),
      okButtonProps: { danger: true },
      onOk: () => {
        resetChanges();
        action();
      },
    });
  };

  const currentValue = (row: GridRow, column: string): DbCell | undefined => {
    if (row.isNew) {
      const values = inserted.find((item) => item.key === row.key)?.values;
      return values && column in values ? values[column] : undefined;
    }
    const rowEdits = edits[row.key];
    if (rowEdits && column in rowEdits) return rowEdits[column];
    return row.cells[columnIndex.get(column) ?? 0];
  };

  const setCell = (rowKey: string, column: string, value: DbCell) => {
    const row = rowsByKey.get(rowKey);
    if (!row) return;
    if (row.isNew) {
      setInserted((items) =>
        items.map((item) =>
          item.key === rowKey
            ? { ...item, values: { ...item.values, [column]: value } }
            : item,
        ),
      );
      return;
    }
    const original = row.cells[columnIndex.get(column) ?? 0];
    setEdits((current) => {
      const rowEdits = { ...(current[rowKey] ?? {}) };
      if (sameValue(original, value)) {
        delete rowEdits[column];
      } else {
        rowEdits[column] = value;
      }
      const next = { ...current };
      if (Object.keys(rowEdits).length) {
        next[rowKey] = rowEdits;
      } else {
        delete next[rowKey];
      }
      return next;
    });
  };

  const canEditCell = (row: GridRow, column: DbColumn) =>
    !column.binary &&
    (row.isNew ? writable : editable && !deleted.has(row.key));

  const startEdit = (row: GridRow, column: DbColumn) => {
    if (!canEditCell(row, column)) return;
    const value = currentValue(row, column.name);
    setEditing({ rowKey: row.key, column: column.name });
    setEditingText(
      value === null || value === undefined ? "" : formatCell(value),
    );
  };

  const commitEdit = (value: DbCell = editingText) => {
    if (!editing) return;
    setCell(editing.rowKey, editing.column, value);
    setEditing(null);
  };

  const addRow = () => {
    newRowCounter.current += 1;
    setInserted((items) => [
      { key: `new:${newRowCounter.current}`, values: {} },
      ...items,
    ]);
  };

  const removeRows = (keys: string[]) => {
    const newKeys = new Set(keys.filter((key) => key.startsWith("new:")));
    if (newKeys.size) {
      setInserted((items) => items.filter((item) => !newKeys.has(item.key)));
    }
    const existing = keys.filter((key) => !newKeys.has(key));
    if (existing.length) {
      setDeleted((current) => {
        const next = new Set(current);
        for (const key of existing) next.add(key);
        return next;
      });
    }
    setSelected([]);
  };

  const keyValues = (row: GridRow) =>
    Object.fromEntries(
      primaryKey.map((name) => [name, row.cells[columnIndex.get(name) ?? 0]]),
    );

  const save = useMutation({
    mutationFn: () => {
      const changes: DbRowChange[] = [
        ...inserted.map((row) => ({
          kind: "insert" as const,
          values: row.values,
        })),
        ...Object.entries(edits)
          .filter(([key]) => !deleted.has(key))
          .flatMap(([key, values]) => {
            const row = rowsByKey.get(key);
            return row
              ? [{ kind: "update" as const, key: keyValues(row), values }]
              : [];
          }),
        ...[...deleted].flatMap((key) => {
          const row = rowsByKey.get(key);
          return row ? [{ kind: "delete" as const, key: keyValues(row) }] : [];
        }),
      ];
      return applyDbChanges(connectionId, { database, schema, table, changes });
    },
    onSuccess: (summary) => {
      message.success(
        t(
          "已保存：新增 {0} 行，修改 {1} 行，删除 {2} 行",
          summary.inserted,
          summary.updated,
          summary.deleted,
        ),
      );
      resetChanges();
      void rowsQuery.refetch();
    },
    onError: (error) => message.error(error.message),
  });

  const exportRows = (format: DbExportFormat) => {
    const hide = message.loading(t("正在导出…"), 0);
    exportDbRows(
      connectionId,
      { database, schema, table, sort, filters, where },
      format,
    )
      .then((count) => message.success(t("已导出 {0} 行", count)))
      .catch((error: Error) => message.error(error.message || t("导出失败")))
      .finally(hide);
  };

  const applyFilters = () => {
    guardChanges(() => {
      setFilters(
        draftFilters.filter(
          (filter) =>
            filter.column &&
            (FILTER_OPS.find((op) => op.value === filter.op)?.noValue ||
              filter.value !== undefined),
        ),
      );
      setWhere(draftWhere);
      setPage(1);
    });
  };

  const tableColumns: ColumnsType<GridRow> = [
    ...columns.map((column) => {
      const sortEntry = sort.find((item) => item.column === column.name);
      return {
        key: column.name,
        dataIndex: column.name,
        width: columnWidth(column.name, column.data_type),
        ellipsis: true,
        sorter: true,
        sortOrder: sortEntry
          ? sortEntry.desc
            ? ("descend" as const)
            : ("ascend" as const)
          : null,
        title: (
          <Tooltip
            title={
              <div>
                <div>{column.data_type}</div>
                {column.comment ? <div>{column.comment}</div> : null}
              </div>
            }
          >
            <span className="db-grid-header">
              {column.primary_key ? (
                <KeyOutlined className="db-pk-icon" />
              ) : null}
              <span>{column.name}</span>
            </span>
          </Tooltip>
        ),
        onCell: (row: GridRow) => {
          const changed =
            !row.isNew &&
            Boolean(edits[row.key] && column.name in edits[row.key]);
          return {
            className: changed ? "db-cell-changed" : undefined,
            onDoubleClick: () => startEdit(row, column),
            onContextMenu: (event: ReactMouseEvent) => {
              event.preventDefault();
              setContextMenu({
                x: event.clientX,
                y: event.clientY,
                rowKey: row.key,
                column: column.name,
              });
            },
          };
        },
        render: (_: unknown, row: GridRow) => {
          if (editing?.rowKey === row.key && editing.column === column.name) {
            return (
              <Input
                size="small"
                autoFocus
                value={editingText}
                onChange={(event) => setEditingText(event.target.value)}
                onPressEnter={() => commitEdit()}
                onBlur={() => commitEdit()}
                onKeyDown={(event) => {
                  if (event.key === "Escape") setEditing(null);
                }}
                suffix={
                  <Tooltip title={t("设为 NULL")}>
                    <Button
                      size="small"
                      type="text"
                      className="db-null-button"
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => commitEdit(null)}
                    >
                      NULL
                    </Button>
                  </Tooltip>
                }
              />
            );
          }
          const value = currentValue(row, column.name);
          if (value === undefined) {
            return (
              <span className="db-cell-default">
                {column.auto_increment ? t("(自动)") : t("(默认)")}
              </span>
            );
          }
          return <CellText value={value} />;
        },
      };
    }),
  ];

  const handleTableChange: TableProps<GridRow>["onChange"] = (
    _pagination,
    _filters,
    sorter,
  ) => {
    const entry = Array.isArray(sorter) ? sorter[0] : sorter;
    const next: DbSort[] =
      entry?.order && entry.columnKey
        ? [{ column: String(entry.columnKey), desc: entry.order === "descend" }]
        : [];
    guardChanges(() => {
      setSort(next);
      setPage(1);
    });
  };

  const menuRow = contextMenu ? rowsByKey.get(contextMenu.rowKey) : undefined;
  const menuColumn = contextMenu
    ? columns.find((column) => column.name === contextMenu.column)
    : undefined;

  const sqlTemplates = [
    {
      key: "select",
      label: "SELECT",
      onClick: () =>
        onOpenQuery(selectTemplate(dbType, table, database, schema)),
    },
    {
      key: "insert",
      label: "INSERT",
      onClick: () =>
        onOpenQuery(insertTemplate(dbType, table, columns, database, schema)),
    },
    {
      key: "update",
      label: "UPDATE",
      onClick: () =>
        onOpenQuery(updateTemplate(dbType, table, columns, database, schema)),
    },
    {
      key: "delete",
      label: "DELETE",
      onClick: () =>
        onOpenQuery(deleteTemplate(dbType, table, columns, database, schema)),
    },
  ];

  return (
    <div className="db-data-view">
      <div className="db-toolbar">
        <Space wrap size={6}>
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={rowsQuery.isFetching}
            onClick={() => guardChanges(() => void rowsQuery.refetch())}
          >
            {t("刷新")}
          </Button>
          <Badge count={filters.length + (where.trim() ? 1 : 0)} size="small">
            <Button
              size="small"
              type={showFilters ? "primary" : "default"}
              ghost={showFilters}
              icon={<FilterOutlined />}
              onClick={() => setShowFilters((value) => !value)}
            >
              {t("筛选")}
            </Button>
          </Badge>
          {writable ? (
            <>
              <Button size="small" icon={<PlusOutlined />} onClick={addRow}>
                {t("新增行")}
              </Button>
              <Button
                size="small"
                icon={<DeleteOutlined />}
                disabled={!selected.length}
                onClick={() => removeRows(selected)}
              >
                {t("删除行")}
              </Button>
              <Button
                size="small"
                type="primary"
                icon={<SaveOutlined />}
                disabled={!pending}
                loading={save.isPending}
                onClick={() => save.mutate()}
              >
                {pending ? t("保存（{0}）", pending) : t("保存")}
              </Button>
              <Button
                size="small"
                icon={<UndoOutlined />}
                disabled={!pending}
                onClick={resetChanges}
              >
                {t("放弃修改")}
              </Button>
            </>
          ) : null}
          <Dropdown
            menu={{
              items: [
                { key: "csv", label: "CSV", onClick: () => exportRows("csv") },
                {
                  key: "json",
                  label: "JSON",
                  onClick: () => exportRows("json"),
                },
                {
                  key: "sql",
                  label: t("SQL（INSERT 语句）"),
                  onClick: () => exportRows("sql"),
                },
              ],
            }}
          >
            <Button size="small" icon={<ExportOutlined />}>
              {t("导出")} <DownOutlined />
            </Button>
          </Dropdown>
          {canQuery ? (
            <Dropdown menu={{ items: sqlTemplates }}>
              <Button size="small" icon={<CodeOutlined />}>
                {t("生成 SQL")} <DownOutlined />
              </Button>
            </Dropdown>
          ) : null}
        </Space>
        <Typography.Text type="secondary" className="db-toolbar-info">
          {data ? t("共 {0} 行", data.total) : null}
        </Typography.Text>
      </div>

      {showFilters ? (
        <div className="db-filter-panel">
          {draftFilters.map((filter, index) => {
            const op = FILTER_OPS.find((item) => item.value === filter.op);
            return (
              <Space.Compact
                // biome-ignore lint/suspicious/noArrayIndexKey: filters have no id
                key={index}
                className="db-filter-row"
              >
                <Select
                  size="small"
                  showSearch
                  className="db-filter-column"
                  value={filter.column || undefined}
                  placeholder={t("字段")}
                  options={columns.map((column) => ({
                    value: column.name,
                    label: column.name,
                  }))}
                  onChange={(value) =>
                    setDraftFilters((items) =>
                      items.map((item, itemIndex) =>
                        itemIndex === index ? { ...item, column: value } : item,
                      ),
                    )
                  }
                />
                <Select
                  size="small"
                  className="db-filter-op"
                  value={filter.op}
                  options={FILTER_OPS.map((item) => ({
                    value: item.value,
                    label: item.label,
                  }))}
                  onChange={(value) =>
                    setDraftFilters((items) =>
                      items.map((item, itemIndex) =>
                        itemIndex === index ? { ...item, op: value } : item,
                      ),
                    )
                  }
                />
                {op?.noValue ? null : (
                  <Input
                    size="small"
                    className="db-filter-value"
                    value={filter.value ?? ""}
                    placeholder={t("值")}
                    onChange={(event) =>
                      setDraftFilters((items) =>
                        items.map((item, itemIndex) =>
                          itemIndex === index
                            ? { ...item, value: event.target.value }
                            : item,
                        ),
                      )
                    }
                    onPressEnter={applyFilters}
                  />
                )}
                <Button
                  size="small"
                  icon={<MinusCircleOutlined />}
                  aria-label={t("删除条件")}
                  onClick={() =>
                    setDraftFilters((items) =>
                      items.filter((_, itemIndex) => itemIndex !== index),
                    )
                  }
                />
              </Space.Compact>
            );
          })}
          {canQuery ? (
            <Input
              size="small"
              addonBefore="WHERE"
              className="db-filter-where"
              value={draftWhere}
              placeholder={t(
                "自定义条件，例如 created_at > now() - interval '1 day'",
              )}
              onChange={(event) => setDraftWhere(event.target.value)}
              onPressEnter={applyFilters}
            />
          ) : null}
          <Space size={6}>
            <Button
              size="small"
              icon={<PlusOutlined />}
              onClick={() =>
                setDraftFilters((items) => [
                  ...items,
                  { column: columns[0]?.name ?? "", op: "eq", value: "" },
                ])
              }
            >
              {t("添加条件")}
            </Button>
            <Button size="small" type="primary" onClick={applyFilters}>
              {t("应用")}
            </Button>
            <Button
              size="small"
              onClick={() => {
                setDraftFilters([]);
                setDraftWhere("");
                guardChanges(() => {
                  setFilters([]);
                  setWhere("");
                  setPage(1);
                });
              }}
            >
              {t("清除")}
            </Button>
          </Space>
        </div>
      ) : null}

      {rowsQuery.error ? (
        <Alert
          type="error"
          showIcon
          className="db-inline-alert"
          message={(rowsQuery.error as Error).message}
        />
      ) : null}
      {data && writable && !primaryKey.length ? (
        <Alert
          type="info"
          showIcon
          className="db-inline-alert"
          message={t("该表没有主键：只能新增行，修改和删除请使用 SQL 编辑器")}
        />
      ) : null}
      {data?.read_only ? (
        <Alert
          type="warning"
          showIcon
          className="db-inline-alert"
          message={t("只读连接，不能修改数据")}
        />
      ) : null}

      <div ref={gridRef} className="db-data-grid">
        {columns.length || rowsQuery.isLoading ? (
          <Table<GridRow>
            virtual
            size="small"
            bordered
            rowKey="key"
            loading={rowsQuery.isFetching && !save.isPending}
            columns={tableColumns}
            dataSource={gridRows}
            pagination={false}
            onChange={handleTableChange}
            showSorterTooltip={false}
            scroll={{
              x: tableColumns.reduce(
                (sum, column) => sum + Number(column.width ?? 0),
                writable ? 48 : 0,
              ),
              y: gridHeight,
            }}
            rowClassName={(row) =>
              row.isNew
                ? "db-row-new"
                : deleted.has(row.key)
                  ? "db-row-deleted"
                  : ""
            }
            rowSelection={
              writable
                ? {
                    columnWidth: 48,
                    selectedRowKeys: selected,
                    onChange: (keys) => setSelected(keys.map(String)),
                    getCheckboxProps: (row) => ({
                      disabled: !row.isNew && !editable,
                    }),
                  }
                : undefined
            }
          />
        ) : (
          <Empty description={t("没有数据")} />
        )}
      </div>

      <div className="db-pagination">
        <Typography.Text type="secondary" className="db-sql-preview" ellipsis>
          {data?.sql}
        </Typography.Text>
        <Pagination
          size="small"
          current={page}
          pageSize={pageSize}
          total={data?.total ?? 0}
          showSizeChanger
          pageSizeOptions={PAGE_SIZES}
          onChange={(nextPage, nextSize) =>
            guardChanges(() => {
              setPage(nextSize !== pageSize ? 1 : nextPage);
              setPageSize(nextSize);
            })
          }
        />
      </div>

      {contextMenu && menuRow && menuColumn ? (
        <Dropdown
          open
          menu={{
            items: [
              {
                key: "copy",
                label: t("复制值"),
                onClick: () => {
                  const value = currentValue(menuRow, menuColumn.name);
                  void navigator.clipboard
                    ?.writeText(
                      value === null || value === undefined
                        ? ""
                        : formatCell(value),
                    )
                    .then(() => message.success(t("已复制")));
                },
              },
              {
                key: "edit",
                label: t("编辑"),
                disabled: !canEditCell(menuRow, menuColumn),
                onClick: () => startEdit(menuRow, menuColumn),
              },
              {
                key: "null",
                label: t("设为 NULL"),
                disabled:
                  !canEditCell(menuRow, menuColumn) || !menuColumn.nullable,
                onClick: () => setCell(menuRow.key, menuColumn.name, null),
              },
              {
                key: "empty",
                label: t("设为空字符串"),
                disabled: !canEditCell(menuRow, menuColumn),
                onClick: () => setCell(menuRow.key, menuColumn.name, ""),
              },
              { type: "divider" },
              {
                key: "filter",
                label: t("按此值筛选"),
                onClick: () => {
                  const value = currentValue(menuRow, menuColumn.name);
                  const filter: DbFilter =
                    value === null || value === undefined
                      ? { column: menuColumn.name, op: "is_null" }
                      : {
                          column: menuColumn.name,
                          op: "eq",
                          value: formatCell(value),
                        };
                  setDraftFilters([filter]);
                  setShowFilters(true);
                  guardChanges(() => {
                    setFilters([filter]);
                    setPage(1);
                  });
                },
              },
              {
                key: "delete",
                danger: true,
                label: t("删除此行"),
                disabled: menuRow.isNew ? !writable : !editable,
                onClick: () => removeRows([menuRow.key]),
              },
            ],
          }}
        >
          <span
            className="db-context-anchor"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          />
        </Dropdown>
      ) : null}
    </div>
  );
}
