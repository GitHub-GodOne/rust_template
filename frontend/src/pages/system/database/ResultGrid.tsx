import { Table, Tooltip, Typography, message } from "antd";
import type { ColumnsType } from "antd/es/table";
import { useEffect, useMemo, useRef, useState } from "react";
import type { DbCell } from "../../../api/admin/dbManager";
import { t } from "../../../i18n";
import { formatCell } from "./dbUtils";

type GridRow = { __index: number; cells: DbCell[] };

const MAX_CELL_CHARS = 300;

export function CellText({ value }: { value: DbCell }) {
  if (value === null) {
    return <span className="db-cell-null">NULL</span>;
  }
  const text = formatCell(value);
  const shown =
    text.length > MAX_CELL_CHARS ? `${text.slice(0, MAX_CELL_CHARS)}…` : text;
  return (
    <span
      className={typeof value === "number" ? "db-cell-number" : undefined}
      title={text.length > 40 ? text.slice(0, 2000) : undefined}
    >
      {shown}
    </span>
  );
}

export function columnWidth(name: string, typeName?: string) {
  const label = Math.max(name.length, (typeName ?? "").length * 0.7);
  return Math.min(320, Math.max(90, Math.round(label * 8.5) + 36));
}

/** Read-only grid for SQL results; double-click copies a cell. */
export function ResultGrid({
  columns,
  rows,
  typeNames,
}: {
  columns: string[];
  rows: DbCell[][];
  typeNames?: string[];
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(300);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const observer = new ResizeObserver(() => {
      // Header and borders take about 40px.
      setHeight(Math.max(120, host.clientHeight - 40));
    });
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  const data = useMemo<GridRow[]>(
    () => rows.map((cells, index) => ({ __index: index, cells })),
    [rows],
  );
  const tableColumns = useMemo<ColumnsType<GridRow>>(
    () => [
      {
        key: "__index",
        title: "#",
        width: 56,
        fixed: "left",
        className: "db-grid-index",
        render: (_, row) => row.__index + 1,
      },
      ...columns.map((name, columnIndex) => ({
        key: `${columnIndex}`,
        width: columnWidth(name, typeNames?.[columnIndex]),
        ellipsis: true,
        title: (
          <Tooltip title={typeNames?.[columnIndex]}>
            <span>{name}</span>
          </Tooltip>
        ),
        onCell: (row: GridRow) => ({
          onDoubleClick: () => {
            const value = row.cells[columnIndex];
            void navigator.clipboard
              ?.writeText(value === null ? "" : formatCell(value))
              .then(() => message.success(t("已复制")));
          },
        }),
        render: (_: unknown, row: GridRow) => (
          <CellText value={row.cells[columnIndex]} />
        ),
      })),
    ],
    [columns, typeNames],
  );

  // Virtual tables need a numeric width.
  const totalWidth = tableColumns.reduce(
    (sum, column) => sum + Number(column.width ?? 0),
    0,
  );

  if (!columns.length) {
    return (
      <Typography.Text type="secondary">
        {t("该语句没有返回结果集")}
      </Typography.Text>
    );
  }

  return (
    <div ref={hostRef} className="db-result-grid">
      <Table<GridRow>
        virtual
        size="small"
        bordered
        rowKey="__index"
        columns={tableColumns}
        dataSource={data}
        pagination={false}
        scroll={{ x: totalWidth, y: height }}
      />
    </div>
  );
}
