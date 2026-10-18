import { SearchOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Drawer,
  Empty,
  Grid,
  Input,
  Space,
  Spin,
  Switch,
  Tooltip,
  Typography,
  message,
} from "antd";
import { useEffect, useMemo, useState } from "react";
import { fetchAuditSettings, saveAuditSettings } from "../../../api/admin/logs";
import { RetentionPanel } from "./RetentionPanel";
import {
  type AuditDraft,
  countRecording,
  draftFrom,
  groupFeatures,
  resetToDefaults,
  sameDraft,
  setAll,
  setRule,
  toPayload,
} from "./auditRules";
import "./logs.css";
import { t } from "../../../i18n";

export function AuditRulesDrawer({
  open,
  canEdit,
  canPurge,
  onClose,
}: {
  open: boolean;
  canEdit: boolean;
  canPurge: boolean;
  onClose: () => void;
}) {
  const screens = Grid.useBreakpoint();
  const queryClient = useQueryClient();
  const [keyword, setKeyword] = useState("");
  const [draft, setDraft] = useState<AuditDraft>();

  const settingsQuery = useQuery({
    queryKey: ["audit-settings"],
    queryFn: fetchAuditSettings,
    enabled: open,
  });
  const saved = useMemo(
    () => (settingsQuery.data ? draftFrom(settingsQuery.data) : undefined),
    [settingsQuery.data],
  );
  useEffect(() => {
    if (open && saved) setDraft(saved);
  }, [open, saved]);

  const save = useMutation({
    mutationFn: () => saveAuditSettings(toPayload(draft as AuditDraft)),
    onSuccess: (settings) => {
      queryClient.setQueryData(["audit-settings"], settings);
      message.success(t("日志记录设置已保存"));
    },
    onError: (error) =>
      message.error(error instanceof Error ? error.message : t("保存失败")),
  });

  const features = settingsQuery.data?.features ?? [];
  const groups = groupFeatures(features, keyword);
  const visibleKeys = groups.flatMap((group) =>
    group.features.map((feature) => feature.key),
  );
  const dirty = Boolean(draft && saved && !sameDraft(draft, saved));
  const locked = !canEdit || !draft?.enabled;

  return (
    <Drawer
      title={t("日志记录设置")}
      open={open}
      onClose={onClose}
      width={screens.md === false ? "100%" : 720}
      className="audit-drawer"
      footer={
        canEdit ? (
          <div className="audit-drawer-footer">
            <Typography.Text type="secondary">
              {dirty ? t("有未保存的修改") : t("设置已保存")}
            </Typography.Text>
            <Space>
              <Button
                disabled={!dirty}
                onClick={() => saved && setDraft(saved)}
              >
                {t("撤销")}
              </Button>
              <Button
                type="primary"
                disabled={!dirty}
                loading={save.isPending}
                onClick={() => save.mutate()}
              >
                {t("保存")}
              </Button>
            </Space>
          </div>
        ) : null
      }
    >
      {settingsQuery.isLoading || !draft ? (
        <div className="audit-loading">
          <Spin />
        </div>
      ) : (
        <>
          <RetentionPanel canEdit={canEdit} canPurge={canPurge} />
          <div className="audit-global">
            <div>
              <strong>{t("启用日志记录")}</strong>
              <Typography.Paragraph type="secondary">
                {t(
                  "关闭后所有功能都不再写入日志（已有日志保留）。默认只记录新增、修改、删除和执行等写操作； 查询类请求可以按功能单独开启。请求内容中的密码、令牌等字段会自动打码。",
                )}
              </Typography.Paragraph>
            </div>
            <Switch
              checked={draft.enabled}
              disabled={!canEdit}
              onChange={(enabled) => setDraft({ ...draft, enabled })}
            />
          </div>
          {!canEdit ? (
            <Alert
              type="info"
              showIcon
              className="audit-alert"
              message={t("你可以查看设置；修改需要「日志记录设置」权限")}
            />
          ) : null}

          <div className="audit-toolbar">
            <Input
              allowClear
              prefix={<SearchOutlined />}
              placeholder={t("搜索功能或接口路径")}
              value={keyword}
              onChange={(event) => setKeyword(event.target.value)}
            />
            <Space wrap size={6}>
              <Button
                size="small"
                disabled={locked}
                onClick={() =>
                  setDraft(setAll(draft, { enabled: true }, visibleKeys))
                }
              >
                {t("全部记录")}
              </Button>
              <Button
                size="small"
                disabled={locked}
                onClick={() =>
                  setDraft(setAll(draft, { enabled: false }, visibleKeys))
                }
              >
                {t("全部关闭")}
              </Button>
              <Button
                size="small"
                disabled={!canEdit}
                onClick={() => setDraft(resetToDefaults(draft, features))}
              >
                {t("恢复默认")}
              </Button>
            </Space>
          </div>
          <Typography.Text type="secondary" className="audit-summary">
            {t(
              "{0} / {1} 个功能正在记录",
              countRecording(draft),
              features.length,
            )}
          </Typography.Text>

          {groups.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={t("没有匹配的功能")}
            />
          ) : (
            groups.map((group) => (
              <section key={group.group} className="audit-group">
                <div className="audit-group-title">{group.group}</div>
                <div className="audit-head" aria-hidden="true">
                  <span>{t("功能")}</span>
                  <span>{t("记录")}</span>
                  <span>{t("含查询")}</span>
                  <span>{t("请求内容")}</span>
                </div>
                {group.features.map((feature) => {
                  const rule = draft.rules[feature.key];
                  const off = locked || !rule.enabled;
                  return (
                    <div key={feature.key} className="audit-row">
                      <div className="audit-name">
                        <strong>{feature.label}</strong>
                        <Tooltip
                          title={
                            feature.prefixes.length
                              ? feature.prefixes.join("\n")
                              : t("未归属任何功能的接口")
                          }
                        >
                          <code>
                            {feature.prefixes[0] ?? feature.key}
                            {feature.prefixes.length > 1
                              ? t(" 等 {0} 组", feature.prefixes.length)
                              : ""}
                          </code>
                        </Tooltip>
                      </div>
                      <span className="audit-switch">
                        <span>{t("记录")}</span>
                        <Switch
                          size="small"
                          checked={rule.enabled}
                          aria-label={t("{0}：记录", feature.label)}
                          disabled={locked}
                          onChange={(enabled) =>
                            setDraft(setRule(draft, feature.key, { enabled }))
                          }
                        />
                      </span>
                      <span className="audit-switch">
                        <span>{t("含查询")}</span>
                        <Switch
                          size="small"
                          checked={rule.log_reads}
                          aria-label={t("{0}：含查询", feature.label)}
                          disabled={off}
                          onChange={(log_reads) =>
                            setDraft(setRule(draft, feature.key, { log_reads }))
                          }
                        />
                      </span>
                      <span className="audit-switch">
                        <span>{t("请求内容")}</span>
                        <Switch
                          size="small"
                          checked={rule.log_body}
                          aria-label={t("{0}：请求内容", feature.label)}
                          disabled={off}
                          onChange={(log_body) =>
                            setDraft(setRule(draft, feature.key, { log_body }))
                          }
                        />
                      </span>
                    </div>
                  );
                })}
              </section>
            ))
          )}
        </>
      )}
    </Drawer>
  );
}
