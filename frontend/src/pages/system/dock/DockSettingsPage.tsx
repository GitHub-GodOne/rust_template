import {
  AppstoreOutlined,
  ArrowDownOutlined,
  ArrowUpOutlined,
  DeleteOutlined,
  HolderOutlined,
  PlusOutlined,
  SaveOutlined,
  SearchOutlined,
  UndoOutlined,
} from "@ant-design/icons";
import {
  Alert,
  Button,
  Card,
  Col,
  ColorPicker,
  Empty,
  Input,
  Popconfirm,
  Row,
  Segmented,
  Select,
  Space,
  Switch,
  Tag,
  Tooltip,
  Typography,
  message,
} from "antd";
import { useEffect, useMemo, useState } from "react";
import {
  type AdminMenuItem,
  defaultAdminMenus,
  mergeMenuItems,
} from "../../../app/menu";
import { PageHeader } from "../../../components/admin/PageHeader";
import { DockBar, resolveDockItems } from "../../../layouts/dock/DockBar";
import {
  DEFAULT_DOCK_CONFIG,
  DOCK_COLORS,
  DOCK_PAGE_MODE_LABELS,
  type DockConfig,
  type DockPageMode,
  type DockPosition,
  type DockSize,
  MAX_DOCK_ITEMS,
  MAX_DOCK_PAGE_RULES,
  colorForPath,
  moveDockItem,
  sameDockConfig,
  withPageRule,
} from "../../../layouts/dock/dockConfig";
import { renderMenuIcon } from "../../../layouts/icons";
import { useAuthStore } from "../../../stores/auth";
import { useDockStore } from "../../../stores/dock";
import { useUiStore } from "../../../stores/ui";
import "./dock-settings.css";
import { t } from "../../../i18n";

function flattenPages(items: AdminMenuItem[]): AdminMenuItem[] {
  return items.flatMap((item) => [
    ...(item.path ? [item] : []),
    ...flattenPages(item.children ?? []),
  ]);
}

