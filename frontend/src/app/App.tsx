import {
  QueryCache,
  QueryClient,
  QueryClientProvider,
} from "@tanstack/react-query";
import { ConfigProvider, type ThemeConfig, message, theme } from "antd";
import enUS from "antd/locale/en_US";
import zhCN from "antd/locale/zh_CN";
import dayjs from "dayjs";
import "dayjs/locale/zh-cn";
import { useEffect } from "react";
import { BrowserRouter } from "react-router-dom";
import { ApiRequestError } from "../api/client";
import { ErrorBoundary } from "../components/ErrorBoundary";
import { ReauthHost } from "../components/ReauthHost";
import { isEnglish, t } from "../i18n";
import { useThemeStore } from "../stores/theme";
import { AppRouter } from "./router";

dayjs.locale(isEnglish ? "en" : "zh-cn");

const FONT_FAMILY =
  '"IBM Plex Sans", Inter, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Noto Sans SC", sans-serif';

const queryClient = new QueryClient({
  queryCache: new QueryCache({
    onError: (error) => {
      void message.error({
        key: "query-error",
        content: error.message || t("数据加载失败，请重试"),
      });
    },
  }),
  defaultOptions: {
    queries: {
      retry: (count, error) =>
        count < 1 &&
        !(
          error instanceof ApiRequestError &&
          error.status &&
          error.status < 500
        ),
      refetchOnWindowFocus: false,
    },
  },
});

const lightTheme: ThemeConfig = {
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
    fontFamily: FONT_FAMILY,
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
};

/** Same warm palette, lightness flipped. */
const darkTheme: ThemeConfig = {
  algorithm: theme.darkAlgorithm,
  token: {
    colorPrimary: "#d4744f",
    colorInfo: "#d4744f",
    colorSuccess: "#6f9a7b",
    colorWarning: "#d39a3c",
    colorError: "#d46a65",
    colorText: "#ece7df",
    colorTextSecondary: "#a59e94",
    colorBorder: "#3a3631",
    colorBgBase: "#1b1a18",
    colorBgContainer: "#22201d",
    colorBgElevated: "#2a2724",
    borderRadius: 7,
    controlHeight: 36,
    fontFamily: FONT_FAMILY,
  },
  components: {
    Layout: {
      bodyBg: "#141311",
      headerBg: "rgba(27, 26, 24, 0.9)",
      siderBg: "#1d1b19",
    },
    Menu: {
      itemBg: "transparent",
      subMenuItemBg: "transparent",
      itemSelectedBg: "#3a332d",
      itemSelectedColor: "#f5f1ea",
      itemHoverBg: "rgba(255, 255, 255, 0.06)",
      itemHoverColor: "#f5f1ea",
      itemBorderRadius: 6,
      darkItemBg: "transparent",
      darkSubMenuItemBg: "transparent",
      darkItemColor: "#bbb4aa",
      darkItemHoverBg: "#33302c",
      darkItemHoverColor: "#f8f4ed",
      darkItemSelectedBg: "#423832",
      darkItemSelectedColor: "#ffffff",
    },
    Button: {
      primaryShadow: "none",
      defaultShadow: "none",
    },
    Table: {
      headerBg: "#2a2723",
      headerColor: "#d6d0c6",
      rowHoverBg: "#2c2925",
      borderColor: "#33302b",
    },
    Card: {
      headerBg: "transparent",
    },
  },
};

export function App() {
  const resolvedTheme = useThemeStore((state) => state.resolved);
  const themeConfig = resolvedTheme === "dark" ? darkTheme : lightTheme;
  // Static message / Modal.confirm calls render outside this tree.
  useEffect(() => {
    ConfigProvider.config({
      holderRender: (children) => (
        <ConfigProvider locale={isEnglish ? enUS : zhCN} theme={themeConfig}>
          {children}
        </ConfigProvider>
      ),
    });
  }, [themeConfig]);
  return (
    <ConfigProvider locale={isEnglish ? enUS : zhCN} theme={themeConfig}>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <ErrorBoundary>
            <AppRouter />
          </ErrorBoundary>
        </BrowserRouter>
        <ReauthHost />
      </QueryClientProvider>
    </ConfigProvider>
  );
}
