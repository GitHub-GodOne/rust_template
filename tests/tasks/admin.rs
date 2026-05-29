use gpt_images::{
    app::App,
    models::_entities::{roles, user_roles, users},
    tasks::admin::{recover_admin, RecoverAdmin},
};
use loco_rs::{task::Task, task::Vars, testing::prelude::*};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn recover_admin_creates_user_and_binds_super_admin_role() {
    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");
    seed::<App>(&boot.app_context)
        .await
        .expect("Failed to seed database");

    let user = recover_admin(
        &boot.app_context.db,
        "recovery@example.com",
        "new-password",
        Some("Recovery Admin"),
        "super_admin",
        Some(1),
    )
    .await
    .expect("Failed to recover admin");

    assert_eq!(user.email, "recovery@example.com");
    assert_eq!(user.name, "Recovery Admin");
    assert!(user.verify_password("new-password"));
    assert!(user.email_verified_at.is_some());

    let super_admin = roles::Entity::find()
        .filter(roles::Column::Code.eq("super_admin"))
        .one(&boot.app_context.db)
        .await
        .unwrap()
        .unwrap();
    let user_role = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user.id))
        .filter(user_roles::Column::RoleId.eq(super_admin.id))
        .one(&boot.app_context.db)
        .await
        .unwrap();
    assert!(user_role.is_some());
}

#[tokio::test]
#[serial]
async fn recover_admin_resets_existing_user_password_without_duplicate_role() {
    let boot = boot_test::<App>()
        .await
        .expect("Failed to boot test application");
    seed::<App>(&boot.app_context)
        .await
        .expect("Failed to seed database");

    let vars = Vars::from_cli_args(vec![
        ("email".to_string(), "admin@example.com".to_string()),
        ("password".to_string(), "reset-password".to_string()),
        ("name".to_string(), "Recovered Admin".to_string()),
    ]);
    RecoverAdmin
        .run(&boot.app_context, &vars)
        .await
        .expect("Failed to run admin recovery task");

    let user = users::Entity::find()
        .filter(users::Column::Email.eq("admin@example.com"))
        .one(&boot.app_context.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.name, "Recovered Admin");
    assert!(user.verify_password("reset-password"));
    assert!(user.email_verified_at.is_some());

    let super_admin = roles::Entity::find()
        .filter(roles::Column::Code.eq("super_admin"))
        .one(&boot.app_context.db)
        .await
        .unwrap()
        .unwrap();
    let grants = user_roles::Entity::find()
        .filter(user_roles::Column::UserId.eq(user.id))
        .filter(user_roles::Column::RoleId.eq(super_admin.id))
        .all(&boot.app_context.db)
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
}
