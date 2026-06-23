import { FilterOutlined, MoreOutlined } from "@ant-design/icons";
import { Alert, Button, Card, Modal, Space, Typography } from "antd";
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
}: {
  title: string;
  subtitle?: string;
  breadcrumb: string[];
  icon?: ReactNode;
  children: ReactNode;
  toolbar?: ReactNode;
  notice?: string;
}) {
  const [mobileToolbarOpen, setMobileToolbarOpen] = useState(false);
  const cardToolbar = toolbar ?? <CrudToolbar />;

  return (
    <div>
      <PageHeader
        title={title}
        subtitle={subtitle}
        breadcrumb={breadcrumb}
        icon={icon}
      />
      <Card
        className="admin-card crud-card"
        title={
          <Space size={10} className="crud-card-title">
            {icon ? <span className="crud-card-icon">{icon}</span> : null}
            <span>{title}</span>
            {subtitle ? (
              <Typography.Text type="secondary" className="crud-card-subtitle">
                {subtitle}
              </Typography.Text>
            ) : null}
          </Space>
        }
        extra={
          <>
            <div className="crud-card-extra-desktop">{cardToolbar}</div>
            <Space.Compact block className="crud-card-extra-mobile">
              <Button
                icon={<FilterOutlined />}
                onClick={() => setMobileToolbarOpen(true)}
              >
                筛选 / 操作
              </Button>
              <Button
                icon={<MoreOutlined />}
                onClick={() => setMobileToolbarOpen(true)}
              >
                更多
              </Button>
            </Space.Compact>
          </>
        }
      >
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
      <Modal
        title={`${title} 操作`}
        open={mobileToolbarOpen}
        onCancel={() => setMobileToolbarOpen(false)}
        footer={null}
        width="min(560px, 94vw)"
      >
        <div className="admin-mobile-toolbar-sheet">{cardToolbar}</div>
      </Modal>
    </div>
  );
}
