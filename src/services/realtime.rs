//! Server-sent events for signed-in users: new notifications appear without
//! polling, and a revoked session learns it was signed out right away.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

const CHANNEL_CAPACITY: usize = 256;
const TICKET_TTL: Duration = Duration::from_secs(60);

/// Who an event is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Audience {
    All,
    User(i32),
    Tenant(i32),
    /// One login session (forced sign-out).
    Session(i32),
    /// Every session of a user except one (`None`: all of them).
    UserSessions {
        user_id: i32,
        except: Option<i32>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct RealtimeEvent {
    /// `notification` or `session_revoked`.
    pub kind: &'static str,
    pub data: serde_json::Value,
    #[serde(skip)]
    pub audience: Audience,
}

/// The listener a stream belongs to.
#[derive(Debug, Clone, Copy)]
pub struct Listener {
    pub user_id: i32,
    pub tenant_id: Option<i32>,
    pub session_id: i32,
}

impl RealtimeEvent {
    #[must_use]
    pub fn is_for(&self, listener: &Listener) -> bool {
        match &self.audience {
            Audience::All => true,
            Audience::User(user_id) => *user_id == listener.user_id,
            Audience::Tenant(tenant_id) => listener.tenant_id == Some(*tenant_id),
            Audience::Session(session_id) => *session_id == listener.session_id,
            Audience::UserSessions { user_id, except } => {
                *user_id == listener.user_id && *except != Some(listener.session_id)
            }
        }
    }
}

static CHANNEL: OnceLock<broadcast::Sender<RealtimeEvent>> = OnceLock::new();

fn channel() -> &'static broadcast::Sender<RealtimeEvent> {
    CHANNEL.get_or_init(|| broadcast::channel(CHANNEL_CAPACITY).0)
}

/// Sends an event to connected listeners; nobody listening is fine.
pub fn publish(kind: &'static str, audience: Audience, data: serde_json::Value) {
    let _ = channel().send(RealtimeEvent {
        kind,
        data,
        audience,
    });
}

#[must_use]
pub fn subscribe() -> broadcast::Receiver<RealtimeEvent> {
    channel().subscribe()
}

/// What a stream ticket stands for.
#[derive(Debug, Clone)]
pub struct StreamTicket {
    pub listener: Listener,
    pub sid: String,
    pub pid: String,
}

static TICKETS: OnceLock<Mutex<HashMap<String, (Instant, StreamTicket)>>> = OnceLock::new();

fn tickets() -> &'static Mutex<HashMap<String, (Instant, StreamTicket)>> {
    TICKETS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `EventSource` cannot send the bearer token, so the page trades it for a
/// short-lived, single-use ticket first.
#[must_use]
pub fn issue_ticket(ticket: StreamTicket) -> String {
    let id = Uuid::new_v4().simple().to_string();
    if let Ok(mut store) = tickets().lock() {
        store.retain(|_, (issued, _)| issued.elapsed() < TICKET_TTL);
        store.insert(id.clone(), (Instant::now(), ticket));
    }
    id
}

#[must_use]
pub fn redeem_ticket(id: &str) -> Option<StreamTicket> {
    let mut store = tickets().lock().ok()?;
    store
        .remove(id)
        .filter(|(issued, _)| issued.elapsed() < TICKET_TTL)
        .map(|(_, ticket)| ticket)
}

#[cfg(test)]
mod tests {
    use super::{issue_ticket, redeem_ticket, Audience, Listener, RealtimeEvent, StreamTicket};

    fn event(audience: Audience) -> RealtimeEvent {
        RealtimeEvent {
            kind: "notification",
            data: serde_json::Value::Null,
            audience,
        }
    }

    #[test]
    fn events_reach_only_their_audience() {
        let listener = Listener {
            user_id: 7,
            tenant_id: Some(2),
            session_id: 30,
        };
        assert!(event(Audience::All).is_for(&listener));
        assert!(event(Audience::User(7)).is_for(&listener));
        assert!(!event(Audience::User(8)).is_for(&listener));
        assert!(event(Audience::Tenant(2)).is_for(&listener));
        assert!(!event(Audience::Session(31)).is_for(&listener));
        assert!(!event(Audience::UserSessions {
            user_id: 7,
            except: Some(30)
        })
        .is_for(&listener));
        assert!(event(Audience::UserSessions {
            user_id: 7,
            except: None
        })
        .is_for(&listener));
    }

    #[test]
    fn tickets_are_single_use() {
        let id = issue_ticket(StreamTicket {
            listener: Listener {
                user_id: 1,
                tenant_id: None,
                session_id: 1,
            },
            sid: "s".into(),
            pid: "p".into(),
        });
        assert!(redeem_ticket(&id).is_some());
        assert!(redeem_ticket(&id).is_none());
    }
}
