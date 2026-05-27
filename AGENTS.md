# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Commands

### Backend / Rust

- Start the Loco app: `cargo loco start`
- Check formatting: `cargo fmt --all -- --check`
- Run clippy as CI does: `cargo clippy --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery -W rust-2018-idioms`
- Run all Rust tests: `cargo test --all-features --all`
- Run one test by name: `cargo test can_register --all-features --all`
- Run the playground example: `cargo playground`

The `.cargo/config.toml` aliases `cargo loco` and `cargo loco-tool` to `cargo run --`; the default binary is `gpt_images-cli` at `src/bin/main.rs`.

Tests and local development expect PostgreSQL by default. `DATABASE_URL` overrides the configured defaults. CI also starts Redis and sets `REDIS_URL` for tests.

### Frontend

From `frontend/`:

- Install dependencies as CI does: `npm install`
- Start the frontend dev server: `npm run dev`
- Build frontend assets: `npm run build`
- Lint frontend source: `npm run lint`
- Preview the frontend build: `npm run preview`

The frontend README documents pnpm equivalents (`pnpm install`, `pnpm dev`, `pnpm build`), while CI currently uses npm. The backend static middleware serves `frontend/dist`, so build the frontend before running `cargo loco start` when static assets are required.

## Architecture

This is a Loco SaaS starter application with a Rust backend, SeaORM migrations/entities, and a React/Rsbuild frontend.

- `src/bin/main.rs` launches the Loco CLI with `App` and `migration::Migrator`.
- `src/app.rs` implements Loco `Hooks`: booting the app, registering routes, connecting workers, registering tasks, truncating tables, and seeding fixtures.
- `src/controllers/auth.rs` owns the `/api/auth` API surface: register, verify, login, forgot/reset password, current user, magic link, and resend verification email.
- `src/models/_entities/` contains SeaORM generated entity definitions. Put domain behavior in `src/models/users.rs`, which wraps user lookup, validation, password hashing, JWT generation, verification/reset tokens, and magic-link state changes.
- `migration/` is a separate workspace crate that registers migrations. The current migration creates the `users` table used by the generated entity and auth model.
- `src/mailers/auth.rs` renders auth email templates from `src/mailers/auth/{welcome,forgot,magic_link}/`; mailers use `ctx.config.server.full_url()` for links.
- `src/views/auth.rs` defines the serialized auth response shapes returned by controllers.
- `src/workers/downloader.rs` defines the registered background worker; `src/app.rs` wires it into Loco's queue.
- `frontend/` is a React 18 app built by Rsbuild. During frontend development, `rsbuild.config.ts` proxies `/api` to the backend at `http://127.0.0.1:5150`. In backend serving mode, Loco serves `frontend/dist` at `/` with `frontend/dist/index.html` as fallback.

## Configuration and tests

- Environment config lives in `config/development.yaml`, `config/test.yaml`, and `config/production.yaml`.
- `development.yaml` and `test.yaml` default to PostgreSQL URLs and enable `auto_migrate`.
- `test.yaml` has `dangerously_truncate: true` and `dangerously_recreate: true`; do not point tests at a database with data to keep.
- Test fixtures live in `src/fixtures/users.yaml` and are loaded through `App::seed`.
- Rust tests live under `tests/` and use `loco_rs::testing`, `serial_test`, `rstest`, and `insta` snapshots. Snapshot files are committed under `tests/**/snapshots/`.
