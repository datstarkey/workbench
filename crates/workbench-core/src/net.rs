//! Addresses a phone can use to reach this machine's LAN server when pairing.

use std::net::{IpAddr, Ipv4Addr};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingAddress {
    pub interface: String,
    pub address: String,
    /// Confirmed Tailscale address: traffic is WireGuard-encrypted.
    pub tailscale: bool,
}

const TAILSCALE_CLI_TIMEOUT: Duration = Duration::from_millis(1500);

/// This machine's reachable IPv4 addresses, best pairing candidates first.
pub fn pairing_addresses() -> anyhow::Result<Vec<PairingAddress>> {
    let addrs: Vec<(String, Ipv4Addr)> = if_addrs::get_if_addrs()?
        .into_iter()
        .filter_map(|iface| match iface.ip() {
            IpAddr::V4(ip) => Some((iface.name, ip)),
            IpAddr::V6(_) => None,
        })
        .collect();
    // Only ask the CLI when an interface name can't confirm a 100.64/10 address
    // (macOS names Tailscale's tunnel `utunN`, like every other VPN).
    let unconfirmed = addrs
        .iter()
        .any(|(name, ip)| in_cgnat_range(*ip) && !interface_is_tailscale(name));
    let cli_ips = if unconfirmed {
        tailscale_cli_ipv4()
    } else {
        Vec::new()
    };
    Ok(rank_addresses(addrs, &cli_ips))
}

