import {
  ApartmentOutlined,
  CloudUploadOutlined,
  ControlOutlined,
  DashboardOutlined,
  FileSearchOutlined,
  MenuOutlined,
  SafetyCertificateOutlined,
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
  apartment: <ApartmentOutlined />,
};

export function renderMenuIcon(name: string) {
  return icons[name as keyof typeof icons] ?? <MenuOutlined />;
}
