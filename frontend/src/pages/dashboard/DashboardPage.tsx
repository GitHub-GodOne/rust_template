import {
  ApiOutlined,
  DatabaseOutlined,
  SafetyCertificateOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { Card, Col, List, Row, Tag, Timeline } from "antd";
import { PageHeader } from "../../components/admin/PageHeader";
import { WidgetCard } from "../../components/admin/WidgetCard";

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
        title="控制台"
        subtitle="面向开箱即用后台模板的总览页"
        breadcrumb={["首页", "控制台"]}
        icon={<ApiOutlined />}
      />
      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} xl={6}>
          <WidgetCard title="用户体系" value="JWT" prefix={<UserOutlined />} />
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
      <Row gutter={[16, 16]} className="dashboard-main-row">
        <Col xs={24} lg={14}>
          <Card title="模板模块进度" className="admin-card">
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
          <Card title="推荐实施顺序" className="admin-card">
            <Timeline
              items={[
                { children: "后台外壳与登录流程" },
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
