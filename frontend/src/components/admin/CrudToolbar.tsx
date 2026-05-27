import {
  CloudDownloadOutlined,
  CloudUploadOutlined,
  DeleteOutlined,
  PlusOutlined,
  PrinterOutlined,
  QuestionCircleOutlined,
  ReloadOutlined,
  SaveOutlined,
  StopOutlined,
} from "@ant-design/icons";
import { Space } from "antd";
import type { ReactNode } from "react";
import type { PermissionCode } from "../../app/menu";
import { PermissionButton } from "./PermissionButton";

type ToolbarAction = {
  key: string;
  label: string;
  icon: ReactNode;
  permission?: PermissionCode;
  danger?: boolean;
  primary?: boolean;
  onClick?: () => void;
};

const defaultActions: ToolbarAction[] = [
  { key: "create", label: "新增", icon: <PlusOutlined />, primary: true },
  { key: "delete", label: "删除", icon: <DeleteOutlined />, danger: true },
  { key: "save", label: "保存", icon: <SaveOutlined /> },
  { key: "cancel", label: "取消", icon: <StopOutlined /> },
  { key: "refresh", label: "刷新", icon: <ReloadOutlined /> },
  { key: "import", label: "导入", icon: <CloudUploadOutlined /> },
  { key: "export", label: "导出", icon: <CloudDownloadOutlined /> },
  { key: "print", label: "打印", icon: <PrinterOutlined /> },
  { key: "help", label: "帮助", icon: <QuestionCircleOutlined /> },
];

export function CrudToolbar({
  actions = defaultActions,
}: {
  actions?: ToolbarAction[];
}) {
  return (
    <div className="crud-toolbar">
      <Space wrap>
        {actions.map((action) => (
          <PermissionButton
            key={action.key}
            permission={action.permission}
            danger={action.danger}
            type={action.primary ? "primary" : "default"}
            icon={action.icon}
            onClick={action.onClick}
          >
            {action.label}
          </PermissionButton>
        ))}
      </Space>
    </div>
  );
}
