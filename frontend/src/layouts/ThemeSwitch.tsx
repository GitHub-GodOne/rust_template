import { DesktopOutlined, MoonOutlined, SunOutlined } from "@ant-design/icons";
import { Button, Tooltip } from "antd";
import { t } from "../i18n";
import { type ThemeMode, useThemeStore } from "../stores/theme";

const labels: Record<ThemeMode, () => string> = {
  light: () => t("浅色模式"),
  dark: () => t("深色模式"),
  system: () => t("跟随系统"),
};

const icons: Record<ThemeMode, JSX.Element> = {
  light: <SunOutlined />,
  dark: <MoonOutlined />,
  system: <DesktopOutlined />,
};

/** Cycles light → dark → follow the system. */
export function ThemeSwitch({ className }: { className?: string }) {
  const mode = useThemeStore((state) => state.mode);
  const cycle = useThemeStore((state) => state.cycle);
  const label = t("外观：{0}（点击切换）", labels[mode]());
  return (
    <Tooltip title={label}>
      <Button
        type="text"
        className={`admin-icon-button ${className ?? ""}`}
        aria-label={label}
        icon={icons[mode]}
        onClick={cycle}
      />
    </Tooltip>
  );
}