export function DockSettingsPage() {
  const config = useDockStore((state) => state.config);
  const save = useDockStore((state) => state.save);
  const reset = useDockStore((state) => state.reset);
  const menus = useAuthStore((state) => state.menus);
  const hasPermission = useAuthStore((state) => state.hasPermission);
  useAuthStore((state) => state.permissions);
  const dockVisible = useUiStore((state) => state.dockVisible);
  const toggleDockVisible = useUiStore((state) => state.toggleDockVisible);
  const [draft, setDraft] = useState<DockConfig>(config);
  const [saving, setSaving] = useState(false);
  const [search, setSearch] = useState("");
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);

  // Follow the stored config until the user starts editing.
  const [touched, setTouched] = useState(false);
  useEffect(() => {
    if (!touched) setDraft(config);
  }, [config, touched]);

  const update = (change: (current: DockConfig) => DockConfig) => {
    setTouched(true);
    setDraft((current) => change(current));
  };

  const visibleMenus =
    menus.length > 0
      ? mergeMenuItems(menus, defaultAdminMenus)
      : defaultAdminMenus;
  const resolved = resolveDockItems(draft.items, visibleMenus, hasPermission);
  const previewItems = resolved.filter((item) => !item.unavailable);
  const dirty = !sameDockConfig(draft, config);
  const full = draft.items.length >= MAX_DOCK_ITEMS;

  const availablePages = useMemo(() => {
    const added = new Set(draft.items.map((item) => item.path));
    const keyword = search.trim().toLowerCase();
    return flattenPages(visibleMenus).filter((page) => {
      if (!page.path || added.has(page.path)) return false;
      if (!hasPermission(page.permission ?? page.permission_code)) return false;
      if (!keyword) return true;
      return (
        page.label.toLowerCase().includes(keyword) ||
        page.path.toLowerCase().includes(keyword)
      );
    });
  }, [draft.items, hasPermission, search, visibleMenus]);

  // Every page the user can open, for per-page dock rules.
  const accessiblePages = useMemo(
    () =>
      flattenPages(visibleMenus).filter((page) =>
        hasPermission(page.permission ?? page.permission_code),
      ),
    [hasPermission, visibleMenus],
  );
  const pageByPath = new Map(
    accessiblePages.map((page) => [page.path as string, page]),
  );
  const [rulePath, setRulePath] = useState<string>();
  const [ruleMode, setRuleMode] = useState<RuleModeChoice>("autoHide");
  const [ruleReserve, setRuleReserve] = useState<RuleReserveChoice>("default");
  const ruleOptions = accessiblePages
    .filter((page) => !draft.pageRules.some((rule) => rule.path === page.path))
    .map((page) => ({
      value: page.path as string,
      label: `${page.label}（${page.path}）`,
    }));
  const pageModeOptions: { value: RuleModeChoice; label: string }[] = [
    { value: "default", label: t("默认") },
    ...(["show", "autoHide", "hide"] as const).map((mode) => ({
      value: mode,
      label: DOCK_PAGE_MODE_LABELS[mode].replace(" Dock", ""),
    })),
  ];

  const addPage = (path: string) =>
    update((current) => ({
      ...current,
      items: [...current.items, { path, color: colorForPath(path) }],
    }));

  const moveItem = (from: number, to: number) =>
    update((current) => ({
      ...current,
      items: moveDockItem(current.items, from, to),
    }));

  const handleSave = async () => {
    setSaving(true);
    try {
      await save(draft);
      setTouched(false);
      if (!dockVisible && draft.enabled) toggleDockVisible();
      message.success(t("Dock 设置已保存"));
    } catch (error) {
      message.error(error instanceof Error ? error.message : t("保存失败"));
    } finally {
      setSaving(false);
    }
  };

  const handleReset = async () => {
    try {
      await reset();
      setTouched(false);
      setDraft(DEFAULT_DOCK_CONFIG);
      message.success(t("已恢复默认 Dock"));
    } catch (error) {
      message.error(error instanceof Error ? error.message : t("恢复失败"));
    }
  };

  return (
    <div className="dock-settings">
      <PageHeader
        title={t("快捷 Dock")}
        subtitle={t(
          "选择常用页面放进 Dock，并调整它的位置、大小和行为；设置跟随账号保存",
        )}
        breadcrumb={[t("系统管理"), t("快捷 Dock")]}
        icon={<AppstoreOutlined />}
        extra={
          <Space wrap>
            <Popconfirm
              title={t("恢复默认 Dock？")}
              description={t("会清除你的自定义设置")}
              okText={t("恢复")}
              cancelText={t("取消")}
              onConfirm={handleReset}
            >
              <Button icon={<UndoOutlined />}>{t("恢复默认")}</Button>
            </Popconfirm>
            <Button
              type="primary"
              icon={<SaveOutlined />}
              loading={saving}
              disabled={!dirty}
              onClick={handleSave}
            >
              {t("保存")}
            </Button>
          </Space>
        }
      />

      <Card className="admin-card dock-settings-preview-card">
        <div
          className={`dock-settings-stage is-${draft.position}${draft.enabled ? "" : " is-disabled"}`}
        >
          <div className="dock-settings-screen" aria-hidden="true">
            <span />
            <span />
            <span />
          </div>
          {draft.enabled ? (
            previewItems.length > 0 ? (
              <DockBar
                config={draft}
                items={previewItems}
                preview
                activePath={previewItems[0]?.path}
              />
            ) : (
              <Typography.Text type="secondary" className="dock-settings-hint">
                {t("Dock 中还没有页面，从下方「添加页面」里选择")}
              </Typography.Text>
            )
          ) : (
            <Typography.Text type="secondary" className="dock-settings-hint">
              {t("Dock 已停用")}
            </Typography.Text>
          )}
        </div>
        {dirty ? (
          <Alert
            type="info"
            showIcon
            className="dock-settings-dirty"
            message={t("预览中的修改尚未保存")}
            action={
              <Button
                size="small"
                type="primary"
                loading={saving}
                onClick={handleSave}
              >
                {t("保存")}
              </Button>
            }
          />
        ) : null}
      </Card>

      <Row gutter={[16, 16]}>
        <Col xs={24} xl={14}>
          <Card
            className="admin-card"
            title={t(
              "Dock 中的页面（{0}/{1}）",
              draft.items.length,
              MAX_DOCK_ITEMS,
            )}
            extra={
              <Typography.Text type="secondary" className="dock-settings-tip">
                {t("拖动排序")}
              </Typography.Text>
            }
          >
            {resolved.length === 0 ? (
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={t("还没有页面")}
              />
            ) : (
              <ul className="dock-settings-list">
                {resolved.map((item, index) => (
                  <li
                    key={item.path}
                    draggable
                    className={`dock-settings-row${dragIndex === index ? " is-dragging" : ""}${
                      dropIndex === index && dragIndex !== index
                        ? " is-drop-target"
                        : ""
                    }`}
                    onDragStart={(event) => {
                      setDragIndex(index);
                      event.dataTransfer.effectAllowed = "move";
                    }}
                    onDragOver={(event) => {
                      event.preventDefault();
                      setDropIndex(index);
                    }}
                    onDragEnd={() => {
                      setDragIndex(null);
                      setDropIndex(null);
                    }}
                    onDrop={(event) => {
                      event.preventDefault();
                      if (dragIndex !== null) moveItem(dragIndex, index);
                      setDragIndex(null);
                      setDropIndex(null);
                    }}
                  >
                    <HolderOutlined className="dock-settings-handle" />
                    <ColorPicker
                      size="small"
                      disabledAlpha
                      value={item.color}
                      presets={[{ label: t("推荐颜色"), colors: DOCK_COLORS }]}
                      onChangeComplete={(color) =>
                        update((current) => ({
                          ...current,
                          items: current.items.map((entry) =>
                            entry.path === item.path
                              ? { ...entry, color: color.toHexString() }
                              : entry,
                          ),
                        }))
                      }
                    >
                      <button
                        type="button"
                        className="dock-settings-icon"
                        style={{ background: item.color }}
                        aria-label={t("更换 {0} 的颜色", item.label)}
                      >
                        {renderMenuIcon(item.icon)}
                      </button>
                    </ColorPicker>
                    <span className="dock-settings-name">
                      <strong>{item.label}</strong>
                      <span>{item.path}</span>
                    </span>
                    {item.unavailable ? (
                      <Tag color="orange">
                        {item.unavailable === "missing"
                          ? t("页面不存在")
                          : t("无权限，不显示")}
                      </Tag>
                    ) : null}
                    <Space size={2} className="dock-settings-actions">
                      <Tooltip title={t("上移")}>
                        <Button
                          type="text"
                          size="small"
                          icon={<ArrowUpOutlined />}
                          aria-label={t("上移 {0}", item.label)}
                          disabled={index === 0}
                          onClick={() => moveItem(index, index - 1)}
                        />
                      </Tooltip>
                      <Tooltip title={t("下移")}>
                        <Button
                          type="text"
                          size="small"
                          icon={<ArrowDownOutlined />}
                          aria-label={t("下移 {0}", item.label)}
                          disabled={index === resolved.length - 1}
                          onClick={() => moveItem(index, index + 1)}
                        />
                      </Tooltip>
                      <Tooltip title={t("从 Dock 移除")}>
                        <Button
                          type="text"
                          size="small"
                          danger
                          icon={<DeleteOutlined />}
                          aria-label={t("移除 {0}", item.label)}
                          onClick={() =>
                            update((current) => ({
                              ...current,
                              items: current.items.filter(
                                (entry) => entry.path !== item.path,
                              ),
                            }))
                          }
                        />
                      </Tooltip>
                    </Space>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </Col>
        <Col xs={24} xl={10}>
          <Space direction="vertical" size={16} className="full-width">
            <Card className="admin-card" title={t("外观与行为")}>
              <div className="dock-settings-options">
                <div className="dock-settings-option">
                  <span>
                    <strong>{t("启用 Dock")}</strong>
                    <small>{t("关闭后所有页面都不显示 Dock")}</small>
                  </span>
                  <Switch
                    checked={draft.enabled}
                    onChange={(enabled) =>
                      update((current) => ({ ...current, enabled }))
                    }
                  />
                </div>
                <div className="dock-settings-option">
                  <span>
                    <strong>{t("位置")}</strong>
                    <small>{t("手机上始终显示在底部")}</small>
                  </span>
                  <Segmented<DockPosition>
                    value={draft.position}
                    onChange={(position) =>
                      update((current) => ({ ...current, position }))
                    }
                    options={[
                      { value: "left", label: t("左侧") },
                      { value: "bottom", label: t("底部") },
                      { value: "right", label: t("右侧") },
                    ]}
                  />
                </div>
                <div className="dock-settings-option">
                  <span>
                    <strong>{t("图标大小")}</strong>
                  </span>
                  <Segmented<DockSize>
                    value={draft.size}
                    onChange={(size) =>
                      update((current) => ({ ...current, size }))
                    }
                    options={[
                      { value: "small", label: t("小") },
                      { value: "medium", label: t("中") },
                      { value: "large", label: t("大") },
                    ]}
                  />
                </div>
                <div className="dock-settings-option">
                  <span>
                    <strong>{t("悬停放大")}</strong>
                    <small>{t("鼠标指向图标时放大")}</small>
                  </span>
                  <Switch
                    checked={draft.magnify}
                    onChange={(magnify) =>
                      update((current) => ({ ...current, magnify }))
                    }
                  />
                </div>
                <div className="dock-settings-option">
                  <span>
                    <strong>{t("默认自动隐藏")}</strong>
                    <small>
                      {t("移到屏幕边缘时再显示；没有单独设置的页面使用此行为")}
                    </small>
                  </span>
                  <Switch
                    checked={draft.autoHide}
                    onChange={(autoHide) =>
                      update((current) => ({ ...current, autoHide }))
                    }
                  />
                </div>
                <div className="dock-settings-option">
                  <span>
                    <strong>{t("为 Dock 预留空间")}</strong>
                    <small>
                      {t(
                        "常驻显示时在页面边缘留出空白，避免遮挡内容；关闭后 Dock 浮在内容上方",
                      )}
                    </small>
                  </span>
                  <Switch
                    checked={draft.reserveSpace}
                    onChange={(reserveSpace) =>
                      update((current) => ({ ...current, reserveSpace }))
                    }
                  />
                </div>
                <div className="dock-settings-option">
                  <span>
                    <strong>{t("显示名称")}</strong>
                    <small>{t("在图标下方显示页面名称")}</small>
                  </span>
                  <Switch
                    checked={draft.showLabels}
                    onChange={(showLabels) =>
                      update((current) => ({ ...current, showLabels }))
                    }
                  />
                </div>
              </div>
            </Card>
            <Card
              className="admin-card"
              title={t("添加页面")}
              extra={
                full ? (
                  <Typography.Text type="warning">
                    {t("最多 {0} 个", MAX_DOCK_ITEMS)}
                  </Typography.Text>
                ) : null
              }
            >
              <Input
                allowClear
                prefix={<SearchOutlined />}
                placeholder={t("搜索页面名称或路径")}
                value={search}
                onChange={(event) => setSearch(event.target.value)}
              />
              {availablePages.length === 0 ? (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description={
                    search ? t("没有匹配的页面") : t("可用页面都已加入 Dock")
                  }
                />
              ) : (
                <ul className="dock-settings-pages">
                  {availablePages.map((page) => (
                    <li key={page.path}>
                      <span
                        className="dock-settings-icon is-small"
                        style={{ background: colorForPath(page.path ?? "") }}
                      >
                        {renderMenuIcon(page.icon)}
                      </span>
                      <span className="dock-settings-name">
                        <strong>{page.label}</strong>
                        <span>{page.path}</span>
                      </span>
                      <Button
                        size="small"
                        icon={<PlusOutlined />}
                        disabled={full}
                        aria-label={t("添加 {0}", page.label)}
                        onClick={() => page.path && addPage(page.path)}
                      >
                        {t("添加")}
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </Card>
          </Space>
        </Col>
      </Row>

      <Card
        className="admin-card"
        title={t("按页面设置（{0}）", draft.pageRules.length)}
        extra={
          <Typography.Text type="secondary" className="dock-settings-tip">
            {t("也可以在任意页面右键 Dock →「当前页面」")}
          </Typography.Text>
        }
      >
        <div className="dock-rule-add">
          <Select
            showSearch
            allowClear
            placeholder={t("选择一个菜单页面")}
            className="dock-rule-select"
            value={rulePath}
            options={ruleOptions}
            optionFilterProp="label"
            onChange={setRulePath}
          />
          <RuleControls
            mode={ruleMode}
            reserve={ruleReserve}
            defaultReserve={draft.reserveSpace}
            modeOptions={pageModeOptions}
            onModeChange={setRuleMode}
            onReserveChange={setRuleReserve}
          />
          <Button
            type="primary"
            icon={<PlusOutlined />}
            disabled={
              !rulePath ||
              draft.pageRules.length >= MAX_DOCK_PAGE_RULES ||
              (ruleMode === "default" && ruleReserve === "default")
            }
            onClick={() => {
              if (!rulePath) return;
              update((current) =>
                withPageRule(current, rulePath, {
                  mode: modeFromChoice(ruleMode),
                  reserve: reserveFromChoice(ruleReserve),
                }),
              );
              setRulePath(undefined);
            }}
          >
            {t("添加")}
          </Button>
        </div>
        {draft.pageRules.length === 0 ? (
          <Typography.Paragraph type="secondary" className="dock-rule-empty">
            {t(
              "所有页面都使用默认行为（{0}，{1}）。例如可以让 SSH 管理常驻显示但不预留空间，终端占满整个高度。",
              draft.autoHide ? t("自动隐藏") : t("常驻显示"),
              draft.reserveSpace ? t("预留空间") : t("不预留空间"),
            )}
          </Typography.Paragraph>
        ) : (
          <ul className="dock-settings-list">
            {draft.pageRules.map((rule) => {
              const page = pageByPath.get(rule.path);
              return (
                <li key={rule.path} className="dock-settings-row dock-rule-row">
                  <span
                    className="dock-settings-icon is-small"
                    style={{ background: colorForPath(rule.path) }}
                  >
                    {renderMenuIcon(page?.icon)}
                  </span>
                  <span className="dock-settings-name">
                    <strong>{page?.label ?? rule.path}</strong>
                    <span>{rule.path}</span>
                  </span>
                  {page ? null : <Tag color="orange">{t("页面不可用")}</Tag>}
                  <RuleControls
                    size="small"
                    mode={rule.mode ?? "default"}
                    reserve={reserveChoice(rule.reserve)}
                    defaultReserve={draft.reserveSpace}
                    modeOptions={pageModeOptions}
                    onModeChange={(mode) =>
                      update((current) =>
                        withPageRule(current, rule.path, {
                          mode: modeFromChoice(mode),
                        }),
                      )
                    }
                    onReserveChange={(reserve) =>
                      update((current) =>
                        withPageRule(current, rule.path, {
                          reserve: reserveFromChoice(reserve),
                        }),
                      )
                    }
                  />
                  <Tooltip title={t("删除规则，恢复默认")}>
                    <Button
                      type="text"
                      size="small"
                      danger
                      icon={<DeleteOutlined />}
                      aria-label={t(
                        "删除 {0} 的规则",
                        page?.label ?? rule.path,
                      )}
                      onClick={() =>
                        update((current) =>
                          withPageRule(current, rule.path, null),
                        )
                      }
                    />
                  </Tooltip>
                </li>
              );
            })}
          </ul>
        )}
      </Card>
    </div>
  );
}

type RuleModeChoice = DockPageMode | "default";
type RuleReserveChoice = "default" | "on" | "off";

const modeFromChoice = (choice: RuleModeChoice) =>
  choice === "default" ? null : choice;
const reserveFromChoice = (choice: RuleReserveChoice) =>
  choice === "default" ? null : choice === "on";
const reserveChoice = (reserve?: boolean): RuleReserveChoice =>
  reserve === undefined ? "default" : reserve ? "on" : "off";

/** Display mode plus reserve-space choice for one page rule. */
function RuleControls({
  mode,
  reserve,
  defaultReserve,
  modeOptions,
  size,
  onModeChange,
  onReserveChange,
}: {
  mode: RuleModeChoice;
  reserve: RuleReserveChoice;
  defaultReserve: boolean;
  modeOptions: { value: RuleModeChoice; label: string }[];
  size?: "small";
  onModeChange: (mode: RuleModeChoice) => void;
  onReserveChange: (reserve: RuleReserveChoice) => void;
}) {
  return (
    <span className="dock-rule-controls">
      <Segmented<RuleModeChoice>
        size={size}
        value={mode}
        options={modeOptions}
        onChange={onModeChange}
      />
      <Tooltip title={t("Dock 常驻显示时，是否在页面边缘留出空白")}>
        <Segmented<RuleReserveChoice>
          size={size}
          value={reserve}
          options={[
            {
              value: "default",
              label: t("默认（{0}）", defaultReserve ? t("预留") : t("不预留")),
            },
            { value: "on", label: t("预留") },
            { value: "off", label: t("不预留") },
          ]}
          onChange={onReserveChange}
        />
      </Tooltip>
    </span>
  );
}
