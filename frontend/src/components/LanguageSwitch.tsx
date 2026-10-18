import { TranslationOutlined } from "@ant-design/icons";
import { Button, Dropdown, Tooltip } from "antd";
import { LOCALES, locale, setLocale, t } from "../i18n";
import "../i18n/i18n.css";

/** Language picker; switching reloads the page in the chosen language. */
export function LanguageSwitch({ className }: { className?: string }) {
  const current = LOCALES.find((item) => item.value === locale) ?? LOCALES[0];
  return (
    <Dropdown
      trigger={["click"]}
      placement="bottomRight"
      menu={{
        selectable: true,
        selectedKeys: [locale],
        items: LOCALES.map((item) => ({ key: item.value, label: item.label })),
        onClick: ({ key }) =>
          setLocale(key as (typeof LOCALES)[number]["value"]),
      }}
    >
      <Tooltip title={t("切换语言")}>
        <Button
          type="text"
          className={["language-switch", className].filter(Boolean).join(" ")}
          icon={<TranslationOutlined />}
          aria-label={t("切换语言")}
        >
          <span className="language-switch-label">{current.short}</span>
        </Button>
      </Tooltip>
    </Dropdown>
  );
}
