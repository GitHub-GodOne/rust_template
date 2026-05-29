import {
  ApartmentOutlined,
  CloudUploadOutlined,
  ControlOutlined,
  CreditCardOutlined,
  DashboardOutlined,
  DatabaseOutlined,
  FileSearchOutlined,
  MailOutlined,
  MenuOutlined,
  MonitorOutlined,
  NotificationOutlined,
  ProfileOutlined,
  ReadOutlined,
  SafetyCertificateOutlined,
  ScheduleOutlined,
  SettingOutlined,
  TeamOutlined,
  UserOutlined,
} from "@ant-design/icons";

const icons = {
  dashboard: <DashboardOutlined />,
  setting: <SettingOutlined />,
  user: <UserOutlined />,
  team: <TeamOutlined />,
  menu: <MenuOutlined />,
  safety: <SafetyCertificateOutlined />,
  "file-search": <FileSearchOutlined />,
  "cloud-upload": <CloudUploadOutlined />,
  control: <ControlOutlined />,
  "credit-card": <CreditCardOutlined />,
  apartment: <ApartmentOutlined />,
  notification: <NotificationOutlined />,
  schedule: <ScheduleOutlined />,
  database: <DatabaseOutlined />,
  monitor: <MonitorOutlined />,
  mail: <MailOutlined />,
  profile: <ProfileOutlined />,
  read: <ReadOutlined />,
};

export function renderMenuIcon(name: string) {
  return icons[name as keyof typeof icons] ?? <MenuOutlined />;
}
