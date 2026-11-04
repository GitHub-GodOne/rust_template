import {
  ArrowDownOutlined,
  ArrowUpOutlined,
  CaretRightOutlined,
  DeleteOutlined,
  EditOutlined,
  PlusOutlined,
} from "@ant-design/icons";
import {
  Button,
  Drawer,
  Empty,
  Form,
  Input,
  List,
  Modal,
  Popconfirm,
  Switch,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import { useState } from "react";
import { t } from "../../../i18n";
import { createClientId } from "../../../utils/clientId";
import {
  MAX_SSH_COMMANDS,
  MAX_SSH_COMMAND_LENGTH,
  MAX_SSH_COMMAND_NAME_LENGTH,
  type SshQuickCommand,
  fitsPreferenceLimit,
  moveSshCommand,
} from "./sshCommands";

type CommandFormValues = {
  name: string;
  command: string;
  auto_enter: boolean;
  run_on_connect: boolean;
};

type SshCommandsDrawerProps = {
  open: boolean;
  commands: SshQuickCommand[];
  loading: boolean;
  saving: boolean;
  canRun: boolean;
  /** Prefills a new command, e.g. with what is typed in the command bar. */
  draftCommand?: string;
  onClose: () => void;
  /** Persists the whole list; resolves false when saving failed. */
  onSave: (commands: SshQuickCommand[]) => Promise<boolean>;
  onRun: (command: SshQuickCommand) => void;
};

export function SshCommandsDrawer({
  open,
  commands,
  loading,
  saving,
  canRun,
  draftCommand,
  onClose,
  onSave,
  onRun,
}: SshCommandsDrawerProps) {
  const [form] = Form.useForm<CommandFormValues>();
  const [editing, setEditing] = useState<SshQuickCommand | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [formInitial, setFormInitial] = useState<CommandFormValues>();

  const openForm = (command: SshQuickCommand | null) => {
    if (!command && commands.length >= MAX_SSH_COMMANDS) {
      message.warning(t("最多保存 {0} 条命令", MAX_SSH_COMMANDS));
      return;
    }
    setEditing(command);
    // The form remounts on every open (destroyOnClose), so this seeds it.
    setFormInitial(
      command
        ? {
            name: command.name,
            command: command.command,
            auto_enter: command.auto_enter,
            run_on_connect: command.run_on_connect,
          }
        : {
            name: "",
            command: draftCommand?.trim() ?? "",
            auto_enter: true,
            run_on_connect: false,
          },
    );
    setFormOpen(true);
  };

  const save = async (next: SshQuickCommand[]) => {
    if (!fitsPreferenceLimit(next)) {
      message.error(t("命令内容过多，请删除部分命令后再保存"));
      return false;
    }
    return onSave(next);
  };

  const submit = async (values: CommandFormValues) => {
    const entry: SshQuickCommand = {
      id: editing?.id ?? createClientId("cmd"),
      name: values.name.trim(),
      command: values.command.replace(/\s+$/, ""),
      auto_enter: values.auto_enter,
      run_on_connect: values.run_on_connect,
    };
    const next = editing
      ? commands.map((item) => (item.id === editing.id ? entry : item))
      : [...commands, entry];
    if (await save(next)) {
      message.success(editing ? t("命令已更新") : t("命令已添加"));
      setFormOpen(false);
    }
  };

  return (
    <>
      <Drawer
        title={t("快捷命令")}
        open={open}
        width="min(480px, 100vw)"
        onClose={onClose}
        className="ssh-commands-drawer"
        extra={
          <Button
            type="primary"
            size="small"
            icon={<PlusOutlined />}
            onClick={() => openForm(null)}
          >
            {t("添加命令")}
          </Button>
        }
      >
        <Typography.Paragraph type="secondary" className="ssh-commands-tip">
          {t(
            "命令只保存在你自己的账号下。点击运行会发送到当前终端；也可以在终端上方的快捷栏一键运行。开启“连接时自动执行”的命令会在每次新建终端会话时按列表顺序自动运行。",
          )}
        </Typography.Paragraph>
        <List<SshQuickCommand>
          loading={loading}
          dataSource={commands}
          locale={{
            emptyText: (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t("还没有快捷命令，点击右上角添加")}
              />
            ),
          }}
          renderItem={(item, index) => (
            <List.Item
              className="ssh-command-item"
              actions={[
                <Tooltip key="run" title={t("运行")}>
                  <Button
                    type="primary"
                    size="small"
                    icon={<CaretRightOutlined />}
                    aria-label={t("运行")}
                    disabled={!canRun}
                    onClick={() => onRun(item)}
                  />
                </Tooltip>,
                <Tooltip key="edit" title={t("编辑")}>
                  <Button
                    type="text"
                    size="small"
                    icon={<EditOutlined />}
                    aria-label={t("编辑")}
                    onClick={() => openForm(item)}
                  />
                </Tooltip>,
              ]}
            >
              <div className="ssh-command-item-body">
                <div className="ssh-command-item-title">
                  <Typography.Text strong ellipsis>
                    {item.name}
                  </Typography.Text>
                  {item.run_on_connect && (
                    <Tag color="blue">{t("连接时执行")}</Tag>
                  )}
                  {!item.auto_enter && <Tag>{t("不自动回车")}</Tag>}
                </div>
                <Typography.Text code className="ssh-command-item-code">
                  {item.command}
                </Typography.Text>
                <div className="ssh-command-item-tools">
                  <Button
                    type="text"
                    size="small"
                    icon={<ArrowUpOutlined />}
                    aria-label={t("上移")}
                    disabled={index === 0 || saving}
                    onClick={() =>
                      void save(moveSshCommand(commands, index, -1))
                    }
                  />
                  <Button
                    type="text"
                    size="small"
                    icon={<ArrowDownOutlined />}
                    aria-label={t("下移")}
                    disabled={index === commands.length - 1 || saving}
                    onClick={() =>
                      void save(moveSshCommand(commands, index, 1))
                    }
                  />
                  <Popconfirm
                    title={t("删除这条命令？")}
                    okText={t("删除")}
                    cancelText={t("取消")}
                    onConfirm={() =>
                      save(commands.filter((command) => command.id !== item.id))
                    }
                  >
                    <Button
                      type="text"
                      size="small"
                      danger
                      icon={<DeleteOutlined />}
                      aria-label={t("删除")}
                      disabled={saving}
                    />
                  </Popconfirm>
                </div>
              </div>
            </List.Item>
          )}
        />
      </Drawer>
      <Modal
        title={editing ? t("编辑命令") : t("添加命令")}
        open={formOpen}
        okText={t("保存")}
        cancelText={t("取消")}
        confirmLoading={saving}
        onCancel={() => setFormOpen(false)}
        onOk={() => form.submit()}
        destroyOnClose
      >
        <Form<CommandFormValues>
          form={form}
          layout="vertical"
          preserve={false}
          initialValues={formInitial}
          onFinish={(values) => void submit(values)}
        >
          <Form.Item
            name="name"
            label={t("名称")}
            rules={[
              { required: true, whitespace: true, message: t("请输入名称") },
            ]}
          >
            <Input
              maxLength={MAX_SSH_COMMAND_NAME_LENGTH}
              placeholder={t("例如：查看磁盘")}
            />
          </Form.Item>
          <Form.Item
            name="command"
            label={t("命令")}
            extra={t("多行命令会按行依次执行")}
            rules={[
              { required: true, whitespace: true, message: t("请输入命令") },
            ]}
          >
            <Input.TextArea
              autoSize={{ minRows: 3, maxRows: 8 }}
              maxLength={MAX_SSH_COMMAND_LENGTH}
              autoCapitalize="off"
              autoCorrect="off"
              spellCheck={false}
              className="ssh-command-input"
              placeholder="df -h"
            />
          </Form.Item>
          <Form.Item
            name="auto_enter"
            label={t("自动回车执行")}
            valuePropName="checked"
            extra={t("关闭后只把命令填入终端，方便修改参数后再回车")}
          >
            <Switch />
          </Form.Item>
          <Form.Item
            name="run_on_connect"
            label={t("连接时自动执行")}
            valuePropName="checked"
            extra={t(
              "每次新建终端会话后自动运行（总会回车执行），重新接入已有会话时不会重复运行；适合 export 环境变量、cd 到项目目录等",
            )}
          >
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}
