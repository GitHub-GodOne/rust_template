import { Button, Result } from "antd";
import { Component, type ReactNode } from "react";
import { t } from "../i18n";

export class ErrorBoundary extends Component<
  { children: ReactNode },
  { failed: boolean }
> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (this.state.failed) {
      return (
        <Result
          status="error"
          title={t("页面暂时无法显示")}
          subTitle={t("请刷新页面重试。未提交的内容可能需要重新填写。")}
          extra={
            <Button type="primary" onClick={() => window.location.reload()}>
              {t("刷新页面")}
            </Button>
          }
        />
      );
    }
    return this.props.children;
  }
}
