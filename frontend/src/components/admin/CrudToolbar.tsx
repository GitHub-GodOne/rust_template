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
import { Modal, Space } from "antd";
import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import type { PermissionCode } from "../../app/menu";
import { t } from "../../i18n";
import { PermissionButton } from "./PermissionButton";

type ToolbarAction = {
  key: string;
  label: string;
  icon: ReactNode;
  permission?: PermissionCode;
  danger?: boolean;
  disabled?: boolean;
  primary?: boolean;
  onClick?: () => void;
};

const defaultActions: ToolbarAction[] = [
  { key: "create", label: t("新增"), icon: <PlusOutlined />, primary: true },
  { key: "delete", label: t("删除"), icon: <DeleteOutlined />, danger: true },
  { key: "save", label: t("保存"), icon: <SaveOutlined /> },
  { key: "cancel", label: t("取消"), icon: <StopOutlined /> },
  { key: "refresh", label: t("刷新"), icon: <ReloadOutlined /> },
  { key: "import", label: t("导入"), icon: <CloudUploadOutlined /> },
  { key: "export", label: t("导出"), icon: <CloudDownloadOutlined /> },
  { key: "print", label: t("打印"), icon: <PrinterOutlined /> },
  { key: "help", label: t("帮助"), icon: <QuestionCircleOutlined /> },
];

export function CrudToolbar({
  actions = defaultActions,
}: {
  actions?: ToolbarAction[];
}) {
  const [mobileActionsOpen, setMobileActionsOpen] = useState(false);
  const primaryAction = useMemo(
    () => actions.find((action) => action.primary) ?? actions[0],
    [actions],
  );
  const secondaryActions = actions.filter(
    (action) => action.key !== primaryAction?.key,
  );

  const renderAction = (action: ToolbarAction, block = false) => (
    <PermissionButton
      key={action.key}
      block={block}
      size={block ? "middle" : "small"}
      permission={action.permission}
      danger={action.danger}
      disabled={action.disabled || !action.onClick}
      type={action.primary ? "primary" : "default"}
      icon={action.icon}
      onClick={() => {
        setMobileActionsOpen(false);
        action.onClick?.();
      }}
    >
      {action.label}
    </PermissionButton>
  );

  return (
    <div className="crud-toolbar">
      <Space wrap className="crud-toolbar-desktop">
        {actions.map((action) => renderAction(action))}
      </Space>
      <Space.Compact block className="crud-toolbar-mobile">
        {primaryAction ? renderAction(primaryAction) : null}
        {secondaryActions.length > 0 ? (
          <PermissionButton onClick={() => setMobileActionsOpen(true)}>
            {t("更多")}
          </PermissionButton>
        ) : null}
      </Space.Compact>
      <Modal
        title={t("更多操作")}
        open={mobileActionsOpen}
        onCancel={() => setMobileActionsOpen(false)}
        footer={null}
        width="min(420px, 92vw)"
      >
        <Space direction="vertical" className="admin-form-stack">
          {secondaryActions.map((action) => renderAction(action, true))}
        </Space>
      </Modal>
    </div>
  );
}
