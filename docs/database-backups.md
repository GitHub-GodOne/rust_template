# 数据库备份配置

数据库备份默认使用当前项目的 `DATABASE_URL`（也就是后端配置里的数据库）。在“系统管理 → 数据库备份 → 立即备份”中选择“当前项目数据库（默认）”即可。

如果需要备份其他 PostgreSQL 数据库，先在“数据库管理”中保存一个 PostgreSQL 连接，再在创建备份时选择它。连接密码会加密保存，备份记录会记录所选连接名称；MySQL、SQLite 连接不能用于 `pg_dump` 备份。

备份推送目标可以在创建备份时选择，也可以留空使用系统设置中的默认目标。Telegram 群组在“系统设置”中编辑 `backup.telegram_destinations`，格式如下：

```json
[
  {
    "id": "production",
    "name": "生产群",
    "bot_token": "123456:your-bot-token",
    "chat_id": "-1001234567890",
    "enabled": true
  },
  {
    "id": "staging",
    "name": "测试群",
    "bot_token": "123456:your-bot-token",
    "chat_id": "-1009876543210",
    "enabled": true
  }
]
```

目标填写 `telegram` 会发送到所有启用的 Telegram 目标，填写 `telegram:production` 或 `telegram:staging` 可以只发送到指定群组。成功的 PostgreSQL 备份会作为文件上传，同时附带备份摘要；旧的单个 `backup.telegram_bot_token` 和 `backup.telegram_chat_id` 配置仍然兼容。

国内网络访问 Telegram 时，在“系统设置”中编辑 `http_client.runtime`，保留其他字段并设置代理：

```json
{
  "enabled": true,
  "request_timeout_seconds": 120,
  "connect_timeout_seconds": 20,
  "pool_idle_timeout_seconds": 90,
  "proxy_enabled": true,
  "proxy_url": "http://127.0.0.1:7890",
  "danger_accept_invalid_certs": false,
  "user_agent": null
}
```

`proxy_url` 支持 HTTP、HTTPS 和 SOCKS5，例如 `socks5://127.0.0.1:1080`。这个代理是后端出站 HTTP 的全局代理，因此 Telegram、Webhook 等推送都会使用它。`pg_dump` 需要安装在运行后端的机器上。

定时任务的 payload 可以指定数据库和推送目标，例如：

```json
{
  "connection_id": 12,
  "delivery_targets": ["telegram:production", "telegram:staging"]
}
```

省略 `connection_id` 时定时任务仍备份当前项目数据库；省略 `delivery_targets` 时使用 `backup.delivery_targets` 的默认配置。
