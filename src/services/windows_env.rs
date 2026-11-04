//! Up-to-date `PATH` for shells the server starts on Windows.
//!
//! The server usually runs as a scheduled task (`service install`), which
//! hands it the environment from when the task started, and as the SYSTEM
//! account it never sees the signed-in user's `PATH` at all. Shells opened
//! from the admin UI rebuild `PATH` from the registry instead, so tools added
//! to the system or user environment later (for example `claude`) are found.

use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsString,
    ptr,
    sync::OnceLock,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_MORE_DATA, ERROR_SUCCESS, HANDLE},
    Security::{
        GetTokenInformation, IsWellKnownSid, TokenUser, WinLocalSystemSid, TOKEN_QUERY, TOKEN_USER,
    },
    System::{
        Registry::{
            RegCloseKey, RegEnumKeyExW, RegEnumValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
            HKEY_LOCAL_MACHINE, HKEY_USERS, KEY_READ, REG_EXPAND_SZ, REG_SZ,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

const MACHINE_ENVIRONMENT: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";
const MAX_VALUE_BYTES: usize = 1 << 20;

/// Machine `PATH`, then the current account's, then (when running as SYSTEM)
/// every signed-in user's, followed by entries only this process had.
pub fn refreshed_path() -> OsString {
    let machine = RegKey::open(HKEY_LOCAL_MACHINE, MACHINE_ENVIRONMENT)
        .map(|key| key.values())
        .unwrap_or_default();
    let mut groups = vec![path_from(&machine, &[])];

    let mut users = vec![(HKEY_CURRENT_USER, String::new())];
    if is_local_system() {
        users.extend(
            signed_in_users()
                .into_iter()
                .map(|sid| (HKEY_USERS, format!(r"{sid}\"))),
        );
    }
    for (root, prefix) in users {
        let read = |name: &str| {
            RegKey::open(root, &format!("{prefix}{name}"))
                .map(|key| key.values())
                .unwrap_or_default()
        };
        let user = read("Environment");
        // USERPROFILE, APPDATA, LOCALAPPDATA… of that user, for %VAR% references.
        let volatile = read("Volatile Environment");
        groups.push(path_from(&user, &[&volatile, &machine]));
    }

    groups.push(env::var("PATH").unwrap_or_default());
    OsString::from(merge_paths(groups.iter().map(String::as_str)))
}

/// `PATH` of one registry environment, with `%VAR%` resolved from it, then the
/// fallback environments, then the process.
fn path_from(values: &HashMap<String, String>, fallbacks: &[&HashMap<String, String>]) -> String {
    let Some(raw) = values.get("PATH") else {
        return String::new();
    };
    expand(raw, |name| {
        let name = name.to_uppercase();
        std::iter::once(values)
            .chain(fallbacks.iter().copied())
            .find_map(|layer| layer.get(&name).cloned())
            .or_else(|| env::var(&name).ok())
    })
}

/// Replaces `%NAME%` with its value; unknown names stay as written.
fn expand(value: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut expanded = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        expanded.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                if let Some(resolved) = lookup(&after[..end]) {
                    expanded.push_str(&resolved);
                    rest = &after[end + 1..];
                    continue;
                }
                expanded.push('%');
                rest = after;
            }
            _ => {
                expanded.push('%');
                rest = after;
            }
        }
    }
    expanded.push_str(rest);
    expanded
}

/// Joins `PATH` lists, dropping blanks, stray quotes and repeated directories.
fn merge_paths<'a>(groups: impl IntoIterator<Item = &'a str>) -> String {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for entry in groups.into_iter().flat_map(|group| group.split(';')) {
        let entry = entry.trim().trim_matches('"').trim();
        if entry.is_empty() {
            continue;
        }
        if seen.insert(entry.trim_end_matches(['\\', '/']).to_lowercase()) {
            entries.push(entry);
        }
    }
    entries.join(";")
}

fn is_local_system() -> bool {
    static IS_SYSTEM: OnceLock<bool> = OnceLock::new();
    *IS_SYSTEM.get_or_init(|| {
        // SAFETY: the token handle is closed before returning and the buffer is
        // large and aligned enough for a TOKEN_USER with its SID.
        unsafe {
            let mut token: HANDLE = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) == 0 {
                return false;
            }
            let mut buffer = [0u64; 64];
            let mut length = 0u32;
            let queried = GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                len_u32(size_of_val(&buffer)),
                &raw mut length,
            ) != 0;
            CloseHandle(token);
            queried
                && IsWellKnownSid(
                    (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid,
                    WinLocalSystemSid,
                ) != 0
        }
    })
}

