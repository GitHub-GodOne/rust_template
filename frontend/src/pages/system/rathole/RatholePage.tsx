import { DeleteOutlined, EditOutlined, InfoCircleOutlined, PlayCircleOutlined, PlusOutlined, StopOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, Button, Divider, Form, Input, Modal, Popconfirm, Select, Space, Switch, Table, Tag, Typography, message } from "antd";
import { useState } from "react";
import { createRatholeProfile, deleteRatholeProfile, fetchRatholeProfiles, generateRatholeKeypair, startRatholeProfile, stopRatholeProfile, updateRatholeProfile, validateRatholeProfile, type RatholeProfile, type SaveRatholeProfile } from "../../../api/admin/rathole";
import { CrudPage } from "../../../components/admin/CrudPage";
import { PermissionButton } from "../../../components/admin/PermissionButton";
import { t } from "../../../i18n";

const serverTemplate = `[server]\nbind_addr = "0.0.0.0:2333"\ndefault_token = "change-me"\nheartbeat_interval = 30\n\n[server.transport]\ntype = "tcp"\n\n[server.services.web]\ntype = "tcp"\nbind_addr = "0.0.0.0:8080"\n`;
const clientTemplate = `[client]\nremote_addr = "example.com:2333"\ndefault_token = "change-me"\nheartbeat_timeout = 40\nretry_interval = 1\n\n[client.transport]\ntype = "tcp"\n\n[client.services.web]\ntype = "tcp"\nlocal_addr = "127.0.0.1:8080"\n`;
type FormValues = SaveRatholeProfile & { proxy_url?: string };

