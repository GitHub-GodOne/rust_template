//! English translations for Chinese API messages.
//!
//! Messages are written in Chinese throughout the backend. When a client
//! prefers English (`Accept-Language: en…`), `middleware::locale` rewrites the
//! `message` of error responses through [`translate`]. Keys in `en.json` are
//! the Chinese source text; `format!` messages are keys with positional
//! placeholders, e.g. `"{0} 的证书已存在"` → `"A certificate for {0} already exists"`.

use std::{collections::HashMap, sync::OnceLock};

use regex::Regex;

const DICTIONARY: &str = include_str!("en.json");
const MAX_DEPTH: usize = 3;

struct Template {
    pattern: Regex,
    /// Placeholder index for each capture group, in order of appearance.
    slots: Vec<usize>,
    value: String,
    literal_len: usize,
}

struct Dictionary {
    exact: HashMap<String, String>,
    templates: Vec<Template>,
}

fn placeholder() -> &'static Regex {
    static PLACEHOLDER: OnceLock<Regex> = OnceLock::new();
    PLACEHOLDER.get_or_init(|| Regex::new(r"\{(\d+)\}").expect("valid placeholder pattern"))
}

fn dictionary() -> &'static Dictionary {
    static DICTIONARY_CELL: OnceLock<Dictionary> = OnceLock::new();
    DICTIONARY_CELL.get_or_init(|| {
        let entries: HashMap<String, String> =
            serde_json::from_str(DICTIONARY).expect("src/i18n/en.json must be valid JSON");
        let mut templates = Vec::new();
        for (key, value) in &entries {
            if !placeholder().is_match(key) {
                continue;
            }
            let mut pattern = String::from("(?s)^");
            let mut slots = Vec::new();
            let mut last = 0;
            let mut literal_len = 0;
            for capture in placeholder().captures_iter(key) {
                let whole = capture.get(0).expect("match");
                let literal = &key[last..whole.start()];
                literal_len += literal.chars().count();
                pattern.push_str(&regex::escape(literal));
                pattern.push_str("(.+?)");
                slots.push(capture[1].parse().unwrap_or(0));
                last = whole.end();
            }
            let tail = &key[last..];
            literal_len += tail.chars().count();
            pattern.push_str(&regex::escape(tail));
            pattern.push('$');
            if let Ok(pattern) = Regex::new(&pattern) {
                templates.push(Template {
                    pattern,
                    slots,
                    value: value.clone(),
                    literal_len,
                });
            }
        }
        // Prefer the most specific template when several match.
        templates.sort_by_key(|template| std::cmp::Reverse(template.literal_len));
        Dictionary {
            exact: entries,
            templates,
        }
    })
}

fn has_han(text: &str) -> bool {
    text.chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch))
}

/// Translates a Chinese message to English; unknown text is returned as is.
#[must_use]
pub fn translate(message: &str) -> String {
    translate_at(message, 0)
}

fn translate_at(message: &str, depth: usize) -> String {
    if !has_han(message) || depth > MAX_DEPTH {
        return message.to_string();
    }
    let dictionary = dictionary();
    if let Some(value) = dictionary.exact.get(message) {
        return value.clone();
    }
    for template in &dictionary.templates {
        if let Some(captures) = template.pattern.captures(message) {
            let mut arguments: HashMap<usize, String> = HashMap::new();
            for (group, slot) in template.slots.iter().enumerate() {
                if let Some(value) = captures.get(group + 1) {
                    // Arguments are often messages themselves (e.g. "DNS：{0}").
                    arguments.insert(*slot, translate_at(value.as_str(), depth + 1));
                }
            }
            return placeholder()
                .replace_all(&template.value, |capture: &regex::Captures<'_>| {
                    capture[1]
                        .parse::<usize>()
                        .ok()
                        .and_then(|slot| arguments.get(&slot).cloned())
                        .unwrap_or_default()
                })
                .into_owned();
        }
    }
    // Joined problem lists, e.g. "A；B".
    if message.contains('；') {
        return message
            .split('；')
            .map(|part| translate_at(part, depth + 1))
            .collect::<Vec<_>>()
            .join("; ");
    }
    message.to_string()
}

