import * as AntIcons from "@ant-design/icons";
import { type ComponentType, createElement } from "react";
import { t } from "../i18n";

const menuIconLabels: Record<string, string> = {
  home: t("首页 HomeOutlined"),
  dashboard: t("仪表盘 DashboardOutlined"),
  setting: t("系统 SettingOutlined"),
  user: t("用户 UserOutlined"),
  team: t("团队 TeamOutlined"),
  menu: t("菜单 MenuOutlined"),
  safety: t("安全 SafetyCertificateOutlined"),
  "file-search": t("日志 FileSearchOutlined"),
  "cloud-upload": t("上传 CloudUploadOutlined"),
  "cloud-sync": t("同步 CloudSyncOutlined"),
  "cloud-server": t("存储 CloudServerOutlined"),
  "folder-open": t("文件 FolderOpenOutlined"),
  picture: t("素材 PictureOutlined"),
  control: t("设置 ControlOutlined"),
  "credit-card": t("支付 CreditCardOutlined"),
  api: t("接口 ApiOutlined"),
  apartment: t("租户 ApartmentOutlined"),
  branches: t("部门 BranchesOutlined"),
  notification: t("通知 NotificationOutlined"),
  schedule: t("定时 ScheduleOutlined"),
  database: t("数据库 DatabaseOutlined"),
  monitor: t("监控 MonitorOutlined"),
  mail: t("邮件 MailOutlined"),
  profile: t("工单 ProfileOutlined"),
  read: t("内容 ReadOutlined"),
  code: "SSH CodeOutlined",
  wifi: t("网络 WifiOutlined"),
  appstore: t("应用 AppstoreOutlined"),
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
