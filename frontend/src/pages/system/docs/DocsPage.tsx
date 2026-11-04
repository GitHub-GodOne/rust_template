import { ApiOutlined } from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import { Result, Spin } from "antd";
import { type ApiResponse, unwrap } from "../../../api/auth";
import { apiClient } from "../../../api/client";
import { CrudPage } from "../../../components/admin/CrudPage";
import { t } from "../../../i18n";

type DocsTicket = {
  mode: "public" | "login" | "disabled";
  ticket?: string | null;
};

async function fetchDocsTicket() {
  const response =
    await apiClient.post<ApiResponse<DocsTicket>>("/admin/docs/ticket");
  return unwrap(response.data);
}

export function DocsPage() {
  // The iframe cannot send the bearer token, so it gets a short-lived ticket.
  const ticketQuery = useQuery({
    queryKey: ["docs-ticket"],
    queryFn: fetchDocsTicket,
    staleTime: 5 * 60_000,
  });
  const ticket = ticketQuery.data;

  return (
    <CrudPage
      title={t("接口文档")}
      subtitle="OpenAPI / Swagger"
      icon={<ApiOutlined />}
    >
      <div className="docs-frame-shell">
        {ticketQuery.isLoading ? (
          <Spin />
        ) : ticket?.mode === "disabled" ? (
          <Result
            status="info"
            title={t("接口文档已关闭")}
            subTitle={t("可在 系统设置 → security.api_docs_access 中重新开启")}
          />
        ) : ticket ? (
          <iframe
            className="docs-frame"
            src={
              ticket.ticket
                ? `/swagger-ui?ticket=${encodeURIComponent(ticket.ticket)}`
                : "/swagger-ui"
            }
            title={t("接口文档")}
          />
        ) : null}
      </div>
    </CrudPage>
  );
}
