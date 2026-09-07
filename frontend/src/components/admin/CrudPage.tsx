import { MoreOutlined } from "@ant-design/icons";
import { Alert, Button, Card, Drawer } from "antd";
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
}: {
  title: string;
  subtitle?: string;
  breadcrumb: string[];
  icon?: ReactNode;
  children: ReactNode;
  toolbar?: ReactNode;
  notice?: string;
  className?: string;
}) {
  const [mobileToolbarOpen, setMobileToolbarOpen] = useState(false);
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
            <div className="crud-page-toolbar-desktop">{cardToolbar}</div>
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
        open={mobileToolbarOpen}
        onClose={() => setMobileToolbarOpen(false)}
        placement="bottom"
        height="auto"
        rootClassName="admin-action-drawer-root"
        className="admin-action-drawer"
      >
        <div className="admin-mobile-toolbar-sheet">{cardToolbar}</div>
      </Drawer>
    </div>
  );
}
