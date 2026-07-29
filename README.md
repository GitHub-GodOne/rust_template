# Welcome to Loco :train:

[Loco](https://loco.rs) is a web and API framework running on Rust.

This is the **SaaS starter** which includes a `User` model and authentication based on JWT.
It also include configuration sections that help you pick either a frontend or a server-side template set up for your fullstack server.


## Quick Start

```sh
cargo loco start
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

## 新服务器管理员初始化/重置密码

如果登录时报 `login attempt with non-existent email email=admin@example.com`，说明当前数据库里没有默认管理员。可以用恢复任务创建或重置管理员：

```sh
cargo loco -e development task admin.recover email:admin@example.com password:NewPassword123 name:Admin role-code:super_admin
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

`admin.recover` 会在用户不存在时创建用户，用户存在时重置密码，并绑定 `super_admin` 角色。生产环境第一次登录后请立即修改密码。

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

