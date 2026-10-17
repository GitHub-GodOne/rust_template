pub mod admin;
pub mod operations;

/// A `key:value` parameter accepted by a registered task.
#[derive(Debug, Clone, Copy)]
pub struct TaskParam {
    pub name: &'static str,
    pub required: bool,
    /// Masked in the project CLI page's run history.
    pub secret: bool,
    pub help: &'static str,
}

const fn param(name: &'static str, required: bool, help: &'static str) -> TaskParam {
    TaskParam {
        name,
        required,
        secret: false,
        help,
    }
}

/// Parameters of every task registered in `App::register_tasks`.
///
/// The project CLI page reads this to build its task forms. When a task is
/// added or starts reading a new `vars.cli` key, list it here too;
/// `tests::every_task_parameter_is_documented` fails otherwise.
pub const TASK_PARAMS: &[(&str, &[TaskParam])] = &[
    (
        "admin.recover",
        &[
            param("email", true, "管理员邮箱，不存在时自动创建"),
            TaskParam {
                name: "password",
                required: true,
                secret: true,
                help: "新的登录密码",
            },
            param("name", false, "显示名称，默认取邮箱前缀"),
            param("role-code", false, "绑定的角色编码，默认 super_admin"),
            param("tenant-id", false, "所属租户 ID（整数）"),
        ],
    ),
    (
        "admin.unlock",
        &[param("email", true, "要解锁的管理员邮箱")],
    ),
    ("admin.sync_fixture_baseline", &[]),
    ("operations.run_due_scheduled_tasks", &[]),
];

#[must_use]
pub fn task_params(name: &str) -> Option<&'static [TaskParam]> {
    TASK_PARAMS
        .iter()
        .find(|(task, _)| *task == name)
        .map(|(_, params)| *params)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, path::Path};

    use loco_rs::{app::Hooks, task::Tasks};
    use regex::Regex;

    use super::*;
    use crate::app::App;

    #[test]
    fn every_registered_task_is_listed() {
        let mut tasks = Tasks::default();
        App::register_tasks(&mut tasks);
        let registered: BTreeSet<String> = tasks.names().into_iter().collect();
        let listed: BTreeSet<String> = TASK_PARAMS
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect();
        assert_eq!(
            registered, listed,
            "update tasks::TASK_PARAMS to match App::register_tasks"
        );
    }

    #[test]
    fn every_task_parameter_is_documented() {
        // Any string literal handed to an argument reader counts as a task parameter.
        let reader = Regex::new(
            r#"(?:cli_arg|cli\s*\.\s*get|required_arg\(\s*vars\s*,|optional_i32_arg\(\s*vars\s*,)\s*\(?\s*"([^"]+)""#,
        )
        .unwrap();
        let documented: BTreeSet<&str> = TASK_PARAMS
            .iter()
            .flat_map(|(_, params)| params.iter().map(|param| param.name))
            .collect();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tasks");
        let mut found = 0;
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let source = fs::read_to_string(&path).unwrap();
            for capture in reader.captures_iter(&source) {
                let key = &capture[1];
                found += 1;
                assert!(
                    documented.contains(key),
                    "{} reads task parameter `{key}`; document it in tasks::TASK_PARAMS",
                    path.display()
                );
            }
        }
        assert!(
            found >= 6,
            "the parameter pattern no longer matches the task sources"
        );
    }
}
