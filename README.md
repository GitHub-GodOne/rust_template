# Welcome to Loco :train:

[Loco](https://loco.rs) is a web and API framework running on Rust.

This is the **SaaS starter** which includes a `User` model and authentication based on JWT.
It also include configuration sections that help you pick either a frontend or a server-side template set up for your fullstack server.


## Quick Start

```sh
cargo loco start
```

### 指定启动端口

`start` 支持通过 `--port`（简写 `-p`）临时指定监听端口：

```sh
cargo loco start --port 5166
# 等价简写
cargo loco start -p 5166
```

使用编译后的二进制时同样有效：

```sh
./target/release/gpt_images-cli start --port 5166
```

命令行端口的优先级高于当前环境配置文件中的 `server.port`。不传
`--port` 时才会使用 `config/<environment>.local.yaml` 或
`config/<environment>.yaml` 中配置的端口。首次安装且配置文件尚不存在时，
该参数也会作为 `/setup` 安装服务的端口。

## 开机启动与自动重启

后端把 `frontend/dist` 嵌入二进制；必须先构建前端，再构建 release 二进制。
`build.rs` 会追踪前端资源变化，确保更新网页后重新编译后端。构建后再安装为系统服务：

```sh
npm --prefix frontend run build
cargo build --release --bin gpt_images-cli
```

安装后会立即启动、随系统开机启动，并在进程异常退出后自动重启。默认服务名为
`gpt-images`，默认运行 `server` 模式；命令中的端口和监听地址优先于配置文件。

### Linux（systemd）

```sh
sudo ./target/release/gpt_images-cli service install \
  --environment development \
  --port 5155 \
  --binding 0.0.0.0

sudo ./target/release/gpt_images-cli service status
sudo ./target/release/gpt_images-cli service restart
sudo ./target/release/gpt_images-cli service stop
sudo ./target/release/gpt_images-cli service start
sudo ./target/release/gpt_images-cli service uninstall
```

系统级安装会创建 `/etc/systemd/system/gpt-images.service`，使用
`Restart=always`。实时查看日志：

```sh
sudo journalctl -u gpt-images.service -f
```

不希望使用管理员权限时，可以给所有命令增加 `--user`。用户级 systemd 服务默认在
用户登录后启动；如需在用户未登录时也随 Linux 开机启动，可额外执行：

```sh
sudo loginctl enable-linger "$USER"
./target/release/gpt_images-cli service install --user --environment production
./target/release/gpt_images-cli service restart --user
```

### macOS（launchd）

系统级服务写入 `/Library/LaunchDaemons`，需要管理员权限：

```sh
sudo ./target/release/gpt_images-cli service install --environment production --port 5150
sudo ./target/release/gpt_images-cli service status
sudo ./target/release/gpt_images-cli service restart
sudo ./target/release/gpt_images-cli service uninstall
```

系统服务以 `root` 运行时，日志位于 `/var/log/gpt-images.log` 和
`/var/log/gpt-images.error.log`。通过 `sudo` 安装且服务使用当前登录账户运行时，日志会
写入该账户的 `~/Library/Logs`，避免 launchd 因无权创建 `/var/log` 文件而以
`EX_CONFIG(78)` 退出。安装命令也会输出最终采用的两个日志路径。

也可以增加 `--user` 安装到 `~/Library/LaunchAgents`，此时无需 `sudo`，服务会在用户
登录后启动，日志同样位于 `~/Library/Logs`。

### macOS 注销后仍需使用网页

后端及 PostgreSQL 都应使用系统级 LaunchDaemon。`--run-as pikju` 只指定运行账户，
不会把系统服务改成登录后才运行的 LaunchAgent。不要给后台常驻服务加 `--user`。

更新代码后，构建前端及二进制，再重启已安装服务即可，无需先停止再重新安装：

```sh
npm --prefix frontend install
npm --prefix frontend run build
cargo build --release --bin gpt_images-cli
sudo ./target/release/gpt_images-cli service restart
```

只有端口、环境、工作目录、运行账户或服务配置改变时，才需要再次执行 `service install`。
macOS 服务配置启用 `GPT_IMAGES_DB_WATCHDOG=1`：每次检查前等待 5 秒，数据库查询最多
等待 3 秒，连续失败 3 次后退出，让 launchd 重启。健康时不会定时重启；最坏约 24 秒
判定持续故障，之后的启动时间取决于数据库是否已恢复。`KeepAlive` 只负责进程退出后的
重启，不能自行发现仍存活但卡住的进程。

开发环境的默认数据库连接池允许 10 个连接，连接和获取连接超时为 5000 毫秒，
空闲超时为 300000 毫秒。可以通过 `DB_MAX_CONNECTIONS`、`DB_CONNECT_TIMEOUT`、
`DB_ACQUIRE_TIMEOUT`、`DB_IDLE_TIMEOUT` 覆盖。网页初始化和凭证刷新遇到网络、超时
或服务端错误时保留登录凭证；页面提供重试按钮，刷新凭证明确失效才会清除登录状态。

系统休眠期间服务不能处理网页请求，watchdog 也不会执行。若 Mac 要长期作为服务器，
在系统设置中启用接通电源时防止自动休眠；下面的命令可检查当前休眠策略和两个服务：

```sh
pmset -g custom
sudo launchctl print system/top.pikju.gpt-images
sudo launchctl print system/homebrew.mxcl.postgresql@14
```

如仍出现超时，记录发生时间并检查 `~/Library/Logs/gpt-images.log`、
`~/Library/Logs/gpt-images.error.log` 以及 PostgreSQL 日志。`/_ping` 返回正常只说明
HTTP 服务响应，不能证明数据库查询正常。

### Windows（任务计划程序）

在“以管理员身份运行”的 PowerShell 中执行。Windows 版本使用系统的任务计划程序，
实现开机启动和失败重试：

```powershell
.\gpt_images-cli.exe service install --environment production --port 5150
.\gpt_images-cli.exe service status
.\gpt_images-cli.exe service restart
.\gpt_images-cli.exe service stop
.\gpt_images-cli.exe service start
.\gpt_images-cli.exe service uninstall
```

增加 `--user` 后会改为当前用户登录时启动，不需要系统级开机任务。

所有平台都支持以下安装参数：

- `--mode server-and-worker`：同时运行 Web 服务和后台任务；也可使用 `server` 或 `all`。
- `--working-directory /path/to/project`：指定配置文件和静态资源所在的项目目录。
- `--run-as USER`：指定 Linux/macOS 系统级服务的运行账户。
- `--name NAME`：安装多个不同名称的服务，后续管理命令需传相同的名称。
- `--no-start`：只安装并启用开机启动，本次不立即启动。

服务配置会保存安装时的二进制和工作目录绝对路径。移动二进制或项目目录后，应使用
新的路径再次执行 `service install` 覆盖配置。同名安装也可用于更新端口、环境或模式。

## Windows VNC 服务安装与启动

后台“VNC 管理”的本机目标连接后端所在机器的 `127.0.0.1:5900`，需要该机器先运行
VNC Server。目前项目的自动安装和服务控制命令仅支持 Windows，使用 TightVNC
（服务名 `tvnserver`）。请在目标 Windows 机器上，以管理员身份打开 PowerShell，
进入 `gpt_images-cli.exe` 所在目录执行：

```powershell
# 检查 TightVNC 服务及本机 5900 端口
.\gpt_images-cli.exe vnc status

# 未安装时通过 winget 安装，然后配置自动启动并启动服务
.\gpt_images-cli.exe vnc install

# 安装后再次检查端口是否可连接
.\gpt_images-cli.exe vnc status
```

如果程序已加入 PATH，可以直接使用：

```powershell
gpt_images-cli vnc install
```

仅使用已安装的 TightVNC，不下载或安装软件：

```powershell
.\gpt_images-cli.exe vnc install --no-install
```

启动已安装但未运行的服务（同时设置开机自动启动）：

```powershell
.\gpt_images-cli.exe vnc start
```

从源码运行时，在项目根目录使用：

```powershell
cargo loco vnc status
cargo loco vnc install
cargo loco vnc start
```

安装命令要求 `winget` 可用并能下载软件；如果系统无法运行 `winget`，需要先修复它，
或手动安装 TightVNC Server 后再检查服务。服务已运行时不必重复执行 `install` 或
`start`；当前实现可能返回“服务已启动”的错误，此时用 `vnc status` 检查即可。
`install` 和 `start` 本身不验证端口可用性，请以随后执行的 `status` 和实际连接为准。

在 TightVNC 中配置访问密码，再在后台“VNC 管理”选择“本机 VNC”连接；这些命令不会
代替你设置 VNC 密码。Linux/macOS 需要自行配置 VNC 服务。更多说明见 [Windows VNC](docs/vnc.md)。

## 数据库迁移到另一台服务器

本项目默认使用 PostgreSQL。迁移时需要同时迁移数据库、配置文件和本地文件存储；只复制
数据库不会包含上传的图片、附件等本地文件。迁移前先停止旧服务器上的应用，避免导出期间
仍有写入。

### 1. 旧服务器导出

Linux/macOS：

```sh
pg_dump --format=custom --no-owner --no-acl \
  --dbname="postgresql://DB_USER:DB_PASSWORD@127.0.0.1:5432/DB_NAME" \
  --file=/tmp/gpt-images.dump
```

Windows PowerShell：

```powershell
$env:PGPASSWORD = "DB_PASSWORD"
pg_dump.exe --format=custom --no-owner --no-acl `
  --host=127.0.0.1 --port=5432 --username=DB_USER --dbname=DB_NAME `
  --file=C:\backup\gpt-images.dump
Remove-Item Env:PGPASSWORD
```

然后把 dump 文件复制到新服务器，例如：

```sh
scp /tmp/gpt-images.dump NEW_USER@NEW_SERVER:/tmp/gpt-images.dump
```

如果使用本地存储，还要复制存储根目录。该目录以后台“存储配置”中的本地根目录为准，
例如：

```sh
rsync -a /opt/gpt-images/storage/ NEW_USER@NEW_SERVER:/opt/gpt-images/storage/
```

### 2. 新服务器创建数据库并导入

先在新服务器创建空数据库和数据库用户（名称可以与旧服务器不同）：

```sh
sudo -u postgres createuser --pwprompt DB_USER
sudo -u postgres createdb --owner=DB_USER DB_NAME
```

导入数据：

```sh
pg_restore --exit-on-error --no-owner --no-acl \
  --dbname="postgresql://DB_USER:DB_PASSWORD@127.0.0.1:5432/DB_NAME" \
  /tmp/gpt-images.dump
```

如果目标数据库不是本机，把 `127.0.0.1` 改成 PostgreSQL 主机地址。Windows 上使用
`pg_restore.exe`，参数相同。

### 3. 更新新服务器配置并验证

把旧服务器的 `config/production.local.yaml` 安全复制到新服务器，或只更新其中的
`database.uri`、JWT 密钥和存储配置。数据库连接地址也可以用环境变量覆盖：

```sh
export DATABASE_URL='postgres://DB_USER:DB_PASSWORD@127.0.0.1:5432/DB_NAME'
./gpt_images-cli -e production db migrate
./gpt_images-cli -e production task admin.unlock email:admin@example.com
./gpt_images-cli -e production start
```

Windows PowerShell：

```powershell
$env:DATABASE_URL = "postgres://DB_USER:DB_PASSWORD@127.0.0.1:5432/DB_NAME"
.\gpt_images-cli.exe -e production db migrate
.\gpt_images-cli.exe -e production start
```

确认可以登录、后台菜单和历史数据正常后，再安装开机服务。不要在已有数据的新服务器
上执行 `db seed --reset`，它会清空并重置 fixture 相关数据；普通迁移只执行
`db migrate`。

### 使用项目自带命令导入导出

项目提供跨平台的 `data export` 和 `data import` 命令。它们要求服务器已安装
PostgreSQL 客户端工具 `pg_dump` 和 `pg_restore`，不要求安装完整 PostgreSQL 服务端。
Linux、macOS 和 Windows 使用相同参数。

默认优先读取 `config/<environment>.local.yaml`，其次读取
`config/<environment>.yaml` 中的 `database.uri`。配置文件存在时不需要重复写数据库连接串。

```sh
DATABASE_URL='postgres://DB_USER:DB_PASSWORD@127.0.0.1:5432/DB_NAME' \
  ./gpt_images-cli data export --file=/tmp/gpt-images.dump
```

Windows PowerShell：

```powershell
$env:DATABASE_URL = "postgres://DB_USER:DB_PASSWORD@127.0.0.1:5432/DB_NAME"
.\gpt_images-cli.exe data export --file C:\backup\gpt-images.dump
```

如果没有配置文件，或需要临时指定数据库，才传入 `--database-url`。在新服务器创建空数据库后导入：

```sh
./gpt_images-cli data import \
  --database-url 'postgres://DB_USER:DB_PASSWORD@127.0.0.1:5432/DB_NAME' \
  --file=/tmp/gpt-images.dump
```

Windows PowerShell：

```powershell
.\gpt_images-cli.exe data import `
  --database-url "postgres://DB_USER:DB_PASSWORD@127.0.0.1:5432/DB_NAME" `
  --file C:\backup\gpt-images.dump
```

如果不指定 `--file`，导出命令会在当前目录生成带时间戳的 `.dump` 文件。导入建议只对
新建的空数据库执行；完成后运行 `db migrate` 再启动应用。数据库之外的本地图片、附件
仍需单独复制存储根目录。

### 使用 Loco 自带命令导入导出 YAML fixtures

本项目使用的 Loco 0.16.4 已内置 `db seed --dump`，不需要新增导出工具。
在项目根目录执行，命令使用对应环境的数据库配置，不必重复传入连接串；
`-e development` 与 `-e production` 应按实际数据库环境选择。

导出所有表到默认的 `src/fixtures` 目录（一张表生成一个 `<表名>.yaml`）：

```sh
cargo loco -e development db seed --dump
```

生产环境指定导出目录。建议先导出到独立目录检查，避免覆盖项目的初始化数据：

```sh
cargo loco -e production db seed --dump --from backups/fixtures
```

只导出指定表，表名用逗号分隔：

```sh
cargo loco -e production db seed --dump-tables roles,permissions,menus,role_permissions,role_menus --from backups/fixtures
```

确认需要更新项目 fixtures 后，可直接指定 `--from src/fixtures`。
虽然参数名为 `--from`，在导出模式下它表示**输出目录**；同名 YAML 文件会被覆盖。

使用编译后的程序（Linux/macOS）：

```sh
./gpt_images-cli -e production db seed --dump --from backups/fixtures
```

Windows PowerShell：

```powershell
.\gpt_images-cli.exe -e production db seed --dump --from backups/fixtures
```

已有数据库要更新菜单分组、顺序、权限和角色授权，可使用专用同步任务：

```sh
./gpt_images-cli -e production task admin.sync_fixture_baseline
```

Windows 使用 `.\gpt_images-cli.exe`；开发环境将 `production` 换成
`development`。此任务读取编译进程序的 `menus.yaml`、`permissions.yaml`、
`role_menus.yaml`、`role_permissions.yaml`，所以修改这些文件后必须重新构建程序并部署
到目标机器。执行后刷新网页，重新加载登录用户的菜单。

页面“菜单管理”中的调整只保存到数据库，不会自动更新源码 fixtures。当前默认分组为
首页、存储管理、接口管理、便捷管理、系统管理、远程管理；`parent_id` 决定分组，
同组内按 `sort_order`、`id` 升序排列。同步任务会更新分组和排序，保留目标数据库里的
菜单 ID；页面按路径匹配，分组按名称和父菜单匹配，并映射角色菜单授权。未在 fixtures
中的自定义菜单和授权不会被删除。

导入 fixtures 时去掉 `--dump`。先确保目标数据库已迁移，且不会与待导入记录产生主键冲突：

```sh
cargo loco -e production db migrate
cargo loco -e production db seed --from backups/fixtures
```

Windows 编译后的程序：

```powershell
.\gpt_images-cli.exe -e production db migrate
.\gpt_images-cli.exe -e production db seed --from backups/fixtures
```

导入只加载 `src/app.rs` 的 `App::seed` 中登记的表及对应 YAML，并不会自动导入目录中的
所有文件。只导出部分表的目录也不能直接当成完整 seed 目录使用，需补齐 seed 所需文件。
不要在已有业务数据的数据库上使用 `db seed --reset`，该选项会重置数据库。
导出的文件可能含真实账号、密码哈希、密钥等敏感数据，提交到仓库前务必检查。
fixtures 适合保存初始化数据；完整数据库迁移仍建议使用前述 PostgreSQL `.dump` 备份方式，
上传图片、附件等本地文件仍需另行复制。

## 首次安装向导

已有配置文件时，正常启动也会检查 PostgreSQL 目标数据库：数据库缺失时自动创建，
随后执行配置启用的自动迁移。数据库账号需要 `CREATEDB` 权限；PostgreSQL 服务本身
仍需安装并运行。连接失败时会提示检查服务、网络或权限，不会把连接失败当作空数据库重置。
测试环境不执行此启动检查。新建库不会自动填入演示管理员；按下面的管理员初始化说明配置账号。

执行 `start` 时，应用会按下面顺序查找当前环境的配置：

1. `config/<environment>.local.yaml`
2. `config/<environment>.yaml`

如果两个文件都不存在，后端不会直接退出，而是启动首次安装服务。浏览器访问：

```text
http://127.0.0.1:5150/setup
```

安装向导会配置项目使用的 PostgreSQL 数据库，并完成：

- 测试数据库地址、账号和网络连接。
- PostgreSQL 数据库不存在时自动创建数据库。
- 执行全部数据库迁移并创建系统表。
- 全新数据库写入默认菜单、权限、系统设置和存储配置。
- 创建首个超级管理员并移除 fixture 中的演示账户。
- 生成随机 JWT 密钥，将配置原子写入 `config/<environment>.local.yaml`。
- 自动关闭安装服务，并在同一进程中切换到正式应用。

配置文件包含数据库密码，在 Unix 系统中会设置为仅当前用户可读写。已有 `users` 数据的数据库只执行迁移并保留原有账户，不会清空数据或覆盖管理员密码。

指定其他环境、端口或配置目录时，安装服务会沿用对应参数：

```sh
LOCO_CONFIG_FOLDER=/opt/gpt-images/config \
  cargo loco -e production start --port 5150
```

前端资源由后端内嵌提供，发布或直接运行编译后二进制前先构建前端：

```sh
npm --prefix frontend run build
cargo build --release --bin gpt_images-cli
./target/release/gpt_images-cli -e production start
```

```sh
$ cargo loco start
Finished dev [unoptimized + debuginfo] target(s) in 21.63s
    Running `target/debug/myapp start`

    :
    :
    :

controller/app_routes.rs:203: [Middleware] Adding log trace id

                      ▄     ▀
                                 ▀  ▄
                  ▄       ▀     ▄  ▄ ▄▀
                                    ▄ ▀▄▄
                        ▄     ▀    ▀  ▀▄▀█▄
                                          ▀█▄
▄▄▄▄▄▄▄  ▄▄▄▄▄▄▄▄▄   ▄▄▄▄▄▄▄▄▄▄▄ ▄▄▄▄▄▄▄▄▄ ▀▀█
 ██████  █████   ███ █████   ███ █████   ███ ▀█
 ██████  █████   ███ █████   ▀▀▀ █████   ███ ▄█▄
 ██████  █████   ███ █████       █████   ███ ████▄
 ██████  █████   ███ █████   ▄▄▄ █████   ███ █████
 ██████  █████   ███  ████   ███ █████   ███ ████▀
   ▀▀▀██▄ ▀▀▀▀▀▀▀▀▀▀  ▀▀▀▀▀▀▀▀▀▀  ▀▀▀▀▀▀▀▀▀▀ ██▀
       ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                https://loco.rs

environment: development
   database: automigrate
     logger: debug
compilation: debug
      modes: server

listening on http://localhost:5150
```

## Full Stack Serving

You can check your [configuration](config/development.yaml) to pick either frontend setup or server-side rendered template, and activate the relevant configuration sections.


## Getting help

Check out [a quick tour](https://loco.rs/docs/getting-started/tour/) or [the complete guide](https://loco.rs/docs/getting-started/guide/).

## 前端编译：
 - npm --prefix frontend run lint
 - npm --prefix frontend run build

## 登录与账号解锁

开发环境登录前，需要保证 PostgreSQL 可连接并启动后端：

```sh
cargo loco start
```

后端正常启动后会监听 `http://127.0.0.1:5150`。可以用健康检查确认服务状态：

```sh
curl http://127.0.0.1:5150/_ping
```

前端开发服务器在另一个终端启动：

```sh
npm --prefix frontend run dev
```

登录页会直接显示后端返回的具体原因：

- `账号或密码错误，还可尝试 N 次`：密码错误，连续失败 3 次后锁定。
- `账号已因登录失败次数过多被锁定，请明天再试`：账号锁定至当天结束。
- `无法连接后端服务，请先在项目目录运行 cargo loco start`：后端未启动、端口不可达或启动失败。

如果密码正确，只需要立即解除锁定，在项目根目录运行：

```sh
cargo loco -e development task admin.unlock email:admin@example.com
```

`admin.unlock` 只清除登录失败次数和锁定时间，不会修改密码。任务会直接连接数据库，不要求 Web 服务正在运行，但使用的 `DATABASE_URL` 和环境必须与后端一致。

生产环境或编译后二进制运行方式：

```sh
./gpt_images-cli -e production task admin.unlock email:admin@example.com
```

如果密码也忘记了，使用 `admin.recover` 重置密码。该任务现在也会同时解除账号锁定：

```sh
cargo loco -e development task admin.recover email:admin@example.com password:NewPassword123 name:Admin role-code:super_admin
```

使用自定义数据库时，任务和后端应传入相同地址：

```sh
DATABASE_URL='postgres://user:password@127.0.0.1:5432/gpt_images' \
  cargo loco -e development task admin.unlock email:admin@example.com
```

## 新服务器管理员初始化/重置密码

如果登录时报 `login attempt with non-existent email email=admin@example.com`，说明当前数据库里没有默认管理员。可以用恢复任务创建或重置管理员：

```sh
cargo loco -e development task admin.recover email:admin@example.com password:Vue8484229@ name:Admin role-code:super_admin
```

然后使用下面账号登录：

- 邮箱：`admin@example.com`
- 密码：`NewPassword123`

如果是直接运行编译后的二进制，把 `cargo loco` 换成二进制文件名，例如：

```sh
./gpt_images-cli -e development task admin.recover email:admin@example.com password:NewPassword123 name:Admin role-code:super_admin
```

如果恢复管理员时报错：

```text
Error: Message("enabled role super_admin not found")
```

说明当前数据库里还没有启用的 `super_admin` 角色，需要先执行 seed 插入默认角色、权限和菜单：

```sh
./gpt_images-cli -e development db seed
./gpt_images-cli -e development task admin.recover email:admin@example.com password:NewPassword123 name:Admin role-code:super_admin
```

如果是全新数据库，推荐先执行迁移和 seed，再恢复管理员：

```sh
cargo loco -e development db migrate
cargo loco -e development db seed
cargo loco -e development task admin.recover email:admin@example.com password:NewPassword123 name:Admin role-code:super_admin
```

如果 `db seed` 报重复数据，说明之前可能已经 seed 过一部分。纯开发库或全新部署库可以使用重置 seed：

```sh
./gpt_images-cli -e development db seed --reset
```

注意：`db seed --reset` 会清空并重置 fixtures 相关数据，只适合全新部署或开发库，不要在已有生产数据的库上随便使用。

`admin.recover` 会在用户不存在时创建用户，用户存在时重置密码并解除登录锁定，同时绑定 `super_admin` 角色。生产环境第一次登录后请立即修改密码。

## 永久 API 授权

登录后台后，在 `系统管理 -> API 授权` 中创建 Bearer 密钥。自动生成的密钥以 `eyJ0_` 开头、没有到期时间，也可以填写不含空格的自定义密钥。

密钥可以调用全部 `/api/*` 后端接口，但不会绕过权限：每次请求都使用密钥创建者当前的角色和功能权限。停用密钥、删除所属用户或移除权限后会立即生效。

通用调用示例：

```sh
curl 'http://127.0.0.1:5150/api/auth/current' \
  -H 'Authorization: Bearer eyJ0_your_api_key' \
  -H 'Accept: application/json'
```

命令执行和状态查询仍可直接使用：

```sh
curl -X POST 'http://127.0.0.1:5150/api/command-api/runs' \
  -H 'Authorization: Bearer eyJ0_your_api_key' \
  -H 'Content-Type: application/json' \
  --data '{"name":"third-party-run","working_directory":"/tmp","command_line":"pwd"}'

curl 'http://127.0.0.1:5150/api/command-api/runs/224?limit=200' \
  -H 'Authorization: Bearer eyJ0_your_api_key'
```

系统监控的“主机温度”显示主机提供的真实传感器读数及最高读数，支持 macOS/Linux
等可通过 `sysinfo` 读取温度的环境。不把无法读取的温度当成 0℃，也不根据未经确认的
传感器名称推测 CPU/GPU 温度。温度采集独立于网页访问，每 15 秒刷新读数，每分钟保存
一条 `host_temperatures` 记录（含全部传感器明细），自动保留最近 7 天；休眠期间不采样。
“温度记录”标签页支持时间范围筛选、分页与展开明细。沿用 `system:monitor:view` 权限，
接口为 `/api/admin/monitoring/temperature-history`，支持 `hours`、`page` 和 `page_size`。

执行记录的“取消”会终止对应命令：macOS/Linux 为每次执行创建独立进程组，取消和
超时会同时终止 shell 及组内的子进程、孙进程。取消接口收到进程终止确认后才将记录
标为 `cancelled`；排队期间收到取消请求的命令不会启动。没有当前后端进程登记的旧执行
记录会返回错误，不会仅修改状态来宣称进程已取消。主动脱离进程组的守护进程不属于
此取消范围，命令脚本应以前台方式运行需随记录取消的任务。

命令管理的执行弹窗提供“出错后自动重试”开关，默认关闭。开启后，脚本非零退出或被信号
异常终止时会从头执行，默认最多重试 3 次（连同首次最多执行 4 次），间隔 5 秒；可设置
1–10 次重试、1–3600 秒间隔。正常退出、手动取消、超时及无法启动进程不会重试。
每次重试都会重新执行前置脚本和命令，沿用本次已解析的参数及预览文件路径；请根据脚本
是否适合重复执行选择开启。执行记录保留同一 ID，日志记录各次执行及失败原因，列表和
详情显示已重试次数；等待重试期间也可以取消。

命令执行 API 同样支持可选字段 `retry_on_failure`、`max_retries`、`retry_delay_seconds`，
省略时默认关闭。返回记录中的 `attempt_count` 表示已开始的执行次数。工作流步骤默认
不自动重试。

命令管理中的 `working_directory` 会同时设置子进程的原生工作目录，并在执行脚本开头
再次执行 `cd`；执行日志会显示实际工作目录以及继承到的 `USER`、`HOME`。工作目录只
影响 `pwd` 和相对路径，不会切换操作系统用户，也不会改变用户数据目录。若命令把配置
保存在 `~/Library/Application Support`、`~/.config` 等位置，启动后端的用户必须与
创建这些配置的用户一致。不要用 root 运行需要读取普通用户配置的命令；安装系统服务时
可通过 `service install --run-as USER` 指定运行账户。

如果 n8n 运行在 Docker 容器中，容器里的 `127.0.0.1` 指向 n8n 容器本身。macOS/Windows 通常应改用 `host.docker.internal:5150`，同一 Docker 网络则使用后端服务名和容器端口。

### 文件上传接口

`POST /api/file-api/uploads` 使用 `multipart/form-data` 上传单个文件。Bearer 密钥所属用户需要拥有 `system:upload:create` 权限；不传 `storage_bucket_id` 时，会自动使用当前租户在 `系统管理 -> 存储配置 -> 桶配置` 中设为“默认”的存储桶。本地存储的实际目录由该桶的“本地根目录”配置决定。

如果 Bearer 密钥由未绑定租户的全局管理员创建，请显式传入 `storage_bucket_id`，否则后端无法判断应使用哪个租户的默认存储桶，并会返回 `tenant_id is required`。桶 ID 可在“存储配置 -> 桶配置”表格中查看。

```sh
curl -X POST 'http://127.0.0.1:5150/api/file-api/uploads' \
  -H 'Authorization: Bearer eyJ0_your_api_key' \
  -H 'Accept: application/json' \
  -F 'file=@/Users/example/Documents/report.pdf' \
  -F 'prefix=third-party/2026'
```

成功响应中的主要字段：

- `network_url`：当前推荐访问地址；配置了公开访问 URL 时优先返回公开地址，否则返回需要 Bearer 授权的后端下载地址。
- `download_url`：始终指向后端下载接口，访问时需要携带同一个 Bearer 密钥。
- `public_url`：存储配置存在公开映射时返回，否则为 `null`。
- `local_path`：本地存储返回服务器上的绝对文件路径；S3、R2、MinIO 等远程存储返回 `null`。
- `object_key`：文件在存储桶中的对象路径。

如果后端运行在 Docker 中，`local_path` 是容器内路径；需要把存储目录挂载到宿主机，才能在宿主机上用对应挂载路径直接访问。

可直接导入 n8n 的示例工作流：

- [单文件上传工作流](examples/n8n/file-upload-single.json)
- [多文件上传工作流](examples/n8n/file-upload-multiple.json)
- [图片生成工作流](examples/n8n/image-generation.json)
- [纯生图工作流](examples/n8n/image-generation-direct.json)
- [n8n 配置与调用说明](examples/n8n/README.md)

### Responses 图片生成接口（兼容保留）

`POST /api/image-api/generations` 会同步调用 CPAMC 兼容的 `/v1/responses` 生图接口，等最终图片生成完成后，再通过与 `/api/file-api/uploads` 相同的存储链路保存到素材库。Bearer 密钥所属用户需要拥有 `system:ai_image:generate` 权限。

可以直接在每次请求中传 `api_url` 和 `api_key`，不需要预先创建供应商配置。也可以在 `系统管理 -> AI 图片生成` 中添加并启用一个配置：`base_url` 填 CPAMC API 根地址（例如 `https://api.pikju.top/v1`），`api_key` 填上游密钥。调用时传 `config_key` 可指定配置；不传时自动使用第一个启用的配置。请求中的 `api_url`、`api_key`、模型、尺寸和质量参数优先级更高，可以临时覆盖后台配置。上游密钥不会出现在接口响应和操作日志中。

后台配置中的图片模型、尺寸、质量、存储桶和目录前缀会作为默认值，但无论配置页面中的保存模式是什么，接口生成结果都会进入素材库统一存储。

没有启用的页面配置时，也可以在启动后端前设置环境变量：

```sh
CPAMC_API_KEY='your_cpamc_key' \
CPAMC_BASE_URL='https://api.pikju.top/v1' \
cargo loco start
```

调用示例：

```sh
curl -X POST 'http://127.0.0.1:5150/api/image-api/generations' \
  -H 'Authorization: Bearer eyJ0_your_api_key' \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json' \
  --data '{
    "prompt": "雨后的上海街头，电影感摄影，霓虹灯倒影",
    "api_url": "https://api.pikju.top/v1",
    "api_key": "your_cpamc_key",
    "base_model": "gpt-5.4-mini",
    "image_model": "gpt-image-2",
    "size": "1024x1536",
    "quality": "high",
    "output_format": "png",
    "partial_images": 2,
    "reasoning_effort": "medium",
    "timeout_seconds": 900,
    "prefix": "ai-generated/2026/08",
    "visibility": "public"
  }'
```

可覆盖的生成参数包括：`base_model`、`image_model`、`size`、`quality`、`output_format`、`partial_images`、`reasoning_effort` 和 `timeout_seconds`。如果已经配置了后台供应商，也可以不传 `api_url` 和 `api_key`，只传 `config_key`。

响应中的文件信息位于 `data.file`：

- `network_url`：推荐访问地址；有公开映射时等于公开地址，否则为需要 Bearer 授权的下载地址。
- `public_url`：存储桶配置了公开 URL 映射时返回，否则为 `null`。
- `download_url`：后端下载接口地址，需要携带 Bearer 授权。
- `local_path`：使用本地存储时返回服务器绝对路径，远程对象存储时为 `null`。

未绑定租户的全局管理员调用时，与文件上传接口相同，建议显式传 `storage_bucket_id`。生成图片大小仍受系统设置中的 `upload.max_size_mb` 限制。

### 纯生图与图片编辑接口

不需要 `base_model` 的纯图片接口：

- `POST /api/image-api/images/generations`：直接调用上游 `/v1/images/generations`。
- `POST /api/image-api/images/edits`：使用 `multipart/form-data` 直接调用上游 `/v1/images/edits`。

两个接口都使用平台永久 Bearer 密钥授权，要求密钥所属用户拥有 `system:ai_image:generate` 权限。上游返回的每张图片都会通过 `/api/file-api/uploads` 共用的存储逻辑写入素材库，响应文件列表位于 `data.files`，每项都包含 `network_url`、`download_url`、`public_url` 和 `local_path`。

请求体里的 `api_key` 是上游 AI 服务密钥。推荐只填写密钥本身；如果误填成 `Bearer sk-...`，后端会自动移除 `Bearer ` 前缀。密钥中不能包含换行或其他不可见字符。

纯生图示例：

```sh
curl -X POST 'http://127.0.0.1:5150/api/image-api/images/generations' \
  -H 'Authorization: Bearer eyJ0_your_api_key' \
  -H 'Content-Type: application/json' \
  --data '{
    "prompt": "电影感雨夜城市街道，霓虹灯倒影",
    "api_url": "https://api.pikju.top/v1",
    "api_key": "your_ai_api_key",
    "model": "gpt-image-2",
    "size": "1024x1536",
    "quality": "high",
    "output_format": "png",
    "n": 1,
    "timeout_seconds": 900,
    "prefix": "ai-generated/direct",
    "visibility": "public"
  }'
```

图片编辑示例：

```sh
curl -X POST 'http://127.0.0.1:5150/api/image-api/images/edits' \
  -H 'Authorization: Bearer eyJ0_your_api_key' \
  -F 'image=@/absolute/path/input.png;type=image/png' \
  -F 'prompt=把图片改成细腻的水彩插画风格' \
  -F 'api_url=https://api.pikju.top/v1' \
  -F 'api_key=your_ai_api_key' \
  -F 'model=gpt-image-2' \
  -F 'size=1024x1536' \
  -F 'quality=high' \
  -F 'output_format=png' \
  -F 'n=1' \
  -F 'timeout_seconds=900' \
  -F 'prefix=ai-edited' \
  -F 'visibility=public'
```

编辑时可以重复传 `image` 字段，最多 16 张；也可以额外传一个 PNG `mask` 字段。单张上传图片大小仍受 `upload.max_size_mb` 限制。`n` 支持 1 到 10，默认 1。

这些接口是同步接口。`timeout_seconds` 控制后端等待上游 AI 服务的时间，但不会自动修改 Nginx、Cloudflare、n8n 网关等中间层的读取超时；如果中间层固定为 120 秒，生图超过 120 秒仍需同步提高其 Proxy Read Timeout，或直接通过内网地址调用后端。

### 大文件预览

素材库和文件管理的预览、下载接口会分块返回内容，并支持标准 `Range`、`Content-Range` 和 `Accept-Ranges` 响应。前端预览请求不再使用全局 15 秒超时，因此大图片、PDF、音频和视频不会因为完整文件尚未下载完而被前端提前中止。

## 文件管理公开访问

文件管理会保留中文等 Unicode 文件名，只会替换危险文件名字符，例如 `/`、`\`、`:`、`*`、`?`、`"`、`<`、`>`、`|` 和控制字符。

后台文件默认是私有的，需要登录并拥有文件下载权限才能通过 `/api/admin/files/download` 下载。在文件管理中把文件设置为公开后，会生成后端托管的公开链接：

```text
/public/files/{token}
```

公开文件可以选择设置访问密码；密码会哈希保存，不会明文入库。带密码的公开链接会先显示密码输入页，验证通过后才返回文件内容。

系统设置里的文件根 URL（例如 `/assets`）只是根目录映射元数据；除非你额外用 Nginx 或静态服务映射该目录，否则不要把它当作可直接公开访问的下载地址。推荐复制文件管理里生成的公开链接。

素材库的公开链接使用存储配置里的公开映射地址生成，例如存储配置的公开基础 URL 或桶的公开前缀。素材设置为私有时，只能通过登录后台后的预览/下载接口访问；设置为公开后，复制链接会复制素材记录里的映射 URL。这个 URL 是否能被未登录用户直接访问，取决于你是否已经用 Nginx、静态服务、S3/R2 公共访问等方式真正暴露了对应存储路径。

## 强制清空重置插入seed
如果这是纯开发库，想直接对齐最新 fixtures
- 运行：! cargo loco -e development db seed --reset
