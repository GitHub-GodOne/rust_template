import {
  ApiOutlined,
  DatabaseOutlined,
  SafetyCertificateOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { ArrowRightOutlined } from "@ant-design/icons";
import { Button, Card, Col, List, Row, Tag, Timeline, Typography } from "antd";
import { PageHeader } from "../../../components/admin/PageHeader";
import { WidgetCard } from "../../../components/admin/WidgetCard";

const modules = [
  { name: "用户管理", status: "页面模板已就绪", color: "processing" },
  { name: "角色权限", status: "等待后端 RBAC 接口", color: "warning" },
  { name: "Swagger 文档", status: "待接入 utoipa", color: "default" },
  { name: "二进制嵌入", status: "待接入 embedded assets", color: "default" },
  { name: "日志与通知", status: "预留模板页", color: "default" },
];

export function DashboardPage() {
  return (
    <div>
      <PageHeader
        title="早上好，欢迎回来"
        subtitle="这里是系统今日的运行概览"
        breadcrumb={["首页", "控制台"]}
        icon={<ApiOutlined />}
      />
      <div className="dashboard-stat-strip">
        <Row gutter={[0, 0]}>
          <Col xs={24} sm={12} xl={6}>
            <WidgetCard
              title="用户体系"
              value="JWT"
              prefix={<UserOutlined />}
            />
          </Col>
          <Col xs={24} sm={12} xl={6}>
            <WidgetCard
              title="权限模型"
              value="RBAC"
              prefix={<SafetyCertificateOutlined />}
              tone="green"
            />
          </Col>
          <Col xs={24} sm={12} xl={6}>
            <WidgetCard
              title="接口文档"
              value="OpenAPI"
              prefix={<ApiOutlined />}
              tone="orange"
            />
          </Col>
          <Col xs={24} sm={12} xl={6}>
            <WidgetCard
              title="部署模式"
              value="Single Binary"
              prefix={<DatabaseOutlined />}
              tone="red"
            />
          </Col>
        </Row>
      </div>
      <Row gutter={[16, 16]} className="dashboard-main-row">
        <Col xs={24} lg={14}>
          <Card
            title="系统模块"
            className="admin-card dashboard-panel"
            extra={
              <Button
                type="text"
                icon={<ArrowRightOutlined />}
                aria-label="查看全部模块"
              />
            }
          >
            <List
              dataSource={modules}
              renderItem={(item) => (
                <List.Item>
                  <List.Item.Meta title={item.name} description={item.status} />
                  <Tag color={item.color}>{item.status}</Tag>
                </List.Item>
              )}
            />
          </Card>
        </Col>
        <Col xs={24} lg={10}>
          <Card title="近期计划" className="admin-card dashboard-panel">
            <Timeline
              items={[
                {
                  children: (
                    <>
                      <Typography.Text strong>
                        后台外壳与登录流程
                      </Typography.Text>
                      <br />
                      <Typography.Text type="secondary">
                        基础访问与身份验证
                      </Typography.Text>
                    </>
                  ),
                },
                { children: "刷新令牌、退出登录、统一错误响应" },
                { children: "用户、角色、菜单、按钮权限" },
                { children: "Swagger/OpenAPI 与二进制嵌入" },
                { children: "上传、日志、通知、设置、监控扩展模块" },
              ]}
            />
          </Card>
        </Col>
      </Row>
    </div>
  );
}
