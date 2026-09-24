# RDP（3389）远程桌面

系统管理 → RDP 管理支持电脑的新增、编辑、删除、停用，以及浏览器内的画面、鼠标、键盘、触摸、全屏、Windows 键和 Ctrl+Alt+Del。最多同时打开 8 个连接标签，切换后台菜单保留连接；关闭标签、刷新页面或退出登录会断开浏览器连接。登录时勾选“记住此电脑的账号和密码”，后续连接可直接使用已保存凭据，也可以取消复用来更新密码，或清除已保存凭据。

RDP 和 VNC 使用不同协议。这里使用 Apache Guacamole 的 guacd 转换 RDP，浏览器不需要安装插件或启动 mstsc。架构：浏览器 → 本项目 WebSocket → guacd → Windows:3389。

## 启动网关

Windows 可使用 Docker Desktop 的 Linux 容器模式：

前提是 Windows 虚拟机平台和 BIOS/UEFI 虚拟化已启用，WSL2 能正常启动。若 `wsl --status` 报告未启用虚拟化，Docker Linux 引擎无法启动，需先由机器管理员完成系统配置并按提示重启；也可以在另一台 Linux 机器部署 guacd。

```powershell
docker compose -f docker-compose.rdp.yml up -d
```

项目后端默认访问 `127.0.0.1:4822`。如使用另一台服务器上的 guacd，在启动后端前设置：

```powershell
$env:RDP_GUACD_ADDRESS = '网关的内网地址:4822'
cargo loco start
```

guacd 端口没有应用层登录认证，只允许本后端通过回环地址或可信私有网络访问，不要把 4822 暴露到公网。后端在容器中时，应将地址设置为同一私有容器网络中的 `guacd:4822`。生产站点使用 HTTPS/WSS，反向代理需允许 WebSocket Upgrade。

## 已有项目升级

安装前端依赖并构建，同步新增权限与菜单（不会清空业务数据）：

```powershell
cd frontend
npm install
npm run build
cd ..
cargo loco task admin.sync_fixture_baseline
cargo loco db migrate
```

重新登录后台刷新权限。默认管理员角色获授新增权限，其他角色按需分配 `system:rdp:list`（查看）、`system:rdp:connect`（连接）和 `system:rdp:update`（管理电脑）。目标配置在首次保存时自动写入 `rdp_manager.targets` 系统设置，不需要重新 seed 数据库。

保存凭据需要新增的 `rdp_credentials` 数据表。升级后重启后端，启用 `auto_migrate` 的配置会自动迁移；否则先执行上面的迁移命令（选择实际部署环境）。密码采用 AES-256-GCM 加密，仅当前后台用户可复用，API 不返回密码。凭据绑定目标标识、主机和端口；更换目标地址或 JWT 加密密钥后需重新输入并保存。勾选记住后，凭据在提交连接时保存，即使网关尚未启动也不必下次重复输入；如果密码输入错误，取消“使用已保存的账号和密码”后重新填写即可。

## 连接电脑

1. 在目标 Windows 上启用“远程桌面”，允许目标账号远程登录并放行 RDP 端口。Windows 家庭版不提供系统自带的 RDP 服务端，通常需专业版、企业版或 Server。
2. 打开“RDP 管理”→“管理电脑”→“添加电脑”，填写名称、IP/域名及端口（默认 3389）。地址必须能从 **guacd** 访问；Docker Desktop 连接当前 Windows 宿主机时使用 `host.docker.internal`，不要填 `127.0.0.1`。
3. 默认校验证书；目标使用自签名证书时，在确认电脑可信后开启“允许自签名 / 不受信任证书”。连接使用 NLA。
4. 选择电脑、点击“连接桌面”，输入 Windows 用户名、账号密码（不是 Windows Hello PIN），域可留空。点击画面后即可发送键盘和鼠标操作。

Windows RDP 的会话行为由目标系统决定，可能锁定本地屏幕，不能保证像 VNC 一样镜像当前物理控制台。此实现不提供音频、文件传输或共享会话。

## 排错与验证

- “无法连接 RDP 网关”：检查 guacd 容器、4822 端口及 `RDP_GUACD_ADDRESS`。
- 登录失败：检查用户名、密码、域、远程登录权限、NLA、证书及目标防火墙；使用 `docker compose -f docker-compose.rdp.yml logs guacd` 查看网关日志。
- 真机验收：连接目标 → 移动鼠标并打开记事本输入 → 调整窗口/全屏 → Ctrl+Alt+Del → 切换菜单返回 → 关闭标签 → 重新连接；确认断开会终止网关连接，失败连接可以重新登录。
- SSH 本机终端在 Windows 上改用 `powershell.exe -NoLogo -NoProfile -NoExit`，其他平台继续使用 `$SHELL` 或 `/bin/sh`。远程 SSH 仍需安装 OpenSSH 客户端，目标运行 SSH 服务。

协议与客户端参考：[Apache Guacamole protocol](https://guacamole.apache.org/doc/gug/guacamole-protocol.html)、[JavaScript API](https://guacamole.apache.org/doc/gug/guacamole-common-js.html)。
