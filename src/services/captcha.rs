//! Login captcha: a short code drawn as a distorted SVG, kept in memory for a
//! few minutes and usable once. Also tracks recent login failures per client
//! IP for the "after failures" mode.

use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use uuid::Uuid;

const CODE_LENGTH: usize = 4;
/// No 0/O, 1/I/L: they are hard to tell apart when distorted.
const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
const TTL: Duration = Duration::from_secs(5 * 60);
const FAILURE_WINDOW: Duration = Duration::from_secs(60 * 60);
const MAX_ENTRIES: usize = 10_000;

struct Challenge {
    answer: String,
    created_at: Instant,
}

static CHALLENGES: OnceLock<Mutex<HashMap<String, Challenge>>> = OnceLock::new();
static FAILURES: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn challenges() -> &'static Mutex<HashMap<String, Challenge>> {
    CHALLENGES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn failures() -> &'static Mutex<HashMap<String, Instant>> {
    FAILURES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Random bytes from the OS-backed UUID generator.
struct Random {
    pool: Vec<u8>,
}

impl Random {
    const fn new() -> Self {
        Self { pool: Vec::new() }
    }

    fn byte(&mut self) -> u8 {
        if self.pool.is_empty() {
            self.pool.extend_from_slice(Uuid::new_v4().as_bytes());
        }
        self.pool.pop().unwrap_or(0)
    }

    /// Uniform-ish value in `min..=max` (small ranges only).
    fn range(&mut self, min: i32, max: i32) -> i32 {
        let span = u32::try_from(max - min + 1).unwrap_or(1).max(1);
        min + i32::try_from(u32::from(self.byte()) % span).unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
pub struct NewChallenge {
    pub id: String,
    pub svg: String,
}

/// Creates a challenge; the answer stays on the server.
#[must_use]
pub fn create() -> NewChallenge {
    let mut random = Random::new();
    let answer: String = (0..CODE_LENGTH)
        .map(|_| char::from(ALPHABET[usize::from(random.byte()) % ALPHABET.len()]))
        .collect();
    let id = Uuid::new_v4().simple().to_string();
    let svg = render(&answer, &mut random);
    if let Ok(mut store) = challenges().lock() {
        store.retain(|_, challenge| challenge.created_at.elapsed() < TTL);
        if store.len() >= MAX_ENTRIES {
            store.clear();
        }
        store.insert(
            id.clone(),
            Challenge {
                answer,
                created_at: Instant::now(),
            },
        );
    }
    NewChallenge { id, svg }
}

/// Checks and consumes a challenge (case-insensitive). Wrong answers also
/// consume it, so each image allows a single guess.
#[must_use]
pub fn verify(id: &str, code: &str) -> bool {
    let Ok(mut store) = challenges().lock() else {
        return false;
    };
    store.remove(id).is_some_and(|challenge| {
        challenge.created_at.elapsed() < TTL && challenge.answer.eq_ignore_ascii_case(code.trim())
    })
}

/// The expected answer, for integration tests that drive the login form.
#[doc(hidden)]
#[must_use]
pub fn answer_for_tests(id: &str) -> Option<String> {
    challenges()
        .lock()
        .ok()
        .and_then(|store| store.get(id).map(|challenge| challenge.answer.clone()))
}

pub fn record_failure(ip: &str) {
    if let Ok(mut store) = failures().lock() {
        store.retain(|_, at| at.elapsed() < FAILURE_WINDOW);
        if store.len() >= MAX_ENTRIES {
            store.clear();
        }
        store.insert(ip.to_string(), Instant::now());
    }
}

pub fn clear_failures(ip: &str) {
    if let Ok(mut store) = failures().lock() {
        store.remove(ip);
    }
}

#[must_use]
pub fn ip_failed_recently(ip: &str) -> bool {
    failures()
        .lock()
        .ok()
        .and_then(|store| store.get(ip).copied())
        .is_some_and(|at| at.elapsed() < FAILURE_WINDOW)
}

fn render(code: &str, random: &mut Random) -> String {
    const WIDTH: i32 = 120;
    const HEIGHT: i32 = 44;
    let mut svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}"><rect width="100%" height="100%" fill="#f4f1ec"/>"##
    );
    let colors = ["#3b4a6b", "#7a3e2e", "#2f5d50", "#5b3f75", "#7a6421"];
    for _ in 0..5 {
        let _ = write!(
            svg,
            r#"<path d="M{} {} Q{} {} {} {}" stroke="{}" stroke-width="1.2" fill="none" opacity="0.55"/>"#,
            random.range(0, 20),
            random.range(4, HEIGHT - 4),
            random.range(30, 90),
            random.range(0, HEIGHT),
            random.range(100, WIDTH),
            random.range(4, HEIGHT - 4),
            colors[usize::from(random.byte()) % colors.len()],
        );
    }
    for (index, character) in code.chars().enumerate() {
        let x = 14 + i32::try_from(index).unwrap_or(0) * 26 + random.range(-3, 3);
        let y = 31 + random.range(-4, 4);
        let _ = write!(
            svg,
            r#"<text x="{x}" y="{y}" font-family="Verdana,Arial,sans-serif" font-size="{}" font-weight="700" fill="{}" transform="rotate({} {x} {y})">{character}</text>"#,
            random.range(22, 28),
            colors[usize::from(random.byte()) % colors.len()],
            random.range(-28, 28),
        );
    }
    for _ in 0..24 {
        let _ = write!(
            svg,
            r##"<circle cx="{}" cy="{}" r="1" fill="#6b6b6b" opacity="0.5"/>"##,
            random.range(0, WIDTH),
            random.range(0, HEIGHT),
        );
    }
    svg.push_str("</svg>");
    svg
}

#[cfg(test)]
mod tests {
    use super::{challenges, create, verify};

    #[test]
    fn a_challenge_is_answered_once() {
        let challenge = create();
        assert!(challenge.svg.starts_with("<svg"));
        let answer = challenges()
            .lock()
            .unwrap()
            .get(&challenge.id)
            .unwrap()
            .answer
            .clone();
        assert!(verify(&challenge.id, &answer.to_lowercase()));
        assert!(!verify(&challenge.id, &answer), "already used");

        let other = create();
        assert!(!verify(&other.id, "wrong"));
        assert!(!challenges().lock().unwrap().contains_key(&other.id));
    }
}
