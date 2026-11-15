//! A URL never grants an SSH route. Remote loopback requires a bound source.
use url::{Host, Url};
pub const MAX_URL: usize = 8192;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    Public,
    Loopback { host: String, port: u16 },
}
pub fn parse(text: &str) -> Result<Url, String> {
    if text.is_empty()
        || text.len() > MAX_URL
        || text.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("Enter an HTTP or HTTPS URL without control characters".into());
    }
    let uri = Url::parse(text).map_err(|_| "Invalid web URL")?;
    if !matches!(uri.scheme(), "http" | "https")
        || uri.host().is_none()
        || !uri.username().is_empty()
        || uri.password().is_some()
    {
        return Err("Only HTTP/HTTPS URLs without embedded credentials are supported".into());
    }
    Ok(uri)
}
pub fn destination(uri: &Url) -> Result<Destination, String> {
    let host = uri.host_str().ok_or("Web host missing")?;
    let target = match uri.host().unwrap() {
        Host::Domain(d)
            if d.eq_ignore_ascii_case("localhost") || d.eq_ignore_ascii_case("localhost.") =>
        {
            Some("127.0.0.1")
        }
        Host::Ipv4(ip) if ip.is_loopback() || ip.is_unspecified() => Some("127.0.0.1"),
        Host::Ipv6(ip) if ip.is_loopback() => Some("::1"),
        Host::Ipv4(ip) if ip.is_private() || ip.is_link_local() || ip.is_unspecified() => {
            return Err(
                "Intranet forwarding is not enabled; only remote loopback previews are supported"
                    .into(),
            );
        }
        Host::Ipv6(ip)
            if ip.is_unique_local() || ip.is_unicast_link_local() || ip.is_unspecified() =>
        {
            return Err("Intranet forwarding is not enabled".into());
        }
        Host::Domain(d) if !d.contains('.') => {
            return Err("Use a public web hostname or an explicit remote loopback URL".into());
        }
        _ => None,
    };
    Ok(match target {
        Some(target) => Destination::Loopback {
            host: target.into(),
            port: uri.port_or_known_default().ok_or("Web port missing")?,
        },
        None => {
            let _ = host;
            Destination::Public
        }
    })
}
pub fn mapped(uri: &Url, port: u16) -> Url {
    let mut mapped = uri.clone();
    // HTTPS still validates the URL hostname; no certificate exceptions.
    let _ = mapped.set_host(Some(if uri.scheme() == "https" {
        "localhost"
    } else {
        "127.0.0.1"
    }));
    let _ = mapped.set_port(Some(port));
    mapped
}
/// Map an HTTPS view to its dedicated loopback identity without changing scheme,
/// path or query. The address is allocated and validated by the SSH forward.
pub fn mapped_to(uri: &Url, host: std::net::Ipv4Addr, port: u16) -> Url {
    let mut mapped = uri.clone();
    let _ = mapped.set_host(Some(&host.to_string()));
    let _ = mapped.set_port(Some(port));
    mapped
}
/// The remote-loopback URL for "Open remote web service…": a port on the bound
/// SSH machine's own loopback and an optional path, never another host.
pub fn service_url(port: &str, path: &str, https: bool) -> Result<Url, String> {
    let port: u16 = port
        .trim()
        .parse()
        .ok()
        .filter(|p| *p > 0)
        .ok_or("Enter a port from 1 to 65535")?;
    let path = path.trim();
    if path.len() > 2048 || path.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("The path must be one line without spaces".into());
    }
    let path = if path.is_empty() {
        "/".to_owned()
    } else if path.starts_with('/') && !path.starts_with("//") {
        path.to_owned()
    } else if path.starts_with(['?', '#']) {
        format!("/{path}")
    } else if !path.contains("://") && !path.starts_with('/') {
        format!("/{path}")
    } else {
        return Err("Enter a path on this machine, such as /app".into());
    };
    let scheme = if https { "https" } else { "http" };
    let uri = parse(&format!("{scheme}://localhost:{port}{path}"))?;
    match destination(&uri)? {
        Destination::Loopback { port: p, .. }
            if p == port && uri.host_str() == Some("localhost") =>
        {
            Ok(uri)
        }
        _ => Err("Enter a path on this machine, such as /app".into()),
    }
}
/// Conservative fallback for terminals without exported OSC 8 attributes.
/// The caller supplies the clicked character index, never executes the text.
pub fn link_at(line: &str, at: usize) -> Option<String> {
    if line.len() > MAX_URL * 2 || at >= line.chars().count() {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let mut start = at;
    let mut end = at;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    while end < chars.len() && !chars[end].is_whitespace() {
        end += 1;
    }
    let token = chars[start..end].iter().collect::<String>();
    let mut candidate = token
        .trim_start_matches(['(', '[', '<', '"', '\''])
        .trim_end_matches(['>', '.', ',', ';', '"', '\'']);
    // Drop prose punctuation without removing balanced URL path parentheses
    // or an IPv6 host's closing bracket.
    while let Some(last) = candidate.chars().last() {
        let open = match last {
            ')' => '(',
            ']' => '[',
            _ => break,
        };
        if candidate.matches(last).count() <= candidate.matches(open).count() {
            break;
        }
        candidate = &candidate[..candidate.len() - last.len_utf8()];
    }
    parse(candidate).ok().map(|u| u.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn balanced_paths_and_ipv6_hosts_survive_prose_delimiters() {
        assert_eq!(
            link_at("(https://example.org/a(b)).", 12).as_deref(),
            Some("https://example.org/a(b)")
        );
        assert_eq!(link_at("http://[::1]", 4).as_deref(), Some("http://[::1]/"));
    }
    #[test]
    fn dangerous_schemes_credentials_controls_and_huge_urls_are_rejected() {
        for u in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "https://host.test/\nabc",
        ] {
            assert!(parse(u).is_err(), "{u}");
        }
        let mut credentials = parse("https://example.test/").unwrap();
        credentials.set_username("generated-user").unwrap();
        credentials.set_password(Some("generated-value")).unwrap();
        assert!(parse(credentials.as_str()).is_err());
        assert!(parse(&format!("https://host.test/{}", "x".repeat(MAX_URL))).is_err());
    }
    #[test]
    fn explicit_loopback_classification_never_tunnels_intranet() {
        for u in [
            "http://localhost:5173/",
            "http://127.0.0.1:5173/",
            "http://0.0.0.0:5173/",
            "http://[::1]:5173/",
        ] {
            assert!(matches!(
                destination(&parse(u).unwrap()).unwrap(),
                Destination::Loopback { port: 5173, .. }
            ));
        }
        assert_eq!(
            destination(&parse("https://example.org/").unwrap()).unwrap(),
            Destination::Public
        );
        assert!(
            destination(
                &parse(&format!(
                    "http://{}:5173/",
                    std::net::Ipv4Addr::new(10, 0, 0, 1)
                ))
                .unwrap()
            )
            .is_err()
        );
        assert!(destination(&parse("http://server:5173/").unwrap()).is_err());
    }
    #[test]
    fn remote_service_urls_stay_on_the_bound_machines_loopback() {
        assert_eq!(
            service_url("3092", "", false).unwrap().as_str(),
            "http://localhost:3092/"
        );
        assert_eq!(
            service_url(" 3092 ", "app/x?q=1#top", false)
                .unwrap()
                .as_str(),
            "http://localhost:3092/app/x?q=1#top"
        );
        assert_eq!(
            service_url("8443", "/", true).unwrap().as_str(),
            "https://localhost:8443/"
        );
        assert_eq!(
            service_url("80", "?q", false).unwrap().as_str(),
            "http://localhost/?q"
        );
        for port in ["0", "65536", "-1", "abc", ""] {
            assert!(service_url(port, "", false).is_err(), "{port}");
        }
        // A path can never redirect the preview to another host or scheme.
        for path in [
            "//example.org/",
            "http://example.org/",
            "/a b",
            "/a\nb",
            "@example.org",
        ] {
            let result = service_url("3092", path, false);
            assert!(
                result
                    .as_ref()
                    .map_or(true, |u| u.host_str() == Some("localhost")
                        && u.port() == Some(3092)),
                "{path}: {result:?}"
            );
        }
        assert!(service_url("3092", "//example.org/", false).is_err());
        assert!(service_url("3092", "http://example.org/", false).is_err());
    }
    #[test]
    fn mapped_navigation_preserves_path_and_memory_only_query() {
        let u = parse("http://localhost:5173/path?q=memory#fragment").unwrap();
        let m = mapped(&u, 31999);
        assert_eq!(m.as_str(), "http://127.0.0.1:31999/path?q=memory#fragment");
        assert_eq!(
            link_at("open (https://example.org/path). next", 15).as_deref(),
            Some("https://example.org/path")
        );
    }
}

#[cfg(test)]
mod isolated_tls_tests {
    use super::*;
    #[test]
    fn distinct_tls_routes_keep_remote_request_and_scheme() {
        let original = parse("https://localhost:3082/app?ticket=generated#page").unwrap();
        let a = mapped_to(&original, "127.64.0.1".parse().unwrap(), 32001);
        let b = mapped_to(&original, "127.64.0.2".parse().unwrap(), 32002);
        assert_ne!(a.host_str(), b.host_str());
        assert_ne!(a.origin(), b.origin());
        assert_eq!(a.scheme(), "https");
        assert_eq!(a.path(), original.path());
        assert_eq!(a.query(), original.query());
        assert_eq!(a.fragment(), original.fragment());
        assert_eq!(original.host_str(), Some("localhost"));
        assert_eq!(original.port(), Some(3082));
    }
}
