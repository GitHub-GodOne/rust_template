//! IP / CIDR allow lists such as `security.admin_ip_allowlist`.

use std::net::IpAddr;

use super::client_info::normalize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IpRule {
    network: IpAddr,
    prefix: u8,
}

impl IpRule {
    /// Parses `203.0.113.8`, `192.168.1.0/24` or `2001:db8::/32`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let (address, prefix) = value
            .split_once('/')
            .map_or((value, None), |(address, prefix)| (address, Some(prefix)));
        let network = normalize(address.trim().parse::<IpAddr>().ok()?);
        let max = if network.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(prefix) => prefix.trim().parse::<u8>().ok().filter(|p| *p <= max)?,
            None => max,
        };
        Some(Self { network, prefix })
    }

    #[must_use]
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.network, normalize(ip)) {
            (IpAddr::V4(network), IpAddr::V4(ip)) => prefix_matches(
                u128::from(u32::from(network)) << 96,
                u128::from(u32::from(ip)) << 96,
                self.prefix,
            ),
            (IpAddr::V6(network), IpAddr::V6(ip)) => {
                prefix_matches(u128::from(network), u128::from(ip), self.prefix)
            }
            _ => false,
        }
    }
}

fn prefix_matches(network: u128, ip: u128, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask = u128::MAX << (128 - u32::from(prefix));
    network & mask == ip & mask
}

/// Splits a comma / whitespace separated list; returns the entries that
/// could not be parsed alongside the rules.
#[must_use]
pub fn parse_rules(value: &str) -> (Vec<IpRule>, Vec<String>) {
    let mut rules = Vec::new();
    let mut invalid = Vec::new();
    for entry in value
        .split(|character: char| character == ',' || character == ';' || character.is_whitespace())
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        match IpRule::parse(entry) {
            Some(rule) => rules.push(rule),
            None => invalid.push(entry.to_string()),
        }
    }
    (rules, invalid)
}

#[cfg(test)]
mod tests {
    use super::{parse_rules, IpRule};

    #[test]
    fn single_addresses_and_networks_match() {
        let ip = |value: &str| value.parse().unwrap();
        let rule = IpRule::parse("192.168.1.0/24").unwrap();
        assert!(rule.contains(ip("192.168.1.200")));
        assert!(!rule.contains(ip("192.168.2.1")));
        assert!(rule.contains(ip("::ffff:192.168.1.9")));
        assert!(IpRule::parse("203.0.113.8")
            .unwrap()
            .contains(ip("203.0.113.8")));
        assert!(!IpRule::parse("203.0.113.8")
            .unwrap()
            .contains(ip("203.0.113.9")));
        let v6 = IpRule::parse("2001:db8::/32").unwrap();
        assert!(v6.contains(ip("2001:db8:1::5")));
        assert!(!v6.contains(ip("2001:db9::1")));
        assert!(IpRule::parse("0.0.0.0/0").unwrap().contains(ip("8.8.8.8")));
    }

    #[test]
    fn lists_report_invalid_entries() {
        let (rules, invalid) = parse_rules("10.0.0.0/8, 203.0.113.8\n bad, 1.2.3.4/33");
        assert_eq!(rules.len(), 2);
        assert_eq!(invalid, vec!["bad".to_string(), "1.2.3.4/33".to_string()]);
        assert!(parse_rules("  ").0.is_empty());
    }
}
