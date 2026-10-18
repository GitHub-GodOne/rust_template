import { MoreOutlined } from "@ant-design/icons";
import { Button, Drawer, Grid, Table } from "antd";
import type { TableProps } from "antd";
import type { ColumnsType } from "antd/es/table";
import {
  Children,
  type ReactElement,
  type ReactNode,
  isValidElement,
  useState,
} from "react";
import { t } from "../../i18n";
import { standardTablePaginationProps } from "./StandardTablePagination";

type ActionElementProps = {
  children?: ReactNode;
  href?: string;
  onClick?: unknown;
};

function componentName(element: ReactElement<ActionElementProps>) {
  const { type } = element;
  if (typeof type === "string") {
    return type;
  }
  if (typeof type === "function") {
    const component = type as { displayName?: string; name?: string };
    return component.displayName ?? component.name ?? "";
  }
  if (typeof type === "object" && type) {
    const component = type as {
      displayName?: string;
      render?: { displayName?: string; name?: string };
    };
    return (
      component.displayName ??
      component.render?.displayName ??
      component.render?.name ??
      ""
    );
  }
  return "";
}

function countTableActions(node: ReactNode): number {
  return Children.toArray(node).reduce<number>((count, child) => {
    if (!isValidElement<ActionElementProps>(child)) {
      return count + 1;
    }

    const name = componentName(child);
    const isInteractive =
      name === "button" ||
      name === "a" ||
      name.includes("Button") ||
      Boolean(child.props.href) ||
      typeof child.props.onClick === "function";
    if (isInteractive) {
      return count + 1;
    }

    const nestedActions = countTableActions(child.props.children);
    return count + Math.max(nestedActions, 1);
  }, 0);
}

function ResponsiveTableActions({
  children,
  isMobile,
  onOpenMobileActions,
}: {
  children: ReactNode;
  isMobile: boolean;
  onOpenMobileActions: (actions: ReactNode) => void;
}) {
  if (countTableActions(children) < 2) {
    return <div className="responsive-table-actions-single">{children}</div>;
  }

  if (isMobile) {
    return (
      <div className="responsive-table-actions">
        <Button
          className="responsive-table-actions-trigger"
          type="text"
          size="small"
          icon={<MoreOutlined />}
          aria-label={t("更多操作")}
          title={t("更多操作")}
          onPointerDown={(event) => event.stopPropagation()}
          onPointerUp={(event) => {
            event.stopPropagation();
            if (event.pointerType !== "mouse") {
              onOpenMobileActions(children);
            }
          }}
          onClick={(event) => {
            event.stopPropagation();
            onOpenMobileActions(children);
          }}
        />
      </div>
    );
  }

  return (
    <div className="responsive-table-actions">
      <div className="responsive-table-actions-desktop">{children}</div>
    </div>
  );
}

function isActionColumn<T extends object>(
  column: NonNullable<ColumnsType<T>>[number],
) {
  if (String(column.key ?? "") === "actions") {
    return true;
  }
  // Titles are translated; match the source text as well as the current language.
  if (typeof column.title !== "string") {
    return false;
  }
  const title = column.title.trim();
  return title === t("操作") || title === "操作";
}

function responsiveColumns<T extends object>(
  columns: ColumnsType<T> | undefined,
  isMobile: boolean,
  onOpenMobileActions: (actions: ReactNode) => void,
): ColumnsType<T> | undefined {
  return columns?.map((column) => {
    if ("children" in column && column.children) {
      return {
        ...column,
        children:
          responsiveColumns(column.children, isMobile, onOpenMobileActions) ??
          [],
      };
    }
    if (!isActionColumn(column) || !column.render) {
      return column;
    }

    const render = column.render;
    return {
      ...column,
      align: isMobile ? "center" : column.align,
      className: [column.className, "responsive-action-column"]
        .filter(Boolean)
        .join(" "),
      width: isMobile ? 84 : column.width,
      render: (value, record, index) => (
        <ResponsiveTableActions
          isMobile={isMobile}
          onOpenMobileActions={onOpenMobileActions}
        >
          {render(value, record, index) as ReactNode}
        </ResponsiveTableActions>
      ),
    };
  });
}

export function DataTable<T extends object>({
  className,
  columns,
  pagination,
  scroll,
  ...tableProps
}: TableProps<T>) {
  const screens = Grid.useBreakpoint();
  const isMobile = !screens.md;
  const [mobileActions, setMobileActions] = useState<ReactNode>(null);
  const resolvedPagination =
    pagination === false
      ? false
      : {
          ...standardTablePaginationProps,
          position: ["bottomRight" as const],
          ...pagination,
        };

  return (
    <>
      <div className="data-table-shell">
        <Table<T>
          className={["professional-data-table", className]
            .filter(Boolean)
            .join(" ")}
          size="small"
          rowKey="id"
          pagination={resolvedPagination}
          scroll={{ x: "max-content", ...scroll }}
          columns={responsiveColumns(columns, isMobile, setMobileActions)}
          {...tableProps}
        />
      </div>
      <Drawer
        title={t("更多操作")}
        open={isMobile && mobileActions !== null}
        placement="bottom"
        height="auto"
        rootClassName="responsive-table-actions-drawer-root"
        className="responsive-table-actions-drawer"
        onClose={() => setMobileActions(null)}
      >
        <div className="responsive-table-actions-menu">{mobileActions}</div>
      </Drawer>
    </>
  );
}
