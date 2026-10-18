import { Tag } from "antd";
import { t } from "../../i18n";

export function StatusTag({ active }: { active: boolean }) {
  return (
    <Tag color={active ? "success" : "default"}>
      {active ? t("启用") : t("停用")}
    </Tag>
  );
}
