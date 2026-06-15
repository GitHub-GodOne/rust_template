import { ApiOutlined } from "@ant-design/icons";
import { CrudPage } from "../../../components/admin/CrudPage";

export function DocsPage() {
  return (
    <CrudPage
      title="接口文档"
      subtitle="OpenAPI / Swagger"
      icon={<ApiOutlined />}
    >
      <div className="docs-frame-shell">
        <iframe className="docs-frame" src="/swagger-ui" title="接口文档" />
      </div>
    </CrudPage>
  );
}
