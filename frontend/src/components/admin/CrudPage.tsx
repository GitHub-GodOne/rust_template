import { MoreOutlined } from "@ant-design/icons";
import { Alert, Button, Card, Drawer, Grid } from "antd";
import type { ReactNode } from "react";
import { useState } from "react";

/**
 * Standard admin page: one card that fills the content area. Filters and
 * actions (`toolbar`, `extra`) sit in a slim bar at the top of the card; on
 * phones the toolbar opens from an 操作 button instead.
 */
export function CrudPage({
  title,
  children,
  toolbar,
  notice,
  className,
  extra,
}: {
  title: string;
  /** Kept for callers; no longer shown. */
  subtitle?: string;
  /** Kept for callers; no longer shown. */
  breadcrumb?: string[];
  extra?: ReactNode;
  /** Kept for callers; no longer shown. */
  icon?: ReactNode;
  children: ReactNode;
  toolbar?: ReactNode;
  notice?: string;
  className?: string;
}) {
  const [mobileToolbarOpen, setMobileToolbarOpen] = useState(false);
  const screens = Grid.useBreakpoint();
  const isMobile = !screens.md;
  const hasBar = Boolean(toolbar || extra);

  return (
    <div className={`crud-page${className ? ` ${className}` : ""}`}>
      <h1 className="visually-hidden">{title}</h1>
      <Card className="admin-card crud-card">
        {hasBar ? (
          <div className="crud-card-toolbar">
            {extra ? (
              <div className="crud-card-toolbar-extra">{extra}</div>
            ) : null}
            {toolbar && !isMobile ? (
              <div className="crud-page-toolbar-desktop">{toolbar}</div>
            ) : null}
            {toolbar && isMobile ? (
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
            ) : null}
          </div>
        ) : null}
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
      {toolbar ? (
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
            <div className="admin-mobile-toolbar-sheet">{toolbar}</div>
          )}
        </Drawer>
      ) : null}
    </div>
  );
}
