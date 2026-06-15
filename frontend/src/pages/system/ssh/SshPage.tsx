import {
  ClearOutlined,
  CodeOutlined,
  DisconnectOutlined,
  LinkOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Button, Card, Select, Space, Tag, Typography, message } from "antd";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  buildSshWebSocketUrl,
  createSshTicket,
  fetchSshTargets,
} from "../../../api/admin/ssh";
import { CrudPage } from "../../../components/admin/CrudPage";
import { PermissionButton } from "../../../components/admin/PermissionButton";

type TerminalStatus = "idle" | "connecting" | "connected" | "closed" | "error";

type ServerTerminalMessage = {
  type: string;
  data?: string;
};

function statusColor(status: TerminalStatus) {
  if (status === "connected") {
    return "green";
  }
  if (status === "connecting") {
    return "processing";
  }
  if (status === "error") {
    return "red";
  }
  return "default";
}

function statusLabel(status: TerminalStatus) {
  if (status === "connected") {
    return "已连接";
  }
  if (status === "connecting") {
    return "连接中";
  }
  if (status === "closed") {
    return "已断开";
  }
  if (status === "error") {
    return "异常";
  }
  return "未连接";
}

export function SshPage() {
  const terminalHostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
  const [targetKey, setTargetKey] = useState("local-shell");
  const [status, setStatus] = useState<TerminalStatus>("idle");
  const [terminalSize, setTerminalSize] = useState({ cols: 120, rows: 32 });

  const targetsQuery = useQuery({
    queryKey: ["admin-ssh-targets"],
    queryFn: fetchSshTargets,
  });
  const ticketMutation = useMutation({ mutationFn: createSshTicket });

  const fitTerminal = useCallback(() => {
    const terminal = terminalRef.current;
    const fitAddon = fitAddonRef.current;
    if (!terminal || !fitAddon) {
      return terminalSize;
    }
    fitAddon.fit();
    const nextSize = { cols: terminal.cols, rows: terminal.rows };
    setTerminalSize(nextSize);
    if (socketRef.current?.readyState === WebSocket.OPEN) {
      socketRef.current.send(JSON.stringify({ type: "resize", ...nextSize }));
    }
    return nextSize;
  }, [terminalSize]);

  useEffect(() => {
    if (!terminalHostRef.current || terminalRef.current) {
      return;
    }
    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: true,
      fontFamily:
        'Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace',
      fontSize: 13,
      theme: {
        background: "#050505",
        foreground: "#f5f5f5",
        cursor: "#ffffff",
        selectionBackground: "#334155",
      },
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(terminalHostRef.current);
    terminal.writeln("后台 SSH 终端已就绪，请选择目标并点击连接。");
    terminal.onData((data) => {
      if (socketRef.current?.readyState === WebSocket.OPEN) {
        socketRef.current.send(JSON.stringify({ type: "input", data }));
      }
    });
    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;
    window.setTimeout(() => fitTerminal(), 0);

    return () => {
      socketRef.current?.close();
      terminal.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
  }, [fitTerminal]);

  useEffect(() => {
    const onResize = () => fitTerminal();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [fitTerminal]);

  useEffect(() => {
    if (!targetKey && targetsQuery.data?.[0]) {
      setTargetKey(targetsQuery.data[0].key);
    }
  }, [targetKey, targetsQuery.data]);

  const disconnect = useCallback(() => {
    socketRef.current?.close();
    socketRef.current = null;
    setStatus("closed");
  }, []);

  const connect = async () => {
    if (!targetKey) {
      message.warning("请先选择 SSH 目标");
      return;
    }
    disconnect();
    const terminal = terminalRef.current;
    terminal?.clear();
    terminal?.writeln("正在创建连接票据...");
    setStatus("connecting");
    try {
      const size = fitTerminal();
      const ticket = await ticketMutation.mutateAsync({
        target_key: targetKey,
        cols: size.cols,
        rows: size.rows,
      });
      terminal?.writeln("正在连接终端...");
      const socket = new WebSocket(buildSshWebSocketUrl(ticket.ticket));
      socketRef.current = socket;
      socket.onopen = () => {
        setStatus("connected");
        socket.send(JSON.stringify({ type: "resize", ...fitTerminal() }));
      };
      socket.onmessage = (event) => {
        try {
          const payload = JSON.parse(event.data) as ServerTerminalMessage;
          if (payload.type === "output") {
            terminalRef.current?.write(payload.data ?? "");
          } else if (payload.type === "connected") {
            setStatus("connected");
          } else if (payload.type === "closed") {
            setStatus("closed");
          } else if (payload.type === "error") {
            setStatus("error");
            terminalRef.current?.writeln(
              `\r\n${payload.data ?? "终端连接异常"}`,
            );
          }
        } catch {
          terminalRef.current?.write(String(event.data));
        }
      };
      socket.onerror = () => {
        setStatus("error");
        terminalRef.current?.writeln("\r\n终端连接异常");
      };
      socket.onclose = () => {
        socketRef.current = null;
        setStatus((current) => (current === "error" ? "error" : "closed"));
      };
    } catch (error) {
      setStatus("error");
      terminal?.writeln(
        `\r\n${error instanceof Error ? error.message : "连接失败"}`,
      );
      message.error(error instanceof Error ? error.message : "连接失败");
    }
  };

  const selectedTarget = targetsQuery.data?.find(
    (target) => target.key === targetKey,
  );

  return (
    <CrudPage
      title="SSH 管理"
      subtitle="后台免二次登录的本机 Shell 与远程 SSH 交互式终端"
      breadcrumb={["系统管理", "SSH 管理"]}
      icon={<CodeOutlined />}
    >
      <Card className="ssh-terminal-card">
        <div className="ssh-terminal-toolbar">
          <Space wrap>
            <Select
              loading={targetsQuery.isLoading}
              value={targetKey}
              style={{ width: 280 }}
              onChange={setTargetKey}
              options={(targetsQuery.data ?? []).map((target) => ({
                value: target.key,
                label:
                  target.target_type === "local"
                    ? target.name
                    : `${target.name} (${target.username}@${target.host}:${target.port ?? 22})`,
              }))}
            />
            <PermissionButton
              type="primary"
              icon={<LinkOutlined />}
              permission="system:ssh:connect"
              loading={ticketMutation.isPending || status === "connecting"}
              onClick={connect}
            >
              连接
            </PermissionButton>
            <Button icon={<DisconnectOutlined />} onClick={disconnect}>
              断开
            </Button>
            <Button
              icon={<ClearOutlined />}
              onClick={() => terminalRef.current?.clear()}
            >
              清屏
            </Button>
            <Button
              icon={<ReloadOutlined />}
              onClick={() => targetsQuery.refetch()}
            >
              刷新目标
            </Button>
            <Tag color={statusColor(status)}>{statusLabel(status)}</Tag>
            <Typography.Text type="secondary">
              {terminalSize.cols} × {terminalSize.rows}
            </Typography.Text>
          </Space>
          <Typography.Text type="secondary">
            {selectedTarget?.target_type === "local"
              ? "当前目标：本机 Shell"
              : selectedTarget
                ? `当前目标：${selectedTarget.username}@${selectedTarget.host}`
                : "未选择目标"}
          </Typography.Text>
        </div>
        <div ref={terminalHostRef} className="ssh-terminal-host" />
      </Card>
    </CrudPage>
  );
}
