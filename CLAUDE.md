# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Backend / Rust

Run from the repository root unless noted otherwise.

- Start the Loco app in development: `cargo loco start`
- Select an environment explicitly: `cargo loco -e development start`, `cargo loco -e test db migrate`, `cargo loco -e development db seed`
- Run migrations: `cargo loco db migrate`
- Seed fixtures: `cargo loco db seed`
- Recover/create a local admin account: `cargo loco task admin.recover email:admin@example.com password:NewPassword123 name:Admin role-code:super_admin`
- Check formatting: `cargo fmt --all -- --check`
- Apply Rust formatting: `cargo fmt --all`
- Run clippy as CI does: `cargo clippy --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms`
- Run all Rust tests: `cargo test --all-features --all`
- Run one Rust test by name: `cargo test super_admin_can_manage_payments --all-features --all`
- Run tests against the local test database: `DATABASE_URL='postgres://postgres:Vue8484229%40@localhost:5432/gpt_images_test' cargo test --all-features --all`
- Run the playground example: `cargo playground`

The `.cargo/config.toml` aliases `cargo loco` and `cargo loco-tool` to `cargo run --`; the default binary is `gpt_images-cli` at `src/bin/main.rs`. `gpt_images-cli-a` / `gpt_images-cli-b` (`src/bin/gpt_images-cli-{a,b}.rs`) `include!` the same `main.rs`, so one can be rebuilt with `--bin` while the service runs the other; keep CLI code in `main.rs`.

Tests and local development use PostgreSQL by default. `DATABASE_URL` overrides the configured URI. `config/test.yaml` enables `dangerously_truncate` and `dangerously_recreate`; never point tests at a database with data to keep.

### Frontend

Run from `frontend/` or use `npm --prefix frontend ...` from the repo root.

- Install dependencies as CI does: `npm install`
- Start the frontend dev server: `npm run dev`
- Lint frontend source: `npm run lint`
- Apply Biome fixes/formatting: `npm exec -- biome check src/ --write`
- Build frontend assets: `npm run build`
- Preview the frontend build: `npm run preview`

The frontend README documents pnpm equivalents, but CI and recent validation use npm. Build `frontend/dist` before compiling or running the backend when embedded static assets are required: `src/controllers/frontend.rs` embeds `frontend/dist` with `include_dir!`.

### Full validation used for feature work

- Backend: `cargo fmt --all -- --check && cargo check --all-features --all && cargo clippy --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms && DATABASE_URL='postgres://postgres:Vue8484229%40@localhost:5432/gpt_images_test' cargo test --all-features --all`
- Frontend: `npm --prefix frontend run lint && npm --prefix frontend run build`

## Architecture

This is a Loco SaaS/admin template with a Rust backend, SeaORM migrations/entities, PostgreSQL-backed fixtures/tests, and a React/Rsbuild frontend.

