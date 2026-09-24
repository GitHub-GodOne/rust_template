import { DesktopOutlined, PlusOutlined } from "@ant-design/icons";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Checkbox,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tabs,
  Tag,
  message,
} from "antd";
import Guacamole from "guacamole-common-js";
import { useEffect, useRef, useState } from "react";
import {
  type RdpLogin,
  type RdpTarget,
  createRdpTicket,
  deleteRdpCredentials,
  fetchRdpCredentialStatus,
  fetchRdpGatewayStatus,
  fetchRdpTargets,
  rdpSocketUrl,
  saveRdpTargets,
} from "../../../api/admin/rdp";
import { CrudPage } from "../../../components/admin/CrudPage";
import { useAuthStore } from "../../../stores/auth";
import { createClientId } from "../../../utils/clientId";

type Session = { id: string; target: RdpTarget; ticket: string };

function Desktop({ session, active }: { session: Session; active: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const clientRef = useRef<Guacamole.Client>();
  const keyboardRef = useRef<Guacamole.Keyboard>();
  const [status, setStatus] = useState("连接中");
  const [error, setError] = useState("");
  const [stopped, setStopped] = useState(false);

  useEffect(() => {
    const container = host.current;
    if (!container) return;
    const tunnel = new Guacamole.WebSocketTunnel(rdpSocketUrl(session.ticket));
    const client = new Guacamole.Client(tunnel);
    clientRef.current = client;
    const display = client.getDisplay();
    const element = display.getElement();
    container.appendChild(element);
    const fit = () => {
      if (container.clientWidth && display.getWidth()) {
        display.scale(
          Math.min(
            container.clientWidth / display.getWidth(),
            container.clientHeight / display.getHeight(),
          ),
        );
      }
    };
    display.onresize = fit;
    const observer = new ResizeObserver(fit);
    observer.observe(container);
    const mouse = new Guacamole.Mouse(element);
    const touch = new Guacamole.Mouse.Touchscreen(element);
    for (const input of [mouse, touch]) {
      input.onEach(["mousedown", "mousemove", "mouseup"], () => {
        container.focus({ preventScroll: true });
        client.sendMouseState(input.currentState, true);
      });
    }
    const keyboard = new Guacamole.Keyboard(container);
    keyboardRef.current = keyboard;
    keyboard.onkeydown = (key) => {
      client.sendKeyEvent(1, key);
      return false;
    };
    keyboard.onkeyup = (key) => client.sendKeyEvent(0, key);
    const release = () => keyboard.reset();
    container.addEventListener("blur", release);
    window.addEventListener("blur", release);
    client.onerror = (failure) => {
      setError(failure.message ?? "RDP 连接失败");
      client.disconnect();
    };
    tunnel.onerror = (failure) => {
      setError(failure.message ?? "RDP 网关连接失败");
      client.disconnect();
    };
    client.onstatechange = (state) => {
      setStatus(state === 3 ? "已连接" : state === 5 ? "已断开" : "连接中");
      if (state === 5) setStopped(true);
    };
    // React StrictMode may mount and clean up effects once before the real mount.
    const connectTimer = window.setTimeout(() => client.connect(), 0);
    return () => {
      window.clearTimeout(connectTimer);
      keyboard.reset();
      keyboard.onkeydown = null;
      keyboard.onkeyup = null;
      window.removeEventListener("blur", release);
      container.removeEventListener("blur", release);
      observer.disconnect();
      client.onerror = null;
      client.onstatechange = null;
      tunnel.onerror = null;
      client.disconnect();
      element.remove();
      clientRef.current = undefined;
    };
  }, [session]);

  useEffect(() => {
    if (!active) keyboardRef.current?.reset();
  }, [active]);

  const keys = (codes: number[]) => {
    for (const code of codes) clientRef.current?.sendKeyEvent(1, code);
    for (const code of [...codes].reverse())
      clientRef.current?.sendKeyEvent(0, code);
    host.current?.focus();
  };

  return (
    <div className="rdp-session">
      <Space wrap>
        <Tag color={status === "已连接" ? "green" : "default"}>{status}</Tag>
        <span>
          {session.target.host}:{session.target.port}
        </span>
        <Button
          disabled={stopped}
          onClick={() => keys([0xffe3, 0xffe9, 0xffff])}
        >
          Ctrl+Alt+Del
        </Button>
        <Button disabled={stopped} onClick={() => keys([0xffeb])}>
          Windows 键
        </Button>
        <Button
          onClick={() => {
            host.current
              ?.requestFullscreen()
              .catch(() => message.error("当前浏览器无法进入全屏"));
          }}
        >
          全屏
        </Button>
        <Button
          disabled={stopped}
          danger
          onClick={() => clientRef.current?.disconnect()}
        >
          断开
        </Button>
      </Space>
      {error && (
        <Alert
          type="error"
          showIcon
          message={error}
          description="检查目标电脑的远程桌面、账号密码、证书和 guacd 网关；重新连接请使用顶部的连接按钮。"
        />
      )}
      {stopped && !error && (
        <Alert type="info" message="连接已断开，可重新选择目标并登录。" />
      )}
      <div
        ref={host}
        // biome-ignore lint/a11y/noNoninteractiveTabindex: The remote desktop receives keyboard input.
        tabIndex={0}
        className="rdp-desktop"
        role="application"
        aria-label={`${session.target.name} 远程桌面，点击后使用键盘和鼠标操作`}
      />
    </div>
  );
}

export function RdpPage({ visible = true }: { visible?: boolean }) {
  const cache = useQueryClient();
  const targets = useQuery({
    queryKey: ["rdp-targets"],
    queryFn: fetchRdpTargets,
  });
  const canConnect = useAuthStore((s) => s.hasPermission("system:rdp:connect"));
  const canEdit = useAuthStore((s) => s.hasPermission("system:rdp:update"));
  const [selected, setSelected] = useState<string>();
  const [sessions, setSessions] = useState<Session[]>([]);
  const [active, setActive] = useState<string>();
  const [loginOpen, setLoginOpen] = useState(false);
  const [manageOpen, setManageOpen] = useState(false);
  const [editing, setEditing] = useState<RdpTarget | null>();
  const [busy, setBusy] = useState(false);
  const [manualLogin, setManualLogin] = useState(false);
  const [rememberCredentials, setRememberCredentials] = useState(true);
  const gateway = useQuery({
    queryKey: ["rdp-gateway"],
    queryFn: fetchRdpGatewayStatus,
    enabled: visible,
    refetchOnWindowFocus: true,
  });
  const credentials = useQuery({
    queryKey: ["rdp-credentials", selected],
    queryFn: () => fetchRdpCredentialStatus(selected as string),
    enabled: loginOpen && !!selected && canConnect,
    staleTime: 0,
  });
  const useSavedCredentials = credentials.data?.saved === true && !manualLogin;
  const [loginForm] = Form.useForm<RdpLogin>();
  const [targetForm] = Form.useForm<RdpTarget>();
  const selectedTarget = targets.data?.find(
    (target) => target.key === selected,
  );
  const connectionBlocked = !canConnect
    ? "当前账号没有 RDP 连接权限，请管理员分配 system:rdp:connect 权限后重新登录。"
    : sessions.length >= 8
      ? "最多同时打开 8 个连接标签，请先关闭一个标签。"
      : selectedTarget && !selectedTarget.enabled
        ? "此电脑已停用，请在管理电脑中编辑并启用。"
        : undefined;

  useEffect(() => {
    const list = targets.data;
    if (!list) return;
    setSelected((current) =>
      list.some((target) => target.key === current)
        ? current
        : list.find((target) => target.enabled)?.key,
    );
  }, [targets.data]);

  const openLogin = (target: RdpTarget) => {
    if (!target.enabled) {
      message.warning("此电脑已停用，请先编辑并启用");
      return;
    }
    if (!canConnect || sessions.length >= 8) {
      message.warning(connectionBlocked);
      return;
    }
    setSelected(target.key);
    setManualLogin(false);
    setRememberCredentials(true);
    loginForm.resetFields();
    setManageOpen(false);
    setLoginOpen(true);
  };

  const connect = async () => {
    const values = useSavedCredentials ? {} : await loginForm.validateFields();
    const target = targets.data?.find((t) => t.key === selected && t.enabled);
    if (!target) {
      message.warning("所选电脑已被删除或停用，请重新选择");
      return;
    }
    setBusy(true);
    try {
      const result = await createRdpTicket({
        ...values,
        target_key: target.key,
        width: Math.min(window.innerWidth, 1920),
        height: 900,
        use_saved_credentials: useSavedCredentials,
        remember_credentials: !useSavedCredentials && rememberCredentials,
      });
      await cache.invalidateQueries({
        queryKey: ["rdp-credentials", target.key],
      });
      const session = {
        id: createClientId("rdp"),
        target,
        ticket: result.ticket,
      };
      setSessions((old) => [...old, session]);
      setActive(session.id);
      setLoginOpen(false);
      loginForm.resetFields();
    } catch {
      message.error("创建 RDP 连接失败，请检查权限和目标配置");
    } finally {
      setBusy(false);
    }
  };

  const save = async (list: RdpTarget[], selectKey?: string) => {
    setBusy(true);
    try {
      const result = await saveRdpTargets(list);
      cache.setQueryData(["rdp-targets"], result);
      setEditing(undefined);
      if (selectKey) {
        setSelected(selectKey);
        setManageOpen(false);
      }
      message.success("目标配置已保存");
    } catch {
      message.error("保存失败，请检查权限、主机和端口");
    } finally {
      setBusy(false);
    }
  };

  return (
    <CrudPage
      title="RDP 远程桌面"
      subtitle="通过 3389 连接 Windows，直接在浏览器操作桌面"
      breadcrumb={["系统管理", "RDP 管理"]}
      icon={<DesktopOutlined />}
      toolbar={
        <Space wrap>
          <Select
            style={{ minWidth: 220 }}
            placeholder="选择远程电脑"
            aria-label="选择远程电脑"
            showSearch
            optionFilterProp="label"
            loading={targets.isLoading}
            value={selected}
            onChange={setSelected}
            options={targets.data?.map((t) => ({
              value: t.key,
              label: `${t.name} (${t.host}:${t.port})${t.enabled ? "" : " · 已停用"}`,
              disabled: !t.enabled,
            }))}
          />
          <Button
            type="primary"
            disabled={!selectedTarget?.enabled || !!connectionBlocked}
            onClick={() => selectedTarget && openLogin(selectedTarget)}
          >
            连接桌面
          </Button>
          {canEdit && (
            <Button onClick={() => setManageOpen(true)}>管理电脑</Button>
          )}
          <Button
            onClick={() => {
              targets.refetch();
              gateway.refetch();
            }}
          >
            刷新
          </Button>
        </Space>
      }
    >
      {gateway.data && !gateway.data.reachable && (
        <Alert
          type="warning"
          showIcon
          message={gateway.data.message}
          description="本机部署：启动 Docker Desktop 后，在项目目录运行 docker compose -f docker-compose.rdp.yml up -d，再点击刷新。已保存的凭据不受网关状态影响。"
          action={
            <Button
              loading={gateway.isFetching}
              onClick={() => gateway.refetch()}
            >
              重新检测
            </Button>
          }
        />
      )}
      {connectionBlocked && (
        <Alert type="warning" showIcon message={connectionBlocked} />
      )}
      {targets.isError && (
        <Alert type="error" message="读取 RDP 目标失败，请重试" />
      )}
      {sessions.length ? (
        <Tabs
          type="editable-card"
          hideAdd
          activeKey={active}
          onChange={setActive}
          onEdit={(id, action) => {
            if (action === "remove") {
              const remaining = sessions.filter((s) => s.id !== id);
              setSessions(remaining);
              if (active === id) setActive(remaining[remaining.length - 1]?.id);
            }
          }}
          items={sessions.map((session) => ({
            key: session.id,
            label: session.target.name,
            forceRender: true,
            children: (
              <Desktop
                session={session}
                active={visible && active === session.id}
              />
            ),
          }))}
        />
      ) : (
        <Empty description="选择或添加一台已启用远程桌面的 Windows 电脑，登录后即可操作桌面。" />
      )}
      <Modal
        title="登录远程电脑"
        open={loginOpen}
        confirmLoading={busy}
        okText={useSavedCredentials ? "使用已保存凭据连接" : "连接"}
        okButtonProps={{
          disabled: credentials.isFetching || credentials.isError,
        }}
        onOk={connect}
        onCancel={() => {
          setLoginOpen(false);
          loginForm.resetFields();
        }}
      >
        {credentials.isFetching && (
          <Alert type="info" message="正在检查已保存凭据…" />
        )}
        {credentials.isError && (
          <Alert
            type="error"
            message="无法读取凭据状态，请确认后端已升级并运行数据库迁移"
            action={<Button onClick={() => credentials.refetch()}>重试</Button>}
          />
        )}
        {credentials.data?.saved && (
          <Space
            direction="vertical"
            style={{ width: "100%", marginBottom: 16 }}
          >
            <Alert
              type="success"
              showIcon
              message="已保存此电脑的登录凭据，无需再次输入密码。"
            />
            <Checkbox
              checked={useSavedCredentials}
              onChange={(event) => setManualLogin(!event.target.checked)}
            >
              使用已保存的账号和密码
            </Checkbox>
            <Popconfirm
              title="清除当前账号为此电脑保存的凭据？"
              onConfirm={async () => {
                try {
                  await deleteRdpCredentials(selected as string);
                  cache.setQueryData(["rdp-credentials", selected], {
                    saved: false,
                  });
                  setManualLogin(true);
                  loginForm.resetFields();
                  message.success("已清除保存的凭据");
                } catch {
                  message.error("清除失败，请重试");
                }
              }}
            >
              <Button danger>清除已保存凭据</Button>
            </Popconfirm>
          </Space>
        )}
        <Form
          form={loginForm}
          layout="vertical"
          autoComplete="off"
          hidden={useSavedCredentials}
        >
          <Form.Item
            name="username"
            label="Windows 用户名"
            rules={[{ required: true }]}
          >
            <Input
              placeholder="用户名或 MicrosoftAccount\邮箱"
              autoComplete="off"
            />
          </Form.Item>
          <Form.Item
            name="password"
            label="Windows 密码（不是 PIN）"
            rules={[{ required: true }]}
          >
            <Input.Password autoComplete="new-password" />
          </Form.Item>
          <Form.Item name="domain" label="域（可选）">
            <Input />
          </Form.Item>
          <Checkbox
            checked={rememberCredentials}
            onChange={(event) => setRememberCredentials(event.target.checked)}
          >
            记住此电脑的账号和密码（加密保存，仅当前后台账号可用）
          </Checkbox>
        </Form>
      </Modal>
      <Modal
        title="管理远程电脑"
        open={manageOpen}
        width={850}
        footer={null}
        onCancel={() => setManageOpen(false)}
      >
        {!canConnect && (
          <Alert
            type="warning"
            showIcon
            message="当前账号只有管理权限，没有 RDP 连接权限。请管理员分配 system:rdp:connect 权限后重新登录。"
          />
        )}
        <Button
          icon={<PlusOutlined />}
          onClick={() => {
            setEditing(null);
            targetForm.resetFields();
            targetForm.setFieldsValue({
              port: 3389,
              enabled: true,
              ignore_certificate: false,
            });
          }}
        >
          添加电脑
        </Button>
        <Table
          rowKey="key"
          dataSource={targets.data}
          pagination={false}
          scroll={{ x: 650 }}
          columns={[
            { title: "名称", dataIndex: "name" },
            { title: "主机", dataIndex: "host" },
            { title: "端口", dataIndex: "port" },
            {
              title: "状态",
              render: (_, t: RdpTarget) => (t.enabled ? "启用" : "停用"),
            },
            {
              title: "操作",
              render: (_, t: RdpTarget) => (
                <Space>
                  <Button
                    type="primary"
                    disabled={!t.enabled || !canConnect || sessions.length >= 8}
                    onClick={() => openLogin(t)}
                  >
                    连接
                  </Button>
                  <Button
                    onClick={() => {
                      setEditing(t);
                      targetForm.setFieldsValue(t);
                    }}
                  >
                    编辑
                  </Button>
                  <Popconfirm
                    title="删除此电脑？已连接桌面不会被关闭。"
                    onConfirm={() =>
                      save((targets.data ?? []).filter((v) => v.key !== t.key))
                    }
                  >
                    <Button danger disabled={busy}>
                      删除
                    </Button>
                  </Popconfirm>
                </Space>
              ),
            },
          ]}
        />
      </Modal>
      <Modal
        title={editing ? "编辑电脑" : "添加电脑"}
        open={editing !== undefined}
        confirmLoading={busy}
        onCancel={() => setEditing(undefined)}
        onOk={async () => {
          const value = await targetForm.validateFields();
          const key = editing?.key ?? createClientId("pc");
          await save(
            [
              ...(targets.data ?? []).filter((t) => t.key !== editing?.key),
              { ...value, key },
            ],
            key,
          );
        }}
      >
        <Form form={targetForm} layout="vertical">
          <Form.Item
            name="name"
            label="名称"
            rules={[{ required: true, whitespace: true, max: 200 }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="host"
            label="主机 IP / 域名（从网关访问）"
            rules={[{ required: true, whitespace: true, max: 255 }]}
          >
            <Input placeholder="192.168.1.10" />
          </Form.Item>
          <Form.Item name="port" label="端口" rules={[{ required: true }]}>
            <InputNumber min={1} max={65535} precision={0} />
          </Form.Item>
          <Form.Item name="enabled" label="启用" valuePropName="checked">
            <Switch />
          </Form.Item>
          <Form.Item
            name="ignore_certificate"
            label="允许自签名 / 不受信任证书"
            valuePropName="checked"
            extra="仅在确认目标电脑可信时开启。"
          >
            <Switch />
          </Form.Item>
        </Form>
      </Modal>
    </CrudPage>
  );
}