/// Drop loopback / link-local / unspecified addresses and order the rest:
/// Tailscale, then RFC 1918 private, then anything else (stable within a group).
/// `tailscale_cli_ips` is what `tailscale ip -4` reported (empty if unavailable).
pub fn rank_addresses(
    addrs: impl IntoIterator<Item = (String, Ipv4Addr)>,
    tailscale_cli_ips: &[Ipv4Addr],
) -> Vec<PairingAddress> {
    let mut ranked: Vec<(u8, PairingAddress)> = Vec::new();
    for (interface, ip) in addrs {
        if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
            continue;
        }
        let address = ip.to_string();
        if ranked.iter().any(|(_, a)| a.address == address) {
            continue;
        }
        let tailscale = is_tailscale(&interface, ip, tailscale_cli_ips);
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

/// 100.64.0.0/10 is Tailscale's range but also carrier-grade NAT, so the range
/// alone proves nothing: require a Tailscale interface name or the CLI's word.
fn is_tailscale(interface: &str, ip: Ipv4Addr, cli_ips: &[Ipv4Addr]) -> bool {
    in_cgnat_range(ip) && (interface_is_tailscale(interface) || cli_ips.contains(&ip))
}

fn in_cgnat_range(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (b & 0b1100_0000) == 64
}

/// `tailscale0` on Linux, an adapter named "Tailscale" on Windows.
fn interface_is_tailscale(name: &str) -> bool {
    name.to_ascii_lowercase().contains("tailscale")
}

/// IPv4 addresses from `tailscale ip -4`, or empty when the CLI is missing,
/// fails, or doesn't answer within [`TAILSCALE_CLI_TIMEOUT`].
fn tailscale_cli_ipv4() -> Vec<Ipv4Addr> {
    let candidates = [
        "tailscale",
        #[cfg(target_os = "macos")]
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    ];
    candidates
        .into_iter()
        .find_map(|program| run_with_timeout(program, &["ip", "-4"], TAILSCALE_CLI_TIMEOUT))
        .map(|out| parse_ipv4_lines(&out))
        .unwrap_or_default()
}

fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = crate::shell::command(program)
        .args(args)
        .env("PATH", crate::paths::enriched_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(child.stdout.as_mut()?, &mut out).ok()?;
    Some(out)
}

fn parse_ipv4_lines(out: &str) -> Vec<Ipv4Addr> {
    out.lines().filter_map(|l| l.trim().parse().ok()).collect()
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

    fn order(ranked: &[PairingAddress]) -> Vec<&str> {
        ranked.iter().map(|a| a.address.as_str()).collect()
    }

    #[test]
    fn orders_tailscale_then_private_then_other() {
        let ranked = rank_addresses(
            ifs(&[
                ("en1", "203.0.113.7"),
                ("en0", "192.168.1.20"),
                ("tailscale0", "100.101.102.103"),
                ("bridge0", "10.0.0.2"),
            ]),
            &[],
        );
        assert_eq!(
            order(&ranked),
            ["100.101.102.103", "192.168.1.20", "10.0.0.2", "203.0.113.7"]
        );
        assert!(ranked[0].tailscale);
        assert!(ranked[1..].iter().all(|a| !a.tailscale));
    }

    #[test]
    fn interface_names_confirm_tailscale_on_linux_and_windows() {
        let ranked = rank_addresses(
            ifs(&[("tailscale0", "100.64.0.1"), ("Tailscale", "100.64.0.2")]),
            &[],
        );
        assert!(ranked.iter().all(|a| a.tailscale));
    }

    #[test]
    fn a_utun_address_is_tailscale_only_when_the_cli_reports_it() {
        let addrs = ifs(&[("en0", "192.168.1.20"), ("utun4", "100.101.102.103")]);
        let confirmed = rank_addresses(addrs.clone(), &["100.101.102.103".parse().unwrap()]);
        assert_eq!(order(&confirmed), ["100.101.102.103", "192.168.1.20"]);
        assert!(confirmed[0].tailscale);

        let unconfirmed = rank_addresses(addrs, &[]);
        assert!(unconfirmed.iter().all(|a| !a.tailscale));
    }

    #[test]
    fn carrier_grade_nat_is_not_tailscale_and_sorts_with_others() {
        let ranked = rank_addresses(
            ifs(&[
                ("wwan0", "100.72.1.9"),
                ("en0", "192.168.1.20"),
                ("en1", "203.0.113.7"),
            ]),
            &["100.101.102.103".parse().unwrap()],
        );
        assert_eq!(
            order(&ranked),
            ["192.168.1.20", "100.72.1.9", "203.0.113.7"]
        );
        assert!(ranked.iter().all(|a| !a.tailscale));
    }

    #[test]
    fn the_cli_cannot_promote_an_address_outside_100_64_slash_10() {
        let ranked = rank_addresses(
            ifs(&[("en0", "192.168.1.20")]),
            &["192.168.1.20".parse().unwrap()],
        );
        assert!(!ranked[0].tailscale);
    }

    #[test]
    fn skips_loopback_link_local_unspecified_and_duplicates() {
        let ranked = rank_addresses(
            ifs(&[
                ("lo0", "127.0.0.1"),
                ("en0", "169.254.10.1"),
                ("any", "0.0.0.0"),
                ("en0", "192.168.1.20"),
                ("en0-alias", "192.168.1.20"),
            ]),
            &[],
        );
        assert_eq!(order(&ranked), ["192.168.1.20"]);
    }

    #[test]
    fn cgnat_range_is_exactly_100_64_slash_10() {
        for (ip, expected) in [
            ("100.64.0.0", true),
            ("100.127.255.255", true),
            ("100.63.255.255", false),
            ("100.128.0.0", false),
        ] {
            assert_eq!(in_cgnat_range(ip.parse().unwrap()), expected, "{ip}");
        }
    }

    #[test]
    fn parses_tailscale_ip_output() {
        assert_eq!(
            parse_ipv4_lines("100.101.102.103\n\nnot-an-ip\n"),
            ["100.101.102.103".parse::<Ipv4Addr>().unwrap()]
        );
    }

    #[test]
    fn a_missing_cli_yields_nothing() {
        assert_eq!(
            run_with_timeout(
                "workbench-definitely-not-a-binary",
                &[],
                Duration::from_millis(100)
            ),
            None
        );
    }
}
