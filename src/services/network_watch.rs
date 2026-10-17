//! Logs the host's network state whenever it changes.
//!
//! A period where the site was unreachable can then be matched against what
//! the machine was doing: who is logged in at the console, the default route,
//! interface addresses, DNS servers and connected VPNs.
//!
//! Only macOS is inspected for now; other platforms log nothing.

use std::time::Duration;

#[cfg(target_os = "macos")]
use tokio::process::Command;

const INTERVAL: Duration = Duration::from_secs(30);
/// A healthy-link line every this many checks (10 × 30 s = 5 min).
const SUMMARY_EVERY: u32 = 10;
/// Round trips above this stall page loads noticeably.
const SLOW_PING_MS: f64 = 300.0;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkSnapshot {
    /// `root` means nobody is logged in (login window).
    pub console_user: String,
    pub default_interface: String,
    pub default_gateway: String,
    /// `interface=address` for every active interface with an IPv4 address.
    pub addresses: Vec<String>,
    pub dns_servers: Vec<String>,
    pub connected_vpns: Vec<String>,
}

impl NetworkSnapshot {
    /// Field-by-field description of what changed.
    #[must_use]
    pub fn diff(&self, previous: &Self) -> Vec<String> {
        let mut changes = Vec::new();
        let mut field = |name: &str, old: String, new: String| {
            if old != new {
                changes.push(format!("{name}: {old} -> {new}"));
            }
        };
        field(
            "console_user",
            previous.console_user.clone(),
            self.console_user.clone(),
        );
        field(
            "default_route",
            format!(
                "{} via {}",
                previous.default_interface, previous.default_gateway
            ),
            format!("{} via {}", self.default_interface, self.default_gateway),
        );
        field(
            "addresses",
            previous.addresses.join(","),
            self.addresses.join(","),
        );
        field(
            "dns",
            previous.dns_servers.join(","),
            self.dns_servers.join(","),
        );
        field(
            "vpn",
            previous.connected_vpns.join(","),
            self.connected_vpns.join(","),
        );
        changes
    }
}

/// `route -n get default` → (interface, gateway).
#[must_use]
pub fn parse_default_route(output: &str) -> (String, String) {
    let value = |key: &str| {
        output
            .lines()
            .find_map(|line| line.trim().strip_prefix(key))
            .map(|rest| rest.trim().to_string())
            .unwrap_or_default()
    };
    (value("interface:"), value("gateway:"))
}

/// `ifconfig` → `en0=172.20.10.8` for active interfaces with IPv4.
#[must_use]
pub fn parse_ifconfig(output: &str) -> Vec<String> {
    let mut addresses = Vec::new();
    let mut interface = String::new();
    let mut pending: Vec<String> = Vec::new();
    let mut active = true;
    let mut flush = |interface: &str, pending: &mut Vec<String>, active: bool| {
        if active && interface != "lo0" {
            for address in pending.iter() {
                addresses.push(format!("{interface}={address}"));
            }
        }
        pending.clear();
    };
    for line in output.lines() {
        if !line.starts_with(['\t', ' ']) && line.contains(": flags=") {
            flush(&interface, &mut pending, active);
            interface = line.split(':').next().unwrap_or_default().to_string();
            active = true;
        } else if let Some(rest) = line.trim().strip_prefix("inet ") {
            if let Some(address) = rest.split_whitespace().next() {
                pending.push(address.to_string());
            }
        } else if let Some(status) = line.trim().strip_prefix("status: ") {
            active = status == "active";
        }
    }
    flush(&interface, &mut pending, active);
    addresses
}

/// `scutil --dns` → unique nameservers in order.
#[must_use]
pub fn parse_dns(output: &str) -> Vec<String> {
    let mut servers: Vec<String> = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("nameserver[") {
            if let Some((_, server)) = line.split_once(':') {
                let server = server.trim().to_string();
                if !servers.contains(&server) {
                    servers.push(server);
                }
            }
        }
    }
    servers
}

/// `scutil --nc list` → names of connected VPN services.
#[must_use]
pub fn parse_connected_vpns(output: &str) -> Vec<String> {
    output
        .lines()
        .filter(|line| line.contains("(Connected)"))
        .filter_map(|line| line.split('"').nth(1))
        .map(ToString::to_string)
        .collect()
}