function readClientProxy(config: string) {
  let inTcpTransport = false;
  for (const line of config.split(/\r?\n/)) {
    if (/^\s*\[/.test(line)) {
      inTcpTransport = line.trim() === "[client.transport.tcp]";
    }
    if (inTcpTransport) {
      const match = line.match(/^\s*proxy\s*=\s*["'](.*)["']\s*$/);
      if (match) {
        return match[1];
      }
    }
  }
  return "";
}

function applyClientProxy(config: string, mode: string, proxyUrl?: string) {
  if (mode !== "client" && mode !== "auto") {
    return config;
  }
  const lines = config.split(/\r?\n/);
  const proxyLine = `proxy = ${JSON.stringify(proxyUrl?.trim() ?? "")}`;
  let tableStart = -1;
  let tableEnd = lines.length;
  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index].trim() === "[client.transport.tcp]") {
      tableStart = index;
      continue;
    }
    if (tableStart >= 0 && index > tableStart && /^\s*\[/.test(lines[index])) {
      tableEnd = index;
      break;
    }
  }
  if (tableStart < 0) {
    if (!proxyUrl?.trim()) {
      return config;
    }
    return `${config.trimEnd()}\n\n[client.transport.tcp]\n${proxyLine}\n`;
  }
  const proxyIndex = lines.findIndex(
    (line, index) => index > tableStart && index < tableEnd && /^\s*proxy\s*=/.test(line),
  );
  if (!proxyUrl?.trim()) {
    if (proxyIndex >= 0) {
      lines.splice(proxyIndex, 1);
    }
  } else if (proxyIndex >= 0) {
    lines[proxyIndex] = proxyLine;
  } else {
    lines.splice(tableStart + 1, 0, proxyLine);
  }
  return lines.join("\n");
}

export function RatholePage(){
 const [editing,setEditing]=useState<RatholeProfile|null>(null); const [open,setOpen]=useState(false); const [helpOpen,setHelpOpen]=useState(false); const [form]=Form.useForm<FormValues>(); const qc=useQueryClient();
 const mode = Form.useWatch("mode", form);
 const query=useQuery({queryKey:["rathole-profiles"],queryFn:fetchRatholeProfiles}); const reload=()=>qc.invalidateQueries({queryKey:["rathole-profiles"]});
 const save=useMutation({mutationFn:(v:FormValues)=>{const payload: SaveRatholeProfile = {name:v.name,mode:v.mode,enabled:v.enabled,config_toml:applyClientProxy(v.config_toml,v.mode,v.proxy_url)};return editing?updateRatholeProfile(editing.id,payload):createRatholeProfile(payload);},onSuccess:()=>{message.success(t("rathole 配置已保存"));setOpen(false);reload();}});
 const remove=useMutation({mutationFn:deleteRatholeProfile,onSuccess:reload}); const run=useMutation({mutationFn:({id,on}:{id:number;on:boolean})=>on?startRatholeProfile(id):stopRatholeProfile(id),onSuccess:reload});
 const openEdit=(item?:RatholeProfile)=>{setEditing(item??null);form.setFieldsValue(item?{name:item.name,mode:item.mode,config_toml:item.config_toml,proxy_url:readClientProxy(item.config_toml),enabled:item.enabled}:{name:"",mode:"auto",config_toml:serverTemplate,proxy_url:"",enabled:false});setOpen(true);};
 return <CrudPage title={t("内网穿透（Rathole）")} extra={<Space><Button icon={<InfoCircleOutlined/>} onClick={()=>setHelpOpen(true)}>{t("配置帮助")}</Button><PermissionButton permission="system:rathole:manage" type="primary" icon={<PlusOutlined/>} onClick={()=>openEdit()}>{t("新建配置")}</PermissionButton></Space>}>
   <Alert type="info" showIcon message={t("配置编辑器直接使用 Rathole 原生 TOML，支持服务端/客户端、TCP/UDP、TLS、Noise、WebSocket、代理、IPv6、心跳、重试和热加载等全部上游选项。")} style={{marginBottom:16}}/>
   <Table rowKey="id" loading={query.isLoading} dataSource={query.data??[]} columns={[{title:t("名称"),dataIndex:"name"},{title:t("模式"),dataIndex:"mode"},{title:t("状态"),render:(_,r:RatholeProfile)=><Tag color={r.running?"green":"default"}>{r.running?t("运行中"):t("已停止")}</Tag>},{title:t("启用"),render:(_,r:RatholeProfile)=><Tag color={r.enabled?"blue":"default"}>{r.enabled?t("是"):t("否")}</Tag>},{title:t("操作"),render:(_,r:RatholeProfile)=><Space><Button size="small" icon={r.running?<StopOutlined/>:<PlayCircleOutlined/>} onClick={()=>run.mutate({id:r.id,on:!r.running})}>{r.running?t("停止"):t("启动")}</Button><Button size="small" icon={<EditOutlined/>} onClick={()=>openEdit(r)}/><Popconfirm title={t("确认删除？")} onConfirm={()=>remove.mutate(r.id)}><Button size="small" danger icon={<DeleteOutlined/>}/></Popconfirm></Space>}]} />
   <Modal open={open} title={editing?t("编辑 Rathole 配置"):t("新建 Rathole 配置")} width={900} okText={t("保存")} confirmLoading={save.isPending} onCancel={()=>setOpen(false)} onOk={()=>form.validateFields().then(async(v)=>{const payload={...v,config_toml:applyClientProxy(v.config_toml,v.mode,v.proxy_url)};await validateRatholeProfile(payload);save.mutate(v);}).catch(()=>{})}><Form form={form} layout="vertical"><Form.Item name="name" label={t("名称")} rules={[{required:true}]}><Input/></Form.Item><Form.Item name="mode" label={t("运行模式")}><Select options={[{value:"auto",label:t("自动")},{value:"server",label:t("服务端")},{value:"client",label:t("客户端")}]} /></Form.Item><Alert type="info" showIcon message={t("客户端代理只影响客户端连接服务端的出站连接；控制流和映射数据会复用同一条已建立的代理隧道，返回方向也会沿该隧道传输。")} style={{marginBottom:12}}/><Form.Item name="proxy_url" label={t("客户端出站代理（可选）")} extra={t("填写 socks5:// 或 http:// 地址，例如 socks5://user:password@127.0.0.1:1080；留空表示直连。") }><Input disabled={mode === "server"} placeholder="socks5://127.0.0.1:1080" /></Form.Item><Space><Button onClick={()=>form.setFieldsValue({config_toml:serverTemplate,proxy_url:""})}>{t("服务端模板")}</Button><Button onClick={()=>form.setFieldsValue({config_toml:clientTemplate})}>{t("客户端模板")}</Button><Button onClick={async()=>{const k=await generateRatholeKeypair();Modal.info({title:t("Noise 密钥对"),content:<pre>{`Private Key:\n${k.private_key}\n\nPublic Key:\n${k.public_key}`}</pre>})}}>{t("生成 Noise 密钥")}</Button></Space><Form.Item name="config_toml" label="TOML" rules={[{required:true}]}><Input.TextArea autoSize={{minRows:18,maxRows:30}} style={{fontFamily:"monospace"}} /></Form.Item><Form.Item name="enabled" valuePropName="checked" label={t("随后台启动") }><Switch/></Form.Item></Form></Modal>
   <Modal open={helpOpen} title={t("Rathole 配置帮助")} width={980} footer={<Button type="primary" onClick={()=>setHelpOpen(false)}>{t("知道了")}</Button>} onCancel={()=>setHelpOpen(false)}>
     <Typography>
       <Typography.Title level={4}>{t("一、它解决什么问题")}</Typography.Title>
       <Typography.Paragraph>{t("Rathole 是反向代理内网穿透工具。公网服务器运行服务端，内网机器运行客户端；客户端主动连接服务端，外部访问服务端端口后，流量会转发到内网客户端的本地服务。适合内网网站、远程桌面、SSH、NAS、摄像头和临时开发环境。")}</Typography.Paragraph>
       <Typography.Title level={4}>{t("二、最常见的配置方式")}</Typography.Title>
       <Typography.Paragraph>{t("先在公网机器创建“服务端”配置，再在内网机器创建“客户端”配置。两边的 remote_addr、服务名称和 token 必须对应；服务端 bind_addr 是公网监听地址，客户端 local_addr 是内网实际服务地址。")}</Typography.Paragraph>
       <Divider orientation="left">{t("服务端示例")}</Divider>
       <Typography.Text code>{`[server]\nbind_addr = "0.0.0.0:2333"\ndefault_token = "change-me"\n\n[server.transport]\ntype = "tcp"\n\n[server.services.web]\ntype = "tcp"\nbind_addr = "0.0.0.0:8080"`}</Typography.Text>
       <Divider orientation="left">{t("客户端示例")}</Divider>
       <Typography.Text code>{`[client]\nremote_addr = "公网服务器IP:2333"\ndefault_token = "change-me"\n\n[client.transport]\ntype = "tcp"\n\n[client.services.web]\ntype = "tcp"\nlocal_addr = "127.0.0.1:8080"`}</Typography.Text>
       <Typography.Title level={4}>{t("三、配置选项说明")}</Typography.Title>
       <Typography.Paragraph><Typography.Text strong>server / client：</Typography.Text>{t("顶层运行模式。server 负责公网监听和端口映射，client 负责连接服务端并转发本地服务。一个配置文件通常只放一个顶层区块；选择“自动”时必须只能存在其中一个。")}</Typography.Paragraph>
       <Typography.Paragraph><Typography.Text strong>transport.type：</Typography.Text>{t("tcp 最简单；tls 用于证书加密；noise 使用 Noise 密钥协商；websocket 适合需要通过 WebSocket 网络环境的场景。")}</Typography.Paragraph>
       <Typography.Paragraph><Typography.Text strong>services：</Typography.Text>{t("每个服务是一个映射，名称必须在服务端和客户端一致。type 可填 tcp 或 udp；服务端用 bind_addr 暴露端口，客户端用 local_addr 指向内网服务。")}</Typography.Paragraph>
       <Typography.Paragraph><Typography.Text strong>token / default_token：</Typography.Text>{t("用于服务端和客户端认证。建议修改默认值，并为不同服务使用独立 token。")}</Typography.Paragraph>
       <Typography.Paragraph><Typography.Text strong>heartbeat / retry：</Typography.Text>{t("heartbeat_interval、heartbeat_timeout 用于检测连接；retry_interval 控制客户端断线重连间隔。网络不稳定时可以适当增大超时。")}</Typography.Paragraph>
       <Typography.Paragraph><Typography.Text strong>tcp.proxy：</Typography.Text>{t("可填 socks5:// 或 http:// 代理地址，适合客户端只能通过代理访问公网的网络。")}</Typography.Paragraph>
       <Typography.Text code>{`[client.transport.tcp]\nproxy = "socks5://user:password@127.0.0.1:1080"`}</Typography.Text>
       <Typography.Paragraph>{t("跨境或 IP 被封禁的场景：如果中国内网客户端无法直接连接美国服务端，在“客户端出站代理”中填写可用的 SOCKS5/HTTP 代理，例如 socks5://127.0.0.1:1080。保存后页面会自动写入 [client.transport.tcp] 的 proxy 配置。代理只需要配置在客户端；Rathole 建立连接后，控制流、TCP/UDP 映射数据以及返回方向都会复用这条代理隧道。")}</Typography.Paragraph>
       <Typography.Title level={4}>{t("四、常见使用场景")}</Typography.Title>
       <Typography.Paragraph>{t("内网网站：服务端暴露 8080，客户端 local_addr 指向 127.0.0.1:8080。SSH/RDP：服务类型仍使用 tcp，将 local_addr 指向 22 或 3389。UDP 服务：两边对应服务的 type 都改成 udp。多台内网机器：每台客户端单独建一个配置档案，或在同一客户端配置中增加多个 services。")}</Typography.Paragraph>
       <Typography.Title level={4}>{t("五、安全和排错")}</Typography.Title>
       <Typography.Paragraph>{t("不要把 change-me 留在生产环境；TLS 服务器需要 pkcs12 和 pkcs12_password；Noise 模式需要把客户端 local_private_key 与服务端 remote_public_key 正确配对。启动失败时先点击保存校验，检查地址、端口、token、服务名称和防火墙；公网服务器还需要放行 bind_addr 对应端口。配置修改后 Rathole 会通过配置文件热加载，运行中的连接可能会短暂重连。")}</Typography.Paragraph>
     </Typography>
   </Modal>
 </CrudPage>;
}
