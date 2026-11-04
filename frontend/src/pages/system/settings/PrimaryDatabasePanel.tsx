import { DatabaseOutlined, ReloadOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Card,
  Checkbox,
  Input,
  Select,
  Space,
  Tag,
  Typography,
  message,
} from "antd";
import { useState } from "react";
import { fetchDbConnections } from "../../../api/admin/dbManager";
import {
  applyPrimaryDatabase,
  fetchPrimaryDatabase,
} from "../../../api/admin/primaryDatabase";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";

export function PrimaryDatabasePanel() {
  const [connectionId, setConnectionId] = useState<number>();
  const [reinitialize, setReinitialize] = useState(false);
  const [confirmPhrase, setConfirmPhrase] = useState("");
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: ["primary-database"],
    queryFn: fetchPrimaryDatabase,
  });
  const connectionsQuery = useQuery({
    queryKey: ["db-connections"],
    queryFn: fetchDbConnections,
  });
  const applyMutation = useMutation({
    mutationFn: applyPrimaryDatabase,
    onSuccess: (result) => {
      message.success(t("主数据库配置已写入，请重启服务"));
      queryClient.invalidateQueries({ queryKey: ["primary-database"] });
      if (result.reinitialized) {
        setConfirmPhrase("");
        setReinitialize(false);
      }
    },
  });

  const submit = () => {
    applyMutation.mutate({
      connection_id: connectionId,
      reinitialize,
      confirm_phrase: reinitialize ? confirmPhrase : undefined,
      restart: false,
    });
  };

  return (
    <Card
      size="small"
      title={
        <Space>
          <DatabaseOutlined />
          {t("主数据库生命周期")}
        </Space>
      }
    >
      <Space direction="vertical" size="middle" className="full-width">
        <Alert
          type="info"
          showIcon
          message={t(
            "主数据库切换会写入本地配置并需要重启服务；运行中的连接不会被热替换。当前安全路线支持 PostgreSQL。",
          )}
        />
        {statusQuery.data ? (
          <Typography.Text>
            {t(
              "当前：{0} · {1}",
              statusQuery.data.engine,
              statusQuery.data.masked_uri,
            )}
          </Typography.Text>
        ) : null}
        <Select
          allowClear
          value={connectionId}
          loading={connectionsQuery.isLoading}
          placeholder={t("保留当前主数据库")}
          style={{ maxWidth: 520, width: "100%" }}
          options={(connectionsQuery.data ?? [])
            .filter((connection) => connection.db_type === "postgres")
            .map((connection) => ({
              value: connection.id,
              label: `${connection.name} (${connection.host ?? ""}/${connection.database_name ?? ""})`,
            }))}
          onChange={(value: number | undefined) => setConnectionId(value)}
        />
        <Space wrap>
          <PermissionButton
            permission="system:database:manage"
            icon={<ReloadOutlined />}
            loading={applyMutation.isPending}
            onClick={submit}
          >
            {t("验证迁移并应用")}
          </PermissionButton>
          <Tag color="orange">{t("应用后请重启服务")}</Tag>
        </Space>
        <Checkbox
          checked={reinitialize}
          onChange={(event) => setReinitialize(event.target.checked)}
        >
          {t("删除目标 PostgreSQL 的所有表并重新初始化后台")}
        </Checkbox>
        {reinitialize ? (
          <Space direction="vertical" className="full-width">
            <Typography.Text type="danger">
              {t(
                "此操作会删除目标数据库中的全部数据；请输入 REINITIALIZE DATABASE",
              )}
            </Typography.Text>
            <Input
              value={confirmPhrase}
              onChange={(event) => setConfirmPhrase(event.target.value)}
              placeholder="REINITIALIZE DATABASE"
              style={{ maxWidth: 420 }}
            />
          </Space>
        ) : null}
      </Space>
    </Card>
  );
}