/// Packet loss and round-trip times from `ping -q`.
#[derive(Debug, Clone, PartialEq)]
pub struct PingResult {
    pub loss_pct: f64,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

impl PingResult {
    /// Loss, or spikes long enough to stall a page load.
    #[must_use]
    pub fn degraded(&self) -> bool {
        self.loss_pct > 0.0 || self.max_ms.is_some_and(|max| max > SLOW_PING_MS)
    }
}

/// Parses the summary of `ping -q` on macOS/BSD.
#[must_use]
pub fn parse_ping(output: &str) -> Option<PingResult> {
    let loss_pct = output.lines().find_map(|line| {
        let before = line
            .split("% packet loss")
            .next()
            .filter(|_| line.contains("% packet loss"))?;
        before.rsplit([' ', ',']).next()?.parse::<f64>().ok()
    })?;
    let times: Vec<f64> = output
        .lines()
        .find(|line| line.contains("round-trip") || line.contains("rtt "))
        .and_then(|line| line.split('=').nth(1))
        .map(|values| {
            values
                .trim()
                .trim_end_matches("ms")
                .split('/')
                .filter_map(|value| value.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default();
    Some(PingResult {
        loss_pct,
        avg_ms: times.get(1).copied(),
        max_ms: times.get(2).copied(),
    })
}

#[cfg(target_os = "macos")]
async fn run(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .output()
        .await
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
async fn snapshot() -> NetworkSnapshot {
    let (default_interface, default_gateway) =
        parse_default_route(&run("/sbin/route", &["-n", "get", "default"]).await);
    NetworkSnapshot {
        console_user: run("/usr/bin/stat", &["-f", "%Su", "/dev/console"])
            .await
            .trim()
            .to_string(),
        default_interface,
        default_gateway,
        addresses: parse_ifconfig(&run("/sbin/ifconfig", &[]).await),
        dns_servers: parse_dns(&run("/usr/sbin/scutil", &["--dns"]).await),
        connected_vpns: parse_connected_vpns(&run("/usr/sbin/scutil", &["--nc", "list"]).await),
    }
}

/// Pings the LAN router (not the VPN) a few times.
#[cfg(target_os = "macos")]
async fn probe_router() -> Option<RouterProbe> {
    let router = run("/usr/sbin/ipconfig", &["getoption", "en0", "router"])
        .await
        .trim()
        .to_string();
    if router.is_empty() {
        return None;
    }
    let small = run(
        "/sbin/ping",
        &["-c", "5", "-i", "0.2", "-t", "4", "-q", &router],
    )
    .await;
    // Full-size frames with "don't fragment": a link that passes small
    // packets but drops these lets /_ping work while pages time out.
    let full = run(
        "/sbin/ping",
        &[
            "-c", "5", "-i", "0.2", "-t", "4", "-q", "-D", "-s", "1472", &router,
        ],
    )
    .await;
    Some(RouterProbe {
        small: parse_ping(&small)?,
        full: parse_ping(&full)?,
        router,
    })
}

#[cfg(target_os = "macos")]
struct RouterProbe {
    router: String,
    small: PingResult,
    full: PingResult,
}

/// Starts the background watcher (macOS only).
pub fn start() {
    #[cfg(target_os = "macos")]
    tokio::spawn(async {
        let mut previous = snapshot().await;
        tracing::info!(
            console_user = previous.console_user,
            default_route = format!(
                "{} via {}",
                previous.default_interface, previous.default_gateway
            ),
            addresses = previous.addresses.join(","),
            dns = previous.dns_servers.join(","),
            vpn = previous.connected_vpns.join(","),
            "network watch started"
        );
        let mut ticks: u32 = 0;
        loop {
            tokio::time::sleep(INTERVAL).await;
            ticks = ticks.wrapping_add(1);
            let current = snapshot().await;
            let changes = current.diff(&previous);
            if !changes.is_empty() {
                tracing::warn!(changes = changes.join("; "), "network state changed");
                previous = current;
            }
            // Wi-Fi link quality to the router, e.g. a phone hotspot.
            if let Some(probe) = probe_router().await {
                let degraded = probe.small.degraded() || probe.full.degraded();
                if degraded || ticks.is_multiple_of(SUMMARY_EVERY) {
                    let message = if degraded {
                        "link to router degraded"
                    } else {
                        "link to router ok"
                    };
                    let fields = (
                        probe.small.loss_pct,
                        probe.small.max_ms.unwrap_or(-1.0),
                        probe.full.loss_pct,
                        probe.full.max_ms.unwrap_or(-1.0),
                    );
                    if degraded {
                        tracing::warn!(
                            router = probe.router,
                            console_user = previous.console_user,
                            small_loss_pct = fields.0,
                            small_max_ms = fields.1,
                            full_size_loss_pct = fields.2,
                            full_size_max_ms = fields.3,
                            "{message}"
                        );
                    } else {
                        tracing::info!(
                            router = probe.router,
                            console_user = previous.console_user,
                            small_loss_pct = fields.0,
                            small_max_ms = fields.1,
                            full_size_loss_pct = fields.2,
                            full_size_max_ms = fields.3,
                            "{message}"
                        );
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_macos_command_output() {
        let route = "   route to: default\ndestination: default\n    gateway: 172.20.10.1\n  interface: en0\n";
        assert_eq!(
            parse_default_route(route),
            ("en0".to_string(), "172.20.10.1".to_string())
        );

        let ifconfig = "lo0: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384\n\tinet 127.0.0.1 netmask 0xff000000\nen0: flags=8863<UP,BROADCAST> mtu 1500\n\tinet6 fe80::1%en0 prefixlen 64\n\tinet 172.20.10.8 netmask 0xfffffff0 broadcast 172.20.10.15\n\tstatus: active\nen1: flags=8863<UP> mtu 1500\n\tinet 10.0.0.5 netmask 0xffffff00\n\tstatus: inactive\nutun4: flags=8051<UP,POINTOPOINT> mtu 4064\n\tinet 198.18.0.1 --> 198.18.0.1 netmask 0xffffffff\n";
        assert_eq!(
            parse_ifconfig(ifconfig),
            ["en0=172.20.10.8", "utun4=198.18.0.1"]
        );

        let dns = "resolver #1\n  nameserver[0] : 198.19.0.3\n  if_index : 23 (utun4)\nresolver #2\n  nameserver[0] : 114.114.114.114\n  nameserver[1] : 198.19.0.3\n";
        assert_eq!(parse_dns(dns), ["198.19.0.3", "114.114.114.114"]);

        let vpns = "* (Disconnected)   1 VPN (a) \"Hiddify\"   [VPN:a]\n* (Connected)      2 VPN (b) \"Quantumult X\"   [VPN:b]\n";
        assert_eq!(parse_connected_vpns(vpns), ["Quantumult X"]);
    }

    /// `cargo test --lib network_watch -- --ignored --nocapture` prints this host's state.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    #[ignore = "reads the live network configuration"]
    async fn prints_the_live_snapshot() {
        let current = snapshot().await;
        println!("{current:#?}");
        assert!(!current.console_user.is_empty());
    }

    #[test]
    fn parses_ping_summaries() {
        let ok = "PING 172.20.10.1 (172.20.10.1): 56 data bytes\n\n--- 172.20.10.1 ping statistics ---\n5 packets transmitted, 5 packets received, 0.0% packet loss\nround-trip min/avg/max/stddev = 7.967/38.861/103.330/33.847 ms\n";
        let result = parse_ping(ok).unwrap();
        assert!((result.loss_pct - 0.0).abs() < f64::EPSILON);
        assert_eq!(result.avg_ms, Some(38.861));
        assert_eq!(result.max_ms, Some(103.33));
        assert!(!result.degraded());

        let lossy = "5 packets transmitted, 3 packets received, 40.0% packet loss\nround-trip min/avg/max/stddev = 5.0/900.5/2400.1/10.0 ms\n";
        let result = parse_ping(lossy).unwrap();
        assert!((result.loss_pct - 40.0).abs() < f64::EPSILON);
        assert!(result.degraded());

        let dead = "5 packets transmitted, 0 packets received, 100.0% packet loss\n";
        let result = parse_ping(dead).unwrap();
        assert_eq!(result.avg_ms, None);
        assert!(result.degraded());
        assert!(parse_ping("ping: cannot resolve").is_none());
    }

    #[test]
    fn describes_what_changed() {
        let before = NetworkSnapshot {
            console_user: "pikju".into(),
            default_interface: "utun4".into(),
            default_gateway: String::new(),
            addresses: vec!["en0=172.20.10.8".into()],
            dns_servers: vec!["198.19.0.3".into()],
            connected_vpns: vec!["Quantumult X".into()],
        };
        let after = NetworkSnapshot {
            console_user: "root".into(),
            default_interface: "en0".into(),
            default_gateway: "172.20.10.1".into(),
            connected_vpns: Vec::new(),
            ..before.clone()
        };
        assert_eq!(
            after.diff(&before),
            [
                "console_user: pikju -> root",
                "default_route: utun4 via  -> en0 via 172.20.10.1",
                "vpn: Quantumult X -> "
            ]
        );
        assert!(after.diff(&after).is_empty());
    }
}
