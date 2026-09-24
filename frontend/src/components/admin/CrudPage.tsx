import { MoreOutlined } from "@ant-design/icons";
import { Alert, Button, Card, Drawer, Grid } from "antd";
import type { ReactNode } from "react";
import { useState } from "react";
import { CrudToolbar } from "./CrudToolbar";
import { PageHeader } from "./PageHeader";

export function CrudPage({
  title,
  subtitle,
  breadcrumb,
  icon,
  children,
  toolbar,
  notice,
  className,
  extra,
}: {
  title: string;
  subtitle?: string;
  breadcrumb?: string[];
  extra?: ReactNode;
  icon?: ReactNode;
  children: ReactNode;
  toolbar?: ReactNode;
  notice?: string;
  className?: string;
}) {
  const [mobileToolbarOpen, setMobileToolbarOpen] = useState(false);
  const screens = Grid.useBreakpoint();
  const isMobile = !screens.md;
  const cardToolbar = toolbar ?? <CrudToolbar />;

  return (
    <div className={className}>
      <PageHeader
        title={title}
        subtitle={subtitle}
        breadcrumb={breadcrumb}
        icon={icon}
        extra={
          <>
            {extra}
            {!isMobile && (
              <div className="crud-page-toolbar-desktop">{cardToolbar}</div>
            )}
            <Button
              className="crud-page-toolbar-mobile"
              type="primary"
              icon={<MoreOutlined />}
              onPointerDown={(event) => event.stopPropagation()}
              onPointerUp={(event) => {
                event.stopPropagation();
                if (event.pointerType !== "mouse") {
                  setMobileToolbarOpen(true);
                }
              }}
              onClick={(event) => {
                event.stopPropagation();
                setMobileToolbarOpen(true);
              }}
            >
              操作
            </Button>
          </>
        }
      />
      <Card className="admin-card crud-card">
        {notice && (
          <Alert
            type="info"
            showIcon
            message={notice}
            className="page-notice"
          />
        )}
        {children}
      </Card>
      <Drawer
        title={`${title} · 操作`}
        open={isMobile && mobileToolbarOpen}
        onClose={() => setMobileToolbarOpen(false)}
        placement="bottom"
        height="auto"
        rootClassName="admin-action-drawer-root"
        className="admin-action-drawer"
      >
        {isMobile && (
          <div className="admin-mobile-toolbar-sheet">{cardToolbar}</div>
        )}
      </Drawer>
    </div>
  );
}