/// SIDs of users whose profile is loaded, i.e. who are signed in.
fn signed_in_users() -> Vec<String> {
    RegKey::open(HKEY_USERS, "")
        .map(|key| key.subkeys())
        .unwrap_or_default()
        .into_iter()
        .filter(|sid| {
            (sid.starts_with("S-1-5-21-") || sid.starts_with("S-1-12-1-"))
                && !sid.ends_with("_Classes")
        })
        .collect()
}

struct RegKey(HKEY);

impl RegKey {
    fn open(root: HKEY, subkey: &str) -> Option<Self> {
        let subkey = wide(subkey);
        let mut key: HKEY = ptr::null_mut();
        // SAFETY: `subkey` is NUL-terminated and outlives the call.
        let status = unsafe { RegOpenKeyExW(root, subkey.as_ptr(), 0, KEY_READ, &raw mut key) };
        (status == ERROR_SUCCESS).then_some(Self(key))
    }

    /// String values keyed by upper-cased name, `%VAR%` left unexpanded.
    fn values(&self) -> HashMap<String, String> {
        let mut values = HashMap::new();
        let mut name = vec![0u16; 16_384];
        let mut data = vec![0u16; 4_096];
        let mut index = 0;
        loop {
            let mut name_len = len_u32(name.len());
            let mut data_bytes = len_u32(data.len() * 2);
            let mut kind = 0u32;
            // SAFETY: the lengths passed match the buffers' capacities.
            let status = unsafe {
                RegEnumValueW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &raw mut name_len,
                    ptr::null(),
                    &raw mut kind,
                    data.as_mut_ptr().cast(),
                    &raw mut data_bytes,
                )
            };
            if status == ERROR_MORE_DATA && (data_bytes as usize) <= MAX_VALUE_BYTES {
                data.resize((data_bytes as usize).div_ceil(2) + 1, 0);
                continue;
            }
            index += 1;
            if status == ERROR_MORE_DATA {
                continue;
            }
            if status != ERROR_SUCCESS {
                break;
            }
            if kind == REG_SZ || kind == REG_EXPAND_SZ {
                let text = &data[..(data_bytes as usize / 2).min(data.len())];
                let text = text.split(|unit| *unit == 0).next().unwrap_or_default();
                values.insert(
                    String::from_utf16_lossy(&name[..name_len as usize]).to_uppercase(),
                    String::from_utf16_lossy(text),
                );
            }
        }
        values
    }

    fn subkeys(&self) -> Vec<String> {
        let mut subkeys = Vec::new();
        let mut name = [0u16; 256];
        for index in 0.. {
            let mut name_len = len_u32(name.len());
            // SAFETY: `name_len` matches the buffer's capacity.
            let status = unsafe {
                RegEnumKeyExW(
                    self.0,
                    index,
                    name.as_mut_ptr(),
                    &raw mut name_len,
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            };
            if status != ERROR_SUCCESS {
                break;
            }
            subkeys.push(String::from_utf16_lossy(&name[..name_len as usize]));
        }
        subkeys
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the key was opened by `RegOpenKeyExW` and is closed once.
        unsafe { RegCloseKey(self.0) };
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn len_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{expand, merge_paths, refreshed_path};

    #[test]
    fn expands_known_variables_only() {
        let lookup = |name: &str| (name == "USERPROFILE").then(|| r"C:\Users\me".to_string());
        assert_eq!(
            expand(r"%USERPROFILE%\.local\bin;%MISSING%\bin;100%", lookup),
            r"C:\Users\me\.local\bin;%MISSING%\bin;100%"
        );
    }

    #[test]
    fn merges_without_blanks_quotes_or_duplicates() {
        assert_eq!(
            merge_paths([
                r"C:\Windows;C:\Tools\",
                r#""C:\install\nvm;C:\install\nodejs;";c:\tools;;"#,
            ]),
            r"C:\Windows;C:\Tools\;C:\install\nvm;C:\install\nodejs"
        );
    }

    #[test]
    fn keeps_system_directories() {
        let path = refreshed_path().to_string_lossy().to_lowercase();
        assert!(path.contains(r"\system32"));
    }
}
