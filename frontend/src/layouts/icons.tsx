import * as AntIcons from "@ant-design/icons";
import { type ComponentType, createElement } from "react";

const menuIconLabels: Record<string, string> = {
  dashboard: "仪表盘 DashboardOutlined",
  setting: "系统 SettingOutlined",
  user: "用户 UserOutlined",
  team: "团队 TeamOutlined",
  menu: "菜单 MenuOutlined",
  safety: "安全 SafetyCertificateOutlined",
  "file-search": "日志 FileSearchOutlined",
  "cloud-upload": "上传 CloudUploadOutlined",
  "cloud-sync": "同步 CloudSyncOutlined",
  "cloud-server": "存储 CloudServerOutlined",
  "folder-open": "文件 FolderOpenOutlined",
  picture: "素材 PictureOutlined",
  control: "设置 ControlOutlined",
  "credit-card": "支付 CreditCardOutlined",
  api: "接口 ApiOutlined",
  apartment: "租户 ApartmentOutlined",
  branches: "部门 BranchesOutlined",
  notification: "通知 NotificationOutlined",
  schedule: "定时 ScheduleOutlined",
  database: "数据库 DatabaseOutlined",
  monitor: "监控 MonitorOutlined",
  mail: "邮件 MailOutlined",
  profile: "工单 ProfileOutlined",
  read: "内容 ReadOutlined",
  code: "SSH CodeOutlined",
};

function iconValueFromExportName(name: string) {
  return name
    .replace(/Outlined$/, "")
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .toLowerCase();
}

export const menuIconOptions = Object.entries(AntIcons)
  .filter(([name]) => name.endsWith("Outlined"))
  .map(([name, Icon]) => {
    const value = iconValueFromExportName(name);
    return {
      value,
      label: menuIconLabels[value] ?? name,
      icon: createElement(Icon as ComponentType),
    };
  })
  .sort((left, right) => left.label.localeCompare(right.label));

const icons = Object.fromEntries(
  menuIconOptions.map((option) => [option.value, option.icon]),
);

export function renderMenuIcon(name?: string | null) {
  if (!name) {
    return <AntIcons.MenuOutlined />;
  }
  return icons[name] ?? <AntIcons.MenuOutlined />;
}
