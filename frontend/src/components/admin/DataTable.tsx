import { Table } from "antd";
import type { TableProps } from "antd";

export function DataTable<T extends object>({
  pagination,
  scroll,
  ...tableProps
}: TableProps<T>) {
  return (
    <div className="data-table-shell">
      <Table<T>
        bordered
        size="middle"
        rowKey="id"
        pagination={{
          pageSize: 10,
          showSizeChanger: true,
          ...pagination,
        }}
        scroll={{ x: "max-content", ...scroll }}
        {...tableProps}
      />
    </div>
  );
}