- `src/bin/main.rs` launches the Loco CLI with `App` and `migration::Migrator`.
- `src/app.rs` implements Loco `Hooks`: booting, route registration, middleware, background workers, tasks, fixture seeding, and test truncation order.
- `src/controllers/auth.rs` owns `/api/auth`: login, token refresh, logout, current user/session, register/verify, forgot/reset password, magic link, and resend verification.
- `src/controllers/admin/mod.rs` groups admin API routes into core RBAC/multi-tenant routes, operations infrastructure routes, work order routes, payment routes, and extension routes. Admin controllers use `authorize(&ctx, &auth, permission)` for RBAC.
- `src/controllers/docs.rs` serves Swagger UI at `/swagger-ui` and OpenAPI JSON at `/api-docs/openapi.json`; `src/openapi.rs` registers the documented paths and schemas.
- `src/controllers/frontend.rs` serves the embedded React build from `frontend/dist`, falls back to `index.html` for SPA paths, and deliberately lets `/api`, `/api-docs`, and `/swagger-ui` pass through as backend paths.
- `src/errors.rs` and `src/responses.rs` provide the API error/result and response envelope patterns used by controllers.
- `src/i18n/` + `src/middleware/locale.rs` translate Chinese error messages for clients sending `Accept-Language: en…` (the frontend sends its locale). Keys in `src/i18n/en.json` are the Chinese source text, `format!` messages use `{0}`-style placeholders; the `i18n` unit tests fail when a Chinese literal in `src/` lacks a translation (`I18N_DUMP=1 cargo test --lib i18n -- --nocapture` prints all keys).
- `src/models/_entities/` contains SeaORM entity definitions. Domain behavior that is not just CRUD lives in focused model modules such as `src/models/users.rs`, `src/models/rbac.rs`, and supporting modules for settings/email templates.
- `migration/` is a workspace crate registering all SeaORM migrations. Keep new tables/entities/fixtures/truncation order aligned when adding backend resources.
- `src/fixtures/` seeds tenants, users, roles, permissions, menus, data scopes, role grants, settings, email templates, notifications, scheduled tasks, rate limits, and dictionaries. Super admin fixture access depends on `role_permissions.yaml` and `role_menus.yaml`.
- `src/tasks/admin.rs` defines the local `admin.recover` task for creating/resetting an admin and binding an enabled role. `src/tasks/operations.rs` runs due scheduled tasks.
- `src/middleware/rate_limit.rs` applies configured IP/user/path rate limits and records rate limit events.
- `src/middleware/audit.rs` records `/api` requests into `operation_logs` using the per-feature switches in `src/services/audit.rs` (`FEATURES` maps path prefixes to features; switches live in `audit_log_rules` and are edited from 日志中心 → 记录设置). Handler-written logs via `models::admin_logs::record` reuse the request's IP/user agent/duration and suppress the generic entry.
- `src/mailers/auth.rs` renders auth emails; email templates can also be managed through the admin email-template API.
- `src/workers/downloader.rs` is registered as the background worker in `src/app.rs`.
- Login sessions (`src/models/user_sessions.rs`): every login opens a `user_sessions` row and the access token carries its `sid` claim (API-key tokens carry `api_key` instead). `src/middleware/session_guard.rs` rejects tokens whose session was revoked/expired or whose user is disabled (`SESSION_EXPIRED`) and exposes the session to handlers as `controllers::admin::SessionExt`. Revoke with `user_sessions::revoke` / `revoke_user`; both also push `session_revoked` over the live event stream.
- Sensitive handlers (SSH/VNC/RDP connect, running commands and project CLI, deleting/restoring backups, resetting passwords, changing `security.*` settings, purging the recycle bin) call `controllers::admin::require_reauth(&ctx, &session)`, which answers `REAUTH_REQUIRED` until `/api/auth/reauth` confirmed the password; the frontend client prompts (`ReauthHost`) and retries. Request tests that hit these call `prepare_data::confirm_password`.
- Deleting records goes through `src/services/recycle_bin.rs`: `delete_row` snapshots the row plus everything `ON DELETE CASCADE` removes (from the PostgreSQL catalog) and re-links `SET NULL` references on restore; `delete_files` moves files into `storage/recycle_bin/<id>/`; JSON-setting lists use `Payload::ConfigItem`. New deletable resources should use it and be listed in `RESOURCES`.
- `security.*` settings (`src/services/security_settings.rs`, cached ~5s): re-auth window, admin IP allowlist (`src/middleware/ip_allowlist.rs`; forwarding headers are trusted only from a loopback peer, see `services::client_info`), API docs access (`public`/`login`/`disabled`), login captcha mode (`src/services/captcha.rs`) and recycle bin retention.
- SSH sessions (`src/controllers/admin/ssh.rs`) belong to the user who opened them: listing, attaching, browsing files and closing only work on your own (others answer `SSH_SESSION_NOT_FOUND`). Each PTY runs in a detached session host (`src/services/ssh_session_host.rs`: this binary started with `__ssh-session-host`, state under `storage/ssh-sessions/<id>/`, loopback JSON-lines protocol; on Windows hosts run from a copy under `storage/ssh-sessions/bin/` so the build can still be replaced). With `ssh_manager.keep_sessions_on_restart` (default `true`) hosts outlive the server and `ssh::restore_sessions` re-attaches them on startup under the same ids; with `false` a host ends its shell as soon as the server's connection drops. The test environment (and a failing host start) uses an in-process PTY instead. Sessions last until the shell exits unless `ssh_manager.idle_timeout_minutes` / `ssh_manager.max_session_hours` (0 = off) are set; a reaper checks every minute. VNC sessions (`vnc.rs`, `VNC_SESSION_NOT_FOUND`) and live RDP connections (`rdp.rs` registry, `/api/admin/rdp/sessions`, `RDP_SESSION_NOT_FOUND`) are private to their owner the same way. Admins see and force-close everyone's sessions in 在线会话 → 远程连接 (`src/controllers/admin/remote_sessions.rs`, `system:session:remote_list` / `remote_close`); each module exposes `remote_sessions()` / `close_remote_session()` for it. Disabling or deleting a user closes all their SSH/VNC/RDP sessions (`remote_sessions::close_user_sessions`).
- 远程音频 (`src/controllers/admin/remote_audio.rs`, `/api/admin/remote-audio`, permissions `system:remote_audio:list` / `listen`, page `/admin/system/remote-audio`): streams what this machine's speakers play, without the screen. `src/services/audio_capture.rs` records the default playback device via WASAPI loopback (`cpal`, Windows-only dependency) or `parec` from the PulseAudio/PipeWire monitor (Linux); macOS is unsupported. A ticket opens `/ws/{ticket}`: a `format` JSON message, then binary 16-bit LE PCM chunks (40 ms; digital silence is not sent) and `stats` every 2 s. Each listening session is a `remote_audio_sessions` row counting `bytes_sent` / `audio_ms` from open to close (saved every 10 s; `finish_stale_sessions` closes rows a stopped server left `live`). Live sessions appear in 在线会话 → 远程连接 as kind `audio`. The browser player (`frontend/src/stores/remoteAudio.ts`) lives outside React so listening continues across pages; `RemoteAudioIndicator` shows it in the header. `cargo test --lib captures_the_playback_device -- --ignored --nocapture` checks real capture while something plays.
- 数据库管理 (`src/controllers/admin/db_manager.rs`, `/api/admin/db-manager`, permissions `system:database:list` / `manage` / `query`, page `/admin/system/database`): a Navicat-style client for PostgreSQL, MySQL and SQLite. Saved connections (`db_connections`, password AES-GCM-encrypted and bound to owner + id like RDP credentials) are private to their creator (`DB_CONNECTION_NOT_FOUND` for others); `read_only` connections are enforced by the database session itself. `src/services/db_manager/` keeps a sqlx pool per connection + database for catalog queries (`meta.rs`), table browsing with bound filters, grid edits applied in one transaction (updates/deletes must hit exactly one row by primary key) and CSV/JSON/SQL exports. Each SQL editor tab sends a `session_key` and gets its own detached connection, so transactions and session settings span runs; scripts are split by `sql::split_statements` (quotes, comments, dollar quotes, MySQL `DELIMITER`, SQLite trigger bodies). Pools and sessions idle 30 minutes are closed. Opening a connection asks for `require_remote_reauth`. Runs are logged in `db_query_logs` (newest 500 per user + connection); saved queries live in `db_saved_queries`. The frontend editor is CodeMirror 6 (`frontend/src/pages/system/database/SqlEditor.tsx`). `tests/requests/db_manager.rs` covers SQLite end-to-end and PostgreSQL against the test database; the MySQL test is `#[ignore]` and needs `DB_MANAGER_MYSQL_URL`.
- 在线聊天 (`src/controllers/admin/im.rs`, `/api/admin/im`, permission `system:im:chat`, page `/admin/system/im`): direct and group chats between back-office users with text, images and files (stored under `storage/im/<conversation id>/`). Members only see their own conversations (`IM_CONVERSATION_NOT_FOUND` otherwise); contacts follow the user's data scope. New messages and membership changes are pushed as `im` events over the user event stream, which `useRealtimeEvents` re-dispatches as the `admin-im-event` window event. Group events are `system` messages with JSON content rendered client-side; dissolving a group goes to the recycle bin.
- `/api/user/*` is self-service (no admin permission): preferences, `profile` (个人中心, own password and sessions) and `notifications` + `events` (header bell; server-sent events via a single-use ticket from `/api/user/events/ticket`, `src/services/realtime.rs`).
- RDP goes through guacd (`RDP_GUACD_ADDRESS`, default `127.0.0.1:4822`). `src/services/rdp_gateway.rs` starts and restarts the installed guacd whenever that address is on this machine and not answering. `rdp install` (`src/rdp_manager.rs`) installs it without Docker: on Windows an Alpine WSL distro `gpt-images-guacd` (WSL 2, falling back to WSL 1; files under `storage/rdp-gateway/`), on Linux apk/apt/dnf packages; macOS has no package. Under WSL 2 NAT, loopback RDP targets are dialed via the Windows host address (`rdp_gateway::dial_host`). `docker-compose.rdp.yml` remains an alternative. The RDP page reuses the VNC window layout and classes (`vnc.css`), quick bar and touch/trackpad modes (`useVncPointerModes` driven through a Guacamole `PointerSurface` in `rdp/guacamolePointer.ts`).
- Processes started by 命令管理 (`src/services/command_runner.rs`) and 项目命令 runs are registered with `services::child_processes::track` and end with the server: `App::on_shutdown` kills every tracked process tree (Ctrl+C alone does not reach them), and on Windows they also join a kill-on-close job object, so a crash or `taskkill` takes their descendants down too. New code that spawns user commands should track them the same way.
- `src/services/project_cli.rs` powers the admin 项目命令 page (`/admin/system/project-cli`): it lists every CLI command and runs them on the server. The list is built from the CLIs themselves: Loco's commands from the binary's `-h` output, the custom `service`/`vnc`/`rdp`/`data` commands from their clap definitions (`cli_command()` in each module), and tasks from `App::register_tasks`.

