#![allow(clippy::missing_errors_doc)]

use loco_rs::{mailer::Email, prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use super::_entities::email_templates::{self, ActiveModel, Column, Entity, Model};

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct RenderedEmailTemplate {
    pub subject: String,
    pub html_body: String,
    pub text_body: String,
}

impl RenderedEmailTemplate {
    #[must_use]
    pub fn into_email(self, to: String) -> Email {
        Email {
            to,
            subject: self.subject,
            html: self.html_body,
            text: self.text_body,
            ..Default::default()
        }
    }
}

pub async fn render_enabled(
    db: &DatabaseConnection,
    code: &str,
    locals: &Value,
) -> Result<Option<RenderedEmailTemplate>> {
    let Some(template) = email_templates::Entity::find()
        .filter(email_templates::Column::Code.eq(code))
        .filter(email_templates::Column::Enabled.eq(true))
        .one(db)
        .await?
    else {
        return Ok(None);
    };

    Ok(Some(render_model(&template, locals)))
}

#[must_use]
pub fn render_model(template: &Model, locals: &Value) -> RenderedEmailTemplate {
    RenderedEmailTemplate {
        subject: render_template(&template.subject, locals),
        html_body: render_template(&template.html_body, locals),
        text_body: render_template(&template.text_body, locals),
    }
}

#[must_use]
pub fn render_template(source: &str, locals: &Value) -> String {
    let mut rendered = source.to_string();
    let Some(values) = locals.as_object() else {
        return rendered;
    };

    for (key, value) in values {
        let replacement = value_to_string(value);
        rendered = rendered.replace(&format!("{{{{{key}}}}}"), &replacement);
        rendered = rendered.replace(&format!("{{{{ {key} }}}}"), &replacement);
    }

    rendered
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => value.to_string(),
    }
}
