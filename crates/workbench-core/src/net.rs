//! Addresses a phone can use to reach this machine's LAN server when pairing.

use std::net::{IpAddr, Ipv4Addr};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingAddress {
    pub interface: String,
    pub address: String,
    /// In Tailscale's CGNAT range (100.64.0.0/10): traffic is WireGuard-encrypted.
    pub tailscale: bool,
}

/// This machine's reachable IPv4 addresses, best pairing candidates first.
pub fn pairing_addresses() -> anyhow::Result<Vec<PairingAddress>> {
    let interfaces = if_addrs::get_if_addrs()?;
    Ok(rank_addresses(interfaces.into_iter().filter_map(
        |iface| match iface.ip() {
            IpAddr::V4(ip) => Some((iface.name, ip)),
            IpAddr::V6(_) => None,
        },
    )))
}

/// Drop loopback / link-local / unspecified addresses and order the rest:
/// Tailscale, then RFC 1918 private, then anything else (stable within a group).
pub fn rank_addresses(addrs: impl IntoIterator<Item = (String, Ipv4Addr)>) -> Vec<PairingAddress> {
    let mut ranked: Vec<(u8, PairingAddress)> = Vec::new();
    for (interface, ip) in addrs {
        if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
            continue;
        }
        let address = ip.to_string();
        if ranked.iter().any(|(_, a)| a.address == address) {
            continue;
        }
        let tailscale = is_tailscale(ip);
        let rank = if tailscale {
            0
        } else if ip.is_private() {
            1
        } else {
            2
        };
        ranked.push((
            rank,
            PairingAddress {
                interface,
                address,
                tailscale,
            },
        ));
    }
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.into_iter().map(|(_, a)| a).collect()
}

fn is_tailscale(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (b & 0b1100_0000) == 64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ifs(entries: &[(&str, &str)]) -> Vec<(String, Ipv4Addr)> {
        entries
            .iter()
            .map(|(name, ip)| (name.to_string(), ip.parse().unwrap()))
            .collect()
    }

    #[test]
    fn orders_tailscale_then_private_then_other() {
        let ranked = rank_addresses(ifs(&[
            ("en1", "203.0.113.7"),
            ("en0", "192.168.1.20"),
            ("utun4", "100.101.102.103"),
            ("bridge0", "10.0.0.2"),
        ]));
        let order: Vec<_> = ranked.iter().map(|a| a.address.as_str()).collect();
        assert_eq!(
            order,
            ["100.101.102.103", "192.168.1.20", "10.0.0.2", "203.0.113.7"]
        );
        assert!(ranked[0].tailscale);
        assert_eq!(ranked[0].interface, "utun4");
        assert!(ranked[1..].iter().all(|a| !a.tailscale));
    }

    #[test]
    fn skips_loopback_link_local_unspecified_and_duplicates() {
        let ranked = rank_addresses(ifs(&[
            ("lo0", "127.0.0.1"),
            ("en0", "169.254.10.1"),
            ("any", "0.0.0.0"),
            ("en0", "192.168.1.20"),
            ("en0-alias", "192.168.1.20"),
        ]));
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].address, "192.168.1.20");
    }

    #[test]
    fn tailscale_range_is_exactly_100_64_slash_10() {
        for (ip, expected) in [
            ("100.64.0.0", true),
            ("100.127.255.255", true),
            ("100.63.255.255", false),
            ("100.128.0.0", false),
        ] {
            assert_eq!(is_tailscale(ip.parse().unwrap()), expected, "{ip}");
        }
    }
}
