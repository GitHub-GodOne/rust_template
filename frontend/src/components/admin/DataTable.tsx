import { Table } from "antd";
import type { TableProps } from "antd";

export function DataTable<T extends object>({
  className,
  pagination,
  scroll,
  ...tableProps
}: TableProps<T>) {
  return (
    <div className="data-table-shell">
      <Table<T>
        bordered
        sticky
        className={["professional-data-table", className]
          .filter(Boolean)
          .join(" ")}
        size="small"
        rowKey="id"
        pagination={{
          pageSize: 10,
          showSizeChanger: true,
          showQuickJumper: true,
          showTotal: (total) => `共 ${total} 条`,
          ...pagination,
        }}
        scroll={{ x: "max-content", ...scroll }}
        {...tableProps}
      />
    </div>
  );
}
