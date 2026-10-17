import {
  AppstoreOutlined,
  BellOutlined,
  CheckCircleFilled,
  CloseCircleFilled,
  CreditCardOutlined,
  ExclamationCircleFilled,
  FileSearchOutlined,
  InfoCircleFilled,
  ProfileOutlined,
  ReloadOutlined,
  RightOutlined,
  UserOutlined,
  WarningFilled,
} from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Row,
  Skeleton,
  Space,
  Tag,
  Typography,
} from "antd";
import type { ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import {
  type DashboardOverview,
  type DashboardTodo,
  fetchDashboardOverview,
} from "../../api/admin/dashboard";
import { defaultAdminMenus, mergeMenuItems } from "../../app/menu";
import { renderMenuIcon } from "../../layouts/icons";
import { useAuthStore } from "../../stores/auth";
import { ActivityTrend } from "./ActivityTrend";
import {
  formatDateTime,
  formatRelativeTime,
  formatToday,
  formatUptime,
  greeting,
  isOverdue,
  levelLabels,
  pendingTodoTotal,
  pickQuickEntries,
  sortTodos,
  workOrderPriorityColors,
  workOrderPriorityLabels,
  workOrderStatusLabels,
} from "./homeOverview";
import "./home.css";

const REFRESH_INTERVAL_MS = 60_000;

type Kpi = {
  key: string;
  label: string;
  value: number;
  hint: string;
  icon: ReactNode;
  path: string;
  alert?: boolean;
};

function buildKpis(stats: DashboardOverview["stats"]): Kpi[] {
  const kpis: Kpi[] = [];
  if (stats.users) {
    kpis.push({
      key: "users",
      label: "用户总数",
      value: stats.users.total,
      hint: `今日新增 ${stats.users.new_today}`,
      icon: <UserOutlined />,
      path: "/admin/system/users",
    });
  }
  if (stats.work_orders) {
    kpis.push({
      key: "work_orders",
      label: "处理中工单",
      value: stats.work_orders.active,
      hint: `已逾期 ${stats.work_orders.overdue}`,
      icon: <ProfileOutlined />,
      path: "/admin/system/work-orders",
      alert: stats.work_orders.overdue > 0,
    });
  }
  if (stats.payments) {
    kpis.push({
      key: "payments",
      label: "今日订单",
      value: stats.payments.today_orders,
      hint: `已支付 ${stats.payments.today_paid} · 待支付 ${stats.payments.pending}`,
      icon: <CreditCardOutlined />,
      path: "/admin/system/payments",
    });
  }
  if (stats.notifications) {
    kpis.push({
      key: "notifications",
      label: "未读通知",
      value: stats.notifications.unread,
      hint: `通知总数 ${stats.notifications.total}`,
      icon: <BellOutlined />,
      path: "/admin/system/notifications",
    });
  }
  if (stats.activity) {
    kpis.push({
      key: "activity",
      label: "今日操作",
      value: stats.activity.today_operations,
      hint: `登录 ${stats.activity.today_logins} · 错误 ${stats.activity.today_errors}`,
      icon: <FileSearchOutlined />,
      path: "/admin/system/logs",
      alert: stats.activity.today_errors > 0,
    });
  }
  return kpis;
}

const todoIcons: Record<string, ReactNode> = {
  error: <ExclamationCircleFilled />,
  warning: <WarningFilled />,
  info: <InfoCircleFilled />,
};

function TodoList({ todos }: { todos: DashboardTodo[] }) {
  const navigate = useNavigate();
  if (todos.length === 0) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description="当前角色暂无需要关注的待办"
      />
    );
  }

  return (
    <ul className="home-todo-list">
      {sortTodos(todos).map((todo) => {
        const done = todo.count === 0;
        return (
          <li key={todo.key}>
            <button
              type="button"
              className={`home-todo-item is-${todo.level}${done ? " is-done" : ""}`}
              onClick={() => navigate(todo.path)}
            >
              <span className="home-todo-icon" aria-hidden="true">
                {done ? <CheckCircleFilled /> : todoIcons[todo.level]}
              </span>
              <span className="home-todo-text">
                <strong>{todo.title}</strong>
                <small>{todo.description}</small>
              </span>
              <span className="home-todo-count">
                {done ? "已清零" : todo.count.toLocaleString()}
              </span>
              <RightOutlined className="home-todo-arrow" aria-hidden="true" />
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function SectionCard({
  title,
  extra,
  children,
  className,
}: {
  title: ReactNode;
  extra?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <Card
      title={title}
      extra={extra}
      className={`admin-card home-card${className ? ` ${className}` : ""}`}
    >
      {children}
    </Card>
  );
}

function MoreLink({
  path,
  label = "查看全部",
}: { path: string; label?: string }) {
  const navigate = useNavigate();
  return (
    <Button type="link" size="small" onClick={() => navigate(path)}>
      {label}
    </Button>
  );
}

function HomeSkeleton() {
  return (
    <div className="home-page" aria-busy="true">
      <Card className="admin-card home-welcome">
        <Skeleton active paragraph={{ rows: 1 }} />
      </Card>
      <div className="home-kpi-grid">
        {["a", "b", "c", "d"].map((key) => (
          <Card key={key} className="admin-card">
            <Skeleton active paragraph={{ rows: 1 }} title={false} />
          </Card>
        ))}
      </div>
      <Row gutter={[16, 16]}>
        <Col xs={24} xl={16}>
          <Card className="admin-card">
            <Skeleton active paragraph={{ rows: 6 }} />
          </Card>
        </Col>
        <Col xs={24} xl={8}>
          <Card className="admin-card">
            <Skeleton active paragraph={{ rows: 6 }} />
          </Card>
        </Col>
      </Row>
    </div>
  );
}

export function HomePage() {
  const navigate = useNavigate();
  const user = useAuthStore((state) => state.user);
  const roles = useAuthStore((state) => state.roles);
  const tenant = useAuthStore((state) => state.tenant);
  const currentDepartment = useAuthStore((state) => state.currentDepartment);
  const menus = useAuthStore((state) => state.menus);
  const hasPermission = useAuthStore((state) => state.hasPermission);
  // Subscribe so shortcuts re-filter once the session permissions arrive.
  useAuthStore((state) => state.permissions);

  const overviewQuery = useQuery({
    queryKey: ["admin-dashboard-overview"],
    queryFn: fetchDashboardOverview,
    refetchInterval: REFRESH_INTERVAL_MS,
  });

  const quickEntries = pickQuickEntries(
    menus.length > 0
      ? mergeMenuItems(menus, defaultAdminMenus)
      : defaultAdminMenus,
    hasPermission,
  );

  if (overviewQuery.isPending) {
    return <HomeSkeleton />;
  }

  if (overviewQuery.isError) {
    return (
      <div className="home-page">
        <Alert
          type="error"
          showIcon
          message="首页数据加载失败"
          description={overviewQuery.error.message}
          action={
            <Button size="small" onClick={() => overviewQuery.refetch()}>
              重试
            </Button>
          }
        />
      </div>
    );
  }

  const overview = overviewQuery.data;
  const now = new Date();
  const kpis = buildKpis(overview.stats);
  const pendingTotal = pendingTodoTotal(overview.todos);
  const previousLogin = overview.recent_logins.filter(
    (item) => item.success,
  )[1];
  const listCards: { key: string; node: ReactNode }[] = [];
  if (overview.my_work_orders) {
    listCards.push({
      key: "work-orders",
      node: (
        <SectionCard
          title="我的工单"
          extra={<MoreLink path="/admin/system/work-orders" />}
        >
          {overview.my_work_orders.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description="没有进行中的工单"
            />
          ) : (
            <ul className="home-feed">
              {overview.my_work_orders.map((item) => (
                <li key={item.id}>
                  <div className="home-feed-main">
                    <Typography.Text strong ellipsis={{ tooltip: item.title }}>
                      {item.title}
                    </Typography.Text>
                    <span className="home-feed-meta">
                      {item.order_no} ·{" "}
                      {workOrderStatusLabels[item.status] ?? item.status}
                      {isOverdue(item.due_at, now) && (
                        <Tag color="red" className="home-inline-tag">
                          已逾期
                        </Tag>
                      )}
                    </span>
                  </div>
                  <Tag
                    color={workOrderPriorityColors[item.priority] ?? "default"}
                  >
                    {workOrderPriorityLabels[item.priority] ?? item.priority}
                  </Tag>
                </li>
              ))}
            </ul>
          )}
        </SectionCard>
      ),
    });
  }
  if (overview.recent_notifications) {
    listCards.push({
      key: "notifications",
      node: (
        <SectionCard
          title="最新通知"
          extra={<MoreLink path="/admin/system/notifications" />}
        >
          {overview.recent_notifications.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description="暂无通知"
            />
          ) : (
            <ul className="home-feed">
              {overview.recent_notifications.map((item) => (
                <li key={item.id}>
                  <span
                    className={`home-feed-dot is-${item.level}`}
                    aria-hidden="true"
                  />
                  <div className="home-feed-main">
                    <Typography.Text
                      strong={!item.read}
                      ellipsis={{ tooltip: item.title }}
                    >
                      {item.title}
                    </Typography.Text>
                    <span className="home-feed-meta">
                      {levelLabels[item.level] ?? item.level} ·{" "}
                      {formatRelativeTime(item.created_at, now)}
                    </span>
                  </div>
                  {!item.read && <span className="home-unread">未读</span>}
                </li>
              ))}
            </ul>
          )}
        </SectionCard>
      ),
    });
  }
  if (overview.recent_logs) {
    listCards.push({
      key: "logs",
      node: (
        <SectionCard
          title="最近操作"
          extra={<MoreLink path="/admin/system/logs" />}
        >
          {overview.recent_logs.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description="暂无日志"
            />
          ) : (
            <ul className="home-timeline">
              {overview.recent_logs.map((item) => (
                <li key={item.id} className={`is-${item.level}`}>
                  <div className="home-feed-main">
                    <Typography.Text ellipsis={{ tooltip: item.message }}>
                      {item.message}
                    </Typography.Text>
                    <span className="home-feed-meta">
                      {item.operator ?? "系统"} · {item.module}/{item.action} ·{" "}
                      {formatRelativeTime(item.created_at, now)}
                    </span>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </SectionCard>
      ),
    });
  }
  const listSpan = 24 / Math.max(listCards.length, 1);

  return (
    <div className="home-page">
      <Card className="admin-card home-welcome">
        <div className="home-welcome-body">
          <div className="home-welcome-main">
            <Typography.Title level={3} className="home-welcome-title">
              {greeting(now.getHours())}，{user?.name ?? "管理员"}
            </Typography.Title>
            <div className="home-welcome-meta">
              <span className="home-meta-item">{formatToday(now)}</span>
              {tenant && <span className="home-meta-item">{tenant.name}</span>}
              {currentDepartment && (
                <span className="home-meta-item">{currentDepartment.name}</span>
              )}
              {roles.length > 0 && (
                <span className="home-meta-roles">
                  {roles.map((role) => (
                    <Tag key={role.id} className="home-role-tag">
                      {role.name}
                    </Tag>
                  ))}
                </span>
              )}
            </div>
          </div>
          <div className="home-welcome-side">
            <span className="home-welcome-pending">
              {pendingTotal > 0 ? (
                <>
                  当前有 <strong>{pendingTotal.toLocaleString()}</strong>{" "}
                  项事务待处理
                </>
              ) : (
                "所有事务均已处理"
              )}
            </span>
            <Space size={8} wrap className="home-welcome-actions">
              <Typography.Text type="secondary" className="home-welcome-time">
                {previousLogin
                  ? `上次登录 ${formatDateTime(previousLogin.created_at)}`
                  : `更新于 ${formatDateTime(overview.generated_at)}`}
              </Typography.Text>
              <Button
                icon={<ReloadOutlined spin={overviewQuery.isFetching} />}
                onClick={() => overviewQuery.refetch()}
                aria-label="刷新首页数据"
              >
                刷新
              </Button>
            </Space>
          </div>
        </div>
      </Card>

      {kpis.length > 0 && (
        <div className="home-kpi-grid">
          {kpis.map((kpi) => (
            <button
              key={kpi.key}
              type="button"
              className="home-kpi"
              onClick={() => navigate(kpi.path)}
            >
              <span className="home-kpi-head">
                <span className="home-kpi-icon" aria-hidden="true">
                  {kpi.icon}
                </span>
                {kpi.label}
              </span>
              <span className="home-kpi-value">
                {kpi.value.toLocaleString()}
              </span>
              <span className={`home-kpi-hint${kpi.alert ? " is-alert" : ""}`}>
                {kpi.hint}
              </span>
            </button>
          ))}
        </div>
      )}

      <Row gutter={[16, 16]} className="home-row">
        <Col xs={24} xl={quickEntries.length > 0 ? 16 : 24}>
          <SectionCard
            title="待办事项"
            extra={
              pendingTotal > 0 ? (
                <Tag color="orange">
                  {pendingTotal.toLocaleString()} 项待处理
                </Tag>
              ) : (
                <Tag color="green">已全部处理</Tag>
              )
            }
          >
            <TodoList todos={overview.todos} />
          </SectionCard>
        </Col>
        {quickEntries.length > 0 && (
          <Col xs={24} xl={8}>
            <SectionCard
              title={
                <span>
                  <AppstoreOutlined /> 快捷入口
                </span>
              }
            >
              <div className="home-shortcuts">
                {quickEntries.map((entry) => (
                  <button
                    key={entry.path}
                    type="button"
                    className="home-shortcut"
                    onClick={() => entry.path && navigate(entry.path)}
                  >
                    <span className="home-shortcut-icon" aria-hidden="true">
                      {renderMenuIcon(entry.icon)}
                    </span>
                    <span className="home-shortcut-label">
                      {entry.label ?? entry.title}
                    </span>
                  </button>
                ))}
              </div>
            </SectionCard>
          </Col>
        )}
      </Row>

      <Row gutter={[16, 16]} className="home-row">
        {overview.activity_trend && (
          <Col xs={24} xl={16}>
            <SectionCard
              title="近 7 日系统活动"
              extra={<MoreLink path="/admin/system/logs" label="日志中心" />}
            >
              <ActivityTrend points={overview.activity_trend} />
            </SectionCard>
          </Col>
        )}
        <Col xs={24} xl={overview.activity_trend ? 8 : 24}>
          <SectionCard title="系统信息">
            <dl className="home-system-list">
              <div>
                <dt>数据库</dt>
                <dd>
                  {overview.system.db_ok ? (
                    <span className="home-status is-ok">
                      <CheckCircleFilled /> 连接正常
                    </span>
                  ) : (
                    <span className="home-status is-error">
                      <CloseCircleFilled /> 连接异常
                    </span>
                  )}
                </dd>
              </div>
              <div>
                <dt>系统版本</dt>
                <dd>v{overview.system.version}</dd>
              </div>
              <div>
                <dt>运行环境</dt>
                <dd>{overview.system.environment}</dd>
              </div>
              <div>
                <dt>服务已运行</dt>
                <dd>{formatUptime(overview.system.uptime_seconds)}</dd>
              </div>
              <div>
                <dt>数据更新</dt>
                <dd>{formatDateTime(overview.generated_at)}</dd>
              </div>
            </dl>
            <Typography.Text type="secondary" className="home-subheading">
              我的登录记录
            </Typography.Text>
            {overview.recent_logins.length === 0 ? (
              <Typography.Text type="secondary">暂无记录</Typography.Text>
            ) : (
              <ul className="home-login-list">
                {overview.recent_logins.map((login, index) => (
                  <li key={`${login.created_at}-${index}`}>
                    <span
                      className={`home-status ${login.success ? "is-ok" : "is-error"}`}
                    >
                      {login.success ? (
                        <CheckCircleFilled />
                      ) : (
                        <CloseCircleFilled />
                      )}
                      {login.success ? "登录成功" : "登录失败"}
                    </span>
                    <span className="home-feed-meta">
                      {formatDateTime(login.created_at)}
                      {login.ip ? ` · ${login.ip}` : ""}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </SectionCard>
        </Col>
      </Row>

      {listCards.length > 0 && (
        <Row gutter={[16, 16]} className="home-row">
          {listCards.map((card) => (
            <Col key={card.key} xs={24} xl={listSpan}>
              {card.node}
            </Col>
          ))}
        </Row>
      )}

      <p className="home-footnote">
        首页数据每分钟自动刷新，统计范围遵循当前账号的角色权限与数据范围。
      </p>
    </div>
  );
}
