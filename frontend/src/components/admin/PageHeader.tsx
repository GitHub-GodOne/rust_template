import { Breadcrumb, Space, Typography } from "antd";
import type { ReactNode } from "react";

export function PageHeader({
  title,
  subtitle,
  icon,
  breadcrumb,
  extra,
}: {
  title: string;
  subtitle?: string;
  icon?: ReactNode;
  breadcrumb?: string[];
  extra?: ReactNode;
}) {
  return (
    <div className="page-header">
      <div>
        {breadcrumb && (
          <Breadcrumb
            className="page-breadcrumb"
            items={breadcrumb.map((item) => ({ title: item }))}
          />
        )}
        <Space align="center">
          {icon && <span className="page-header-icon">{icon}</span>}
          <div>
            <Typography.Title level={3} className="page-title">
              {title}
            </Typography.Title>
            {subtitle && (
              <Typography.Text type="secondary">{subtitle}</Typography.Text>
            )}
          </div>
        </Space>
      </div>
      {extra && <div>{extra}</div>}
    </div>
  );
}
