import { Alert, Card } from "antd";
import type { ReactNode } from "react";
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
  return (
    <div>
      <PageHeader
        title={title}
        subtitle={subtitle}
        breadcrumb={breadcrumb}
        icon={icon}
      />
      <Card
        className="admin-card"
        title={title}
        extra={toolbar ?? <CrudToolbar />}
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
    </div>
  );
}
