# Windows VNC

本项目已经内置浏览器 noVNC 客户端和 VNC 代理。Windows 本机目标默认使用 `127.0.0.1:5900`，需要目标机先运行 VNC Server。

在 Windows 目标机上执行：

```powershell
gpt_images-cli vnc status
gpt_images-cli vnc install
```

`install` 会检测 `tvnserver` 服务；未安装时通过 `winget` 安装 TightVNC，然后将服务设为自动启动、启动服务，并检查 5900 端口。安装 TightVNC 可能需要管理员权限；命令不会在网页首次连接时静默安装软件。

如果只想使用已安装的软件：

```powershell
gpt_images-cli vnc install --no-install
gpt_images-cli vnc start
```

安装后在后台 VNC 管理页面选择“本机 VNC”。远程访问其他 Windows 电脑时，仍可在管理页面新增 VNC 目标，填写目标地址和端口；目标电脑同样需要安装并放行 VNC Server 的 TCP 5900。不要把 5900 直接暴露到公网，优先使用内网、VPN 或 SSH 隧道。
