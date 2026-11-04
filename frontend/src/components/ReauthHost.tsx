import { SafetyCertificateOutlined } from "@ant-design/icons";
import { Alert, Input, Modal, Typography } from "antd";
import { useEffect, useRef, useState } from "react";
import { confirmPassword } from "../api/profile";
import { registerReauthOpener } from "../api/reauth";
import { t } from "../i18n";

/**
 * Asks for the sign-in password when the server answers REAUTH_REQUIRED
 * (remote shells, running commands, restoring backups…).
 */
export function ReauthHost() {
  const [open, setOpen] = useState(false);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string>();
  const [submitting, setSubmitting] = useState(false);
  const resolver = useRef<((confirmed: boolean) => void) | null>(null);

  useEffect(() => {
    registerReauthOpener(
      () =>
        new Promise<boolean>((resolve) => {
          resolver.current = resolve;
          setPassword("");
          setError(undefined);
          setOpen(true);
        }),
    );
    return () => registerReauthOpener(null);
  }, []);

  const finish = (confirmed: boolean) => {
    setOpen(false);
    resolver.current?.(confirmed);
    resolver.current = null;
  };

  const submit = async () => {
    if (!password) {
      setError(t("请输入登录密码"));
      return;
    }
    setSubmitting(true);
    try {
      await confirmPassword(password);
      finish(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("确认失败"));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Modal
      title={
        <span>
          <SafetyCertificateOutlined /> {t("确认登录密码")}
        </span>
      }
      open={open}
      okText={t("确认")}
      cancelText={t("取消")}
      confirmLoading={submitting}
      onOk={() => void submit()}
      onCancel={() => finish(false)}
      destroyOnClose
      width={420}
      // Above page dialogs (the RDP/VNC login sits at 1700–1800) that are
      // still open while the request waits for this confirmation.
      zIndex={2000}
    >
      <Typography.Paragraph type="secondary">
        {t(
          "这是一个敏感操作，请输入当前账号的登录密码。确认后一段时间内不会再次询问。",
        )}
      </Typography.Paragraph>
      {error ? (
        <Alert type="error" showIcon message={error} className="reauth-error" />
      ) : null}
      <Input.Password
        autoFocus
        value={password}
        autoComplete="current-password"
        placeholder={t("登录密码")}
        onChange={(event) => {
          setPassword(event.target.value);
          setError(undefined);
        }}
        onPressEnter={() => void submit()}
      />
    </Modal>
  );
}
