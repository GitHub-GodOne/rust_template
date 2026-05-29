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

The `.cargo/config.toml` aliases `cargo loco` and `cargo loco-tool` to `cargo run --`; the default binary is `gpt_images-cli` at `src/bin/main.rs`.

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
- `src/models/_entities/` contains SeaORM entity definitions. Domain behavior that is not just CRUD lives in focused model modules such as `src/models/users.rs`, `src/models/rbac.rs`, and supporting modules for settings/email templates.
- `migration/` is a workspace crate registering all SeaORM migrations. Keep new tables/entities/fixtures/truncation order aligned when adding backend resources.
- `src/fixtures/` seeds tenants, users, roles, permissions, menus, data scopes, role grants, settings, email templates, notifications, scheduled tasks, rate limits, and dictionaries. Super admin fixture access depends on `role_permissions.yaml` and `role_menus.yaml`.
- `src/tasks/admin.rs` defines the local `admin.recover` task for creating/resetting an admin and binding an enabled role. `src/tasks/operations.rs` runs due scheduled tasks.
- `src/middleware/rate_limit.rs` applies configured IP/user/path rate limits and records rate limit events.
- `src/mailers/auth.rs` renders auth emails; email templates can also be managed through the admin email-template API.
- `src/workers/downloader.rs` is registered as the background worker in `src/app.rs`.

## Implemented admin areas

Current backend/admin coverage includes:

- RBAC and admin identity: users, roles, permissions, menus, button/action permissions, role-menu grants, role-permission grants, role data scopes.
- Multi-tenancy: tenants and data scope resolution exposed through `/api/auth/current`.
- System extensions: logs, settings, dictionaries, uploads/material library, email templates.
- Operations: notifications, scheduled tasks and runs, database backups with delivery target settings, rate limits/events, monitoring overview.
- Work orders: CRUD, comments, assignment, status transitions, attachments.
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

## Configuration and tests

- Environment config lives in `config/development.yaml`, `config/test.yaml`, and `config/production.yaml`.
- `development.yaml` and `test.yaml` default to PostgreSQL URLs and enable `auto_migrate`.
- Rust tests live under `tests/` and use `loco_rs::testing`, `serial_test`, `rstest`, and `insta` snapshots.
- Request tests in `tests/requests/admin.rs` exercise the major admin flows end-to-end through Loco test requests; keep them updated when adding protected admin routes.
- Task tests live under `tests/tasks/` and cover CLI task behavior such as admin recovery.
- When adding a new persisted admin resource, update migration registration, entity modules/prelude, `src/app.rs` truncation order, fixtures/permission grants as needed, OpenAPI registration, frontend API/page/route/menu, and request tests.
