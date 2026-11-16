//! Web addresses as policy checks see them: the host an address names, and whether that
//! host is this computer or the local network. Parsing only; no network I/O.

/// The lower-case host of an HTTP(S) address (no port, no user, no brackets), or `None`.
pub fn host(url: &str) -> Option<String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    let authority = &rest[..rest.find(['/', '?', '#', '\\']).unwrap_or(rest.len())];
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        &bracketed[..bracketed.find(']')?]
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    let host = host.trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_owned())
}

/// Whether a host is this computer or the local network: names such as `localhost` and
/// `*.local`, private, loopback and link-local addresses, and any numeric host written in a
/// form other than the four plain decimal parts (`2130706433`, `0x7f.1`) that a URL
/// parser would still read as an IPv4 address.
pub fn is_local_host(host: &str) -> bool {
    use std::net::{IpAddr, Ipv4Addr};
    let private_v4 = |ip: Ipv4Addr| {
        let [a, b, ..] = ip.octets();
        ip.is_private()
            || ip.is_loopback()
            || ip.is_link_local()
            || ip.is_unspecified()
            || ip.is_broadcast()
            // Shared address space (carrier-grade NAT), 100.64.0.0/10.
            || (a == 100 && (64..128).contains(&b))
            || a == 0
    };
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return true;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => private_v4(ip),
        Ok(IpAddr::V6(ip)) => {
            let first = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                // Unique local fc00::/7 and link-local fe80::/10.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || ip.to_ipv4_mapped().is_some_and(private_v4)
        }
        Err(_) => {
            // A last label of digits or 0x-hex makes the host an IPv4 address to URL parsers.
            let last = host.rsplit('.').next().unwrap_or(host);
            !last.is_empty()
                && (last.bytes().all(|b| b.is_ascii_digit())
                    || last
                        .strip_prefix("0x")
                        .is_some_and(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_read_as_url_parsers_read_them() {
        assert_eq!(
            host("https://user@evil.example/x").as_deref(),
            Some("evil.example")
        );
        assert_eq!(host("http://[::1]:8080/").as_deref(), Some("::1"));
        assert_eq!(
            host("https://DATA.example:443/b.csv").as_deref(),
            Some("data.example")
        );
        // A backslash ends the host, as in browsers.
        assert_eq!(
            host("https://evil.example\\@good.example/").as_deref(),
            Some("evil.example")
        );
        assert_eq!(host("file:///etc/passwd"), None);
        assert_eq!(host("https:///path"), None);
    }

    #[test]
    fn local_and_private_hosts_are_recognised() {
        for host in [
            "localhost",
            "printer.local",
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "2130706433",
            "0x7f.1",
            "127.1",
        ] {
            assert!(is_local_host(host), "{host}");
        }
        for host in [
            "example.org",
            "8.8.8.8",
            "2001:4860::8888",
            "cafe.de",
            "1.example",
        ] {
            assert!(!is_local_host(host), "{host}");
        }
    }

    #[test]
    fn any_address_is_read_without_panicking() {
        let mut inputs = crate::test_inputs::Inputs::new(4);
        let pieces = [
            "https://",
            "http://",
            "[",
            "]",
            ":",
            "@",
            "\\",
            "/",
            "?",
            "#",
            ".",
            "127",
            "0x7f",
            "1",
            "example",
            "é",
            "::1",
            "fe80::",
            "localhost",
        ];
        for _ in 0..50_000 {
            if let Some(host) = host(&inputs.text(&pieces, 10)) {
                let _ = is_local_host(&host);
            }
        }
    }
}