/// Whether an `Accept-Language` header asks for English first.
#[must_use]
pub fn prefers_english(accept_language: Option<&str>) -> bool {
    accept_language
        .and_then(|value| value.split(',').next())
        .is_some_and(|first| first.trim().to_ascii_lowercase().starts_with("en"))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fmt::Write as _, fs, path::Path};

    use super::*;

    /// Files whose Chinese text is handled elsewhere (frontend `tData`).
    const SKIPPED_FILES: [&str; 2] = ["services/audit.rs", "controllers/admin/dashboard.rs"];

    fn rust_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !path.ends_with("fixtures") && !path.ends_with("i18n") {
                    rust_files(&path, files);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }

    /// End of a raw string literal (`r"…"`, `r#"…"#`) starting at `index`.
    fn raw_string_end(chars: &[char], index: usize) -> Option<usize> {
        if chars[index] != 'r' || !matches!(chars.get(index + 1), Some('#' | '"')) {
            return None;
        }
        let mut cursor = index + 1;
        let mut hashes = 0;
        while chars.get(cursor) == Some(&'#') {
            hashes += 1;
            cursor += 1;
        }
        if chars.get(cursor) != Some(&'"') {
            return None;
        }
        cursor += 1;
        while cursor < chars.len() {
            if chars[cursor] == '"'
                && (0..hashes).all(|offset| chars.get(cursor + 1 + offset) == Some(&'#'))
            {
                return Some(cursor + 1 + hashes);
            }
            cursor += 1;
        }
        Some(cursor)
    }

    /// Skips a char literal ('x', '\'', '\u{..}') or a lifetime ('a).
    fn char_literal_end(chars: &[char], index: usize) -> usize {
        if chars.get(index + 1) == Some(&'\\') {
            let mut cursor = index + 2;
            while cursor < chars.len() && chars[cursor] != '\'' {
                cursor += 1;
            }
            return cursor + 1;
        }
        if chars.get(index + 2) == Some(&'\'') {
            return index + 3;
        }
        index + 1
    }

    /// Bodies of the ordinary string literals in production code. Comments,
    /// char literals and raw strings (HTML templates) are skipped, and so are
    /// `#[cfg(test)] mod … { … }` blocks.
    fn production_literals(source: &str) -> Vec<String> {
        let chars: Vec<char> = source.chars().collect();
        let mut literals = Vec::new();
        let mut index = 0;
        // Brace depth at which a test module started, while inside one.
        let mut test_depth: Option<usize> = None;
        let mut depth = 0usize;
        let mut pending_test = false;
        while index < chars.len() {
            let ch = chars[index];
            let next = chars.get(index + 1).copied();
            if ch == '/' && next == Some('/') {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
                continue;
            }
            if ch == '/' && next == Some('*') {
                index += 2;
                while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                    index += 1;
                }
                index += 2;
                continue;
            }
            if let Some(end) = raw_string_end(&chars, index) {
                index = end;
                continue;
            }
            if ch == '\'' {
                index = char_literal_end(&chars, index);
                continue;
            }
            if ch == '"' {
                let mut cursor = index + 1;
                let mut body = String::new();
                while cursor < chars.len() && chars[cursor] != '"' {
                    if chars[cursor] == '\\' && cursor + 1 < chars.len() {
                        body.push(chars[cursor]);
                        cursor += 1;
                    }
                    body.push(chars[cursor]);
                    cursor += 1;
                }
                if test_depth.is_none() {
                    literals.push(body);
                }
                index = cursor + 1;
                continue;
            }
            if ch == '#' && chars[index..].iter().take(12).collect::<String>() == "#[cfg(test)]" {
                pending_test = true;
                index += 12;
                continue;
            }
            if ch == '{' {
                depth += 1;
                if pending_test && test_depth.is_none() {
                    let before: String = chars[index.saturating_sub(80)..index].iter().collect();
                    if before.contains("mod ") {
                        test_depth = Some(depth);
                    }
                }
                pending_test = false;
            } else if ch == '}' {
                if test_depth == Some(depth) {
                    test_depth = None;
                }
                depth = depth.saturating_sub(1);
            } else if ch == ';' {
                pending_test = false;
            }
            index += 1;
        }
        literals
    }

    /// Runtime text of a Rust string literal body, with `format!` arguments
    /// turned into positional placeholders.
    fn message_key(body: &str) -> String {
        let continuation = Regex::new(r"\\\n\s*").unwrap();
        let body = continuation.replace_all(body, "");
        let mut text = String::new();
        let mut chars = body.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('n') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some(other) => text.push(other),
                    None => {}
                }
            } else {
                text.push(ch);
            }
        }
        let mut key = String::new();
        let mut index = 0;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    key.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    key.push('}');
                }
                '{' => {
                    for next in chars.by_ref() {
                        if next == '}' {
                            break;
                        }
                    }
                    let _ = write!(key, "{{{index}}}");
                    index += 1;
                }
                other => key.push(other),
            }
        }
        key
    }

    fn source_keys() -> BTreeSet<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&root, &mut files);
        let mut keys = BTreeSet::new();
        for file in files {
            let relative = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if SKIPPED_FILES.contains(&relative.as_str()) {
                continue;
            }
            for body in production_literals(&fs::read_to_string(&file).unwrap()) {
                let key = message_key(&body);
                if has_han(&key) {
                    keys.insert(key);
                }
            }
        }
        keys
    }

    fn entries() -> HashMap<String, String> {
        serde_json::from_str(DICTIONARY).unwrap()
    }

    #[test]
    fn every_backend_message_has_an_english_translation() {
        if std::env::var_os("I18N_DUMP").is_some() {
            println!("{}", serde_json::to_string_pretty(&source_keys()).unwrap());
        }
        let dictionary = entries();
        let missing: Vec<String> = source_keys()
            .into_iter()
            .filter(|key| !dictionary.contains_key(key))
            .collect();
        assert!(missing.is_empty(), "add to src/i18n/en.json: {missing:#?}");
    }

    #[test]
    fn dictionary_has_no_stale_keys_and_keeps_placeholders() {
        let keys = source_keys();
        let placeholders = |text: &str| -> BTreeSet<String> {
            placeholder()
                .find_iter(text)
                .map(|found| found.as_str().to_string())
                .collect()
        };
        for (key, value) in entries() {
            assert!(keys.contains(&key), "stale key in src/i18n/en.json: {key}");
            assert_eq!(
                placeholders(&key),
                placeholders(&value),
                "placeholders differ: {key}"
            );
            assert!(
                !has_han(&value),
                "translation still contains Chinese: {key}"
            );
        }
    }

    // The inputs are Rust source text, so they look like format strings on purpose.
    #[allow(clippy::literal_string_with_formatting_args)]
    #[test]
    fn message_keys_follow_format_rules() {
        assert_eq!(message_key("{domain} 的证书已存在"), "{0} 的证书已存在");
        assert_eq!(message_key("{} / {:?} 与 {{原样}}"), "{0} / {1} 与 {原样}");
        assert_eq!(message_key("第一行\\n第二行"), "第一行\n第二行");
    }

    #[test]
    fn templates_and_joined_messages_translate_recursively() {
        if entries().is_empty() {
            return;
        }
        assert!(prefers_english(Some("en-US,zh;q=0.8")));
        assert!(!prefers_english(Some("zh-CN,en;q=0.5")));
        assert!(!prefers_english(None));
        assert_eq!(translate("plain English"), "plain English");
        assert_eq!(translate("未知的中文"), "未知的中文");
        assert_eq!(translate("证书不存在"), "Certificate not found");
        assert_eq!(
            translate("example.test 的证书已存在"),
            "A certificate for example.test already exists"
        );
        assert_eq!(
            translate("账号或密码错误，还可尝试 2 次"),
            "Wrong account or password; 2 attempts left"
        );
        // Template arguments are translated too.
        assert_eq!(
            translate("启动 cloudflared 失败：未知错误"),
            "Failed to start cloudflared: Unknown error"
        );
        assert_eq!(
            translate("证书不存在；站点名称不能为空"),
            "Certificate not found; The site name cannot be empty"
        );
    }
}
