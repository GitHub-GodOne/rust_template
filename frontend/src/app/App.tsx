import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ConfigProvider, theme } from "antd";
import zhCN from "antd/locale/zh_CN";
import { BrowserRouter } from "react-router-dom";
import { AppRouter } from "./router";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});

export function App() {
  return (
    <ConfigProvider
      locale={zhCN}
      theme={{
        algorithm: theme.defaultAlgorithm,
        token: {
          colorPrimary: "#c15f3c",
          colorInfo: "#c15f3c",
          colorSuccess: "#4f735b",
          colorWarning: "#b7791f",
          colorError: "#b84a46",
          colorText: "#292622",
          colorTextSecondary: "#756f67",
          colorBorder: "#ded8cf",
          colorBgBase: "#f7f5f1",
          borderRadius: 7,
          controlHeight: 36,
          fontFamily:
            '"IBM Plex Sans", Inter, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Noto Sans SC", sans-serif',
        },
        components: {
          Layout: {
            bodyBg: "#f2efe9",
            headerBg: "rgba(247, 245, 241, 0.9)",
            siderBg: "#282623",
          },
          Menu: {
            itemBg: "transparent",
            subMenuItemBg: "transparent",
            itemSelectedBg: "#d9d1c6",
            itemSelectedColor: "#292622",
            itemHoverBg: "rgba(217, 209, 198, 0.55)",
            itemHoverColor: "#292622",
            itemBorderRadius: 6,
            darkItemBg: "transparent",
            darkSubMenuItemBg: "transparent",
            darkItemColor: "#bbb4aa",
            darkItemHoverBg: "#37332f",
            darkItemHoverColor: "#f8f4ed",
            darkItemSelectedBg: "#423832",
            darkItemSelectedColor: "#ffffff",
          },
          Button: {
            primaryShadow: "none",
            defaultShadow: "none",
          },
          Table: {
            headerBg: "#ebe6de",
            headerColor: "#4d4842",
            rowHoverBg: "#f3eee7",
            borderColor: "#e3ddd5",
          },
          Card: {
            headerBg: "transparent",
          },
        },
      }}
    >
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <AppRouter />
        </BrowserRouter>
      </QueryClientProvider>
    </ConfigProvider>
  );
}
