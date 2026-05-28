// auth mailer
#![allow(non_upper_case_globals)]

use loco_rs::prelude::*;
use serde_json::json;

use crate::models::{email_templates, users};

static welcome: Dir<'_> = include_dir!("src/mailers/auth/welcome");
static forgot: Dir<'_> = include_dir!("src/mailers/auth/forgot");
static magic_link: Dir<'_> = include_dir!("src/mailers/auth/magic_link");

#[allow(clippy::module_name_repetitions)]
pub struct AuthMailer {}
impl Mailer for AuthMailer {}
impl AuthMailer {
    /// Sending welcome email the the given user
    ///
    /// # Errors
    ///
    /// When email sending is failed
    pub async fn send_welcome(ctx: &AppContext, user: &users::Model) -> Result<()> {
        let locals = json!({
          "name": user.name,
          "verifyToken": user.email_verification_token,
          "domain": ctx.config.server.full_url()
        });
        Self::send_template(ctx, "auth_welcome", &welcome, user.email.clone(), locals).await
    }

    /// Sending forgot password email
    ///
    /// # Errors
    ///
    /// When email sending is failed
    pub async fn forgot_password(ctx: &AppContext, user: &users::Model) -> Result<()> {
        let locals = json!({
          "name": user.name,
          "resetToken": user.reset_token,
          "domain": ctx.config.server.full_url()
        });
        Self::send_template(
            ctx,
            "auth_forgot_password",
            &forgot,
            user.email.clone(),
            locals,
        )
        .await
    }

    /// Sends a magic link authentication email to the user.
    ///
    /// # Errors
    ///
    /// When email sending is failed
    pub async fn send_magic_link(ctx: &AppContext, user: &users::Model) -> Result<()> {
        let locals = json!({
          "name": user.name,
          "token": user.magic_link_token.clone().ok_or_else(|| Error::string(
                    "the user model not contains magic link token",
            ))?,
          "host": ctx.config.server.full_url()
        });
        Self::send_template(
            ctx,
            "auth_magic_link",
            &magic_link,
            user.email.clone(),
            locals,
        )
        .await
    }

    async fn send_template(
        ctx: &AppContext,
        code: &str,
        fallback: &Dir<'_>,
        to: String,
        locals: serde_json::Value,
    ) -> Result<()> {
        if let Some(rendered) = email_templates::render_enabled(&ctx.db, code, &locals).await? {
            Self::mail(ctx, &rendered.into_email(to)).await
        } else {
            Self::mail_template(
                ctx,
                fallback,
                mailer::Args {
                    to,
                    locals,
                    ..Default::default()
                },
            )
            .await
        }
    }
}