## Implemented admin areas

Current backend/admin coverage includes:

- RBAC and admin identity: users, roles, permissions, menus, button/action permissions, role-menu grants, role-permission grants, role data scopes.
- Multi-tenancy: tenants and data scope resolution exposed through `/api/auth/current`.
- System extensions: logs, settings, dictionaries, uploads/material library, email templates.
- Operations: notifications, scheduled tasks and runs, database backups with delivery target settings, rate limits/events, monitoring overview.
- Work orders: CRUD, comments, assignment, status transitions, attachments.
- Certificates & domain mappings (`src/services/certificates/`): Let's Encrypt via ACME DNS-01 on Cloudflare with auto-renewal, a built-in SNI HTTPS reverse proxy for `direct` mappings (443 or custom ports, dual-stack, WebSocket upgrades, A/AAAA records), and Cloudflare Tunnel `tunnel` mappings with a supervised `cloudflared`. Issued keys live under the gitignored `storage/certificates/`.
- Payment foundation: payment channels, orders, callbacks/audit records, refunds, manual state actions. This phase stores provider/channel configuration for yipay, PayPal, Stripe, Alipay, WeChat Pay, TokenPay, BEpusdt, epusdt, and OKPay; it does not implement real third-party SDK calls or public payment callback verification yet.

