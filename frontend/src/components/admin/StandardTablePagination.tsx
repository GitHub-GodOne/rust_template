import { Pagination } from "antd";
import type { PaginationProps } from "antd";
import { t } from "../../i18n";

export const standardTablePaginationProps = {
  pageSize: 10,
  pageSizeOptions: [10, 20, 50, 100],
  responsive: true,
  showQuickJumper: true,
  showSizeChanger: true,
  showTotal: (total: number) => t("共 {0} 条", total),
  size: "small",
} satisfies PaginationProps;

export function StandardTablePagination({
  className,
  ...paginationProps
}: PaginationProps) {
  return (
    <Pagination
      {...standardTablePaginationProps}
      {...paginationProps}
      className={["standard-table-pagination", className]
        .filter(Boolean)
        .join(" ")}
    />
  );
}
