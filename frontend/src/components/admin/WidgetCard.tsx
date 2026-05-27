import { Card, Statistic } from "antd";
import type { ReactNode } from "react";

export function WidgetCard({
  title,
  value,
  prefix,
  suffix,
  tone = "blue",
}: {
  title: string;
  value: string | number;
  prefix?: ReactNode;
  suffix?: string;
  tone?: "blue" | "green" | "orange" | "red";
}) {
  return (
    <Card className={`widget-card widget-card-${tone}`}>
      <Statistic title={title} value={value} prefix={prefix} suffix={suffix} />
    </Card>
  );
}