## Frontend structure

The frontend is a React 18 + TypeScript + Ant Design + TanStack Query + Zustand app built by Rsbuild.

- `frontend/src/app/router.tsx` defines protected admin routes and page-level permission guards.
- `frontend/src/app/menu.ts` defines fallback admin menu metadata used when backend session menus are unavailable.
- `frontend/src/api/client.ts` configures Axios with `/api` base URL, bearer token injection, automatic refresh on 401, and sign-out on refresh failure.
- `frontend/src/stores/auth.ts` persists access/refresh tokens and session data in local storage and centralizes permission checks.
- `frontend/src/layouts/` contains the admin shell/sidebar/header and icon mapping.
- `frontend/src/components/admin/` contains reusable admin table/toolbar/permission components used by system pages.
- `frontend/src/pages/system/` contains feature pages matching backend admin modules: users, roles, menus, permissions, tenants, logs, uploads, settings, notifications, scheduled tasks, backups, monitoring, email templates, work orders, and payments.
- During frontend development, `frontend/rsbuild.config.ts` proxies `/api` to `http://127.0.0.1:5150`. In backend serving mode, Loco serves the embedded `frontend/dist` assets.
- `frontend/src/i18n/` holds the zh-CN/en-US switch: Chinese source text is the message key (`t("保存")`, placeholders as `t("剩余 {0} 天", days)`), English lives in `en.json`, and server-provided names (menu titles, permission names, seeded roles/tenants, home to-dos) go through `tData()`. Switching language reloads the page. New UI text must be wrapped in `t()` and translated: `npm --prefix frontend run i18n:wrap -- <files>` wraps literals, `npm --prefix frontend run i18n:missing` lists untranslated keys, and `tests/i18n-coverage.test.cjs` fails on gaps.

## Configuration and tests

- Environment config lives in `config/development.yaml`, `config/test.yaml`, and `config/production.yaml`.
- `development.yaml` and `test.yaml` default to PostgreSQL URLs and enable `auto_migrate`.
- Rust tests live under `tests/` and use `loco_rs::testing`, `serial_test`, `rstest`, and `insta` snapshots.
- Request tests in `tests/requests/admin.rs` exercise the major admin flows end-to-end through Loco test requests; keep them updated when adding protected admin routes.
- Task tests live under `tests/tasks/` and cover CLI task behavior such as admin recovery.
- When adding a new persisted admin resource, update migration registration, entity modules/prelude, `src/app.rs` truncation order, fixtures/permission grants as needed, OpenAPI registration, frontend API/page/route/menu, and request tests.
- When adding API routes, make sure their path prefix belongs to a feature in `services::audit::FEATURES`; `tests/requests/audit.rs` fails for any unmapped `/api` route. New features default to recording writes only.
- Dark mode: antd components follow `darkTheme` in `frontend/src/app/App.tsx`; hand-written CSS gets generated overrides in `frontend/src/styles/dark-theme.css`. After changing any stylesheet run `npm --prefix frontend run theme:dark`.
- When adding or changing command-line commands/arguments, keep the 项目命令 page in sync:
  - New clap flags/subcommands in Loco or in `service`/`vnc`/`rdp`/`data` appear automatically; write `///` help text on them because it is shown as the description.
  - A new top-level command handled before Loco in `src/bin/main.rs` must expose `cli_command()` and be added to `custom_commands()` and `CUSTOM_ROOTS` in `src/services/project_cli.rs`.
  - A new task, or a task reading a new `vars.cli` key, must be listed in `tasks::TASK_PARAMS` (`src/tasks/mod.rs`); mark passwords/tokens `secret: true`. The `tasks::tests` unit tests fail when this is missed.
  - Optionally add a Chinese note and risk level (`safe`/`caution`/`danger`, `long_running`) in `COMMAND_NOTES`; destructive commands should be `danger`.
