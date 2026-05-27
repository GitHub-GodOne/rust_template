import { Button } from "antd";
import type { ButtonProps } from "antd";
import type { PermissionCode } from "../../app/menu";
import { useAuthStore } from "../../stores/auth";

export function PermissionButton({
  permission,
  hideWhenDenied = true,
  ...props
}: ButtonProps & {
  permission?: PermissionCode;
  hideWhenDenied?: boolean;
}) {
  const hasPermission = useAuthStore((state) => state.hasPermission);
  const allowed = hasPermission(permission);

  if (!allowed && hideWhenDenied) {
    return null;
  }

  return <Button disabled={!allowed || props.disabled} {...props} />;
}
