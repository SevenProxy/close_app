use std::{
    collections::{HashMap, HashSet},
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Transport {
    Tcp,
    Udp,
}

#[derive(Clone, Debug)]
pub struct OpenPort {
    pub transport: Transport,
    pub address: String,
    pub port: u16,
    pub state: &'static str,
    pub inode: u64,
    pub pid: Option<u32>,
    pub process: Option<String>,
}

impl OpenPort {
    pub fn is_exposed(&self) -> bool {
        is_external(&self.address)
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim();
        if query.is_empty() {
            return true;
        }

        self.port.to_string().contains(query)
            || self.pid.is_some_and(|pid| pid.to_string().contains(query))
            || self.process.as_deref().is_some_and(|name| {
                name.to_ascii_lowercase()
                    .contains(&query.to_ascii_lowercase())
            })
            || self.address.contains(query)
    }
}

pub fn collect() -> Vec<OpenPort> {
    let mut ports = Vec::new();
    parse_table("/proc/net/tcp", Transport::Tcp, false, &mut ports);
    parse_table("/proc/net/tcp6", Transport::Tcp, true, &mut ports);
    parse_table("/proc/net/udp", Transport::Udp, false, &mut ports);
    parse_table("/proc/net/udp6", Transport::Udp, true, &mut ports);

    let inodes: HashSet<u64> = ports.iter().map(|port| port.inode).collect();
    let owners = owners(&inodes);
    for port in &mut ports {
        if let Some((pid, process)) = owners.get(&port.inode) {
            port.pid = Some(*pid);
            port.process = Some(process.clone());
        }
    }

    ports.sort_by(|a, b| {
        b.is_exposed()
            .cmp(&a.is_exposed())
            .then(a.port.cmp(&b.port))
            .then(a.transport.cmp(&b.transport))
    });
    ports
}

pub fn is_external(address: &str) -> bool {
    address.parse::<IpAddr>().is_ok_and(|ip| !ip.is_loopback())
}

fn parse_table(path: &str, transport: Transport, ipv6: bool, ports: &mut Vec<OpenPort>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }

        let wanted_state = match transport {
            Transport::Tcp => "0A",
            Transport::Udp => "07",
        };
        if fields[3] != wanted_state {
            continue;
        }

        let Some((address, port)) = parse_endpoint(fields[1], ipv6) else {
            continue;
        };
        let Ok(inode) = fields[9].parse::<u64>() else {
            continue;
        };

        ports.push(OpenPort {
            transport,
            address,
            port,
            state: match transport {
                Transport::Tcp => "LISTEN",
                Transport::Udp => "UNCONN",
            },
            inode,
            pid: None,
            process: None,
        });
    }
}

fn parse_endpoint(raw: &str, ipv6: bool) -> Option<(String, u16)> {
    let (address, port) = raw.rsplit_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let address = if ipv6 {
        format_ipv6(address)?
    } else {
        format_ipv4(address)?
    };
    Some((address, port))
}

fn format_ipv4(hex: &str) -> Option<String> {
    if hex.len() != 8 {
        return None;
    }

    let bytes: Option<Vec<u8>> = hex
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(pair, 16).ok()
        })
        .collect();

    let bytes = bytes?;
    Some(Ipv4Addr::new(bytes[3], bytes[2], bytes[1], bytes[0]).to_string())
}

fn format_ipv6(hex: &str) -> Option<String> {
    if hex.len() != 32 {
        return None;
    }

    let mut bytes = [0u8; 16];
    for group in 0..4 {
        for byte in 0..4 {
            let source = group * 8 + byte * 2;
            bytes[group * 4 + byte] = u8::from_str_radix(&hex[source..source + 2], 16).ok()?;
        }
        let start = group * 4;
        bytes[start..start + 4].reverse();
    }

    Some(Ipv6Addr::from(bytes).to_string())
}

fn owners(inodes: &HashSet<u64>) -> HashMap<u64, (u32, String)> {
    let mut owners = HashMap::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return owners;
    };

    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(descriptors) = fs::read_dir(entry.path().join("fd")) else {
            continue;
        };

        for descriptor in descriptors.flatten() {
            let Ok(target) = fs::read_link(descriptor.path()) else {
                continue;
            };
            let Some(text) = target.to_str() else {
                continue;
            };
            let Some(inode) = text
                .strip_prefix("socket:[")
                .and_then(|value| value.strip_suffix(']'))
                .and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };

            if inodes.contains(&inode) {
                let process = fs::read_to_string(entry.path().join("comm"))
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                owners.insert(inode, (pid, process));
            }
        }
    }

    owners
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ipv4_endpoints_from_proc() {
        assert_eq!(
            parse_endpoint("0100007F:1F90", false),
            Some(("127.0.0.1".into(), 8080))
        );
        assert_eq!(
            parse_endpoint("00000000:01BB", false),
            Some(("0.0.0.0".into(), 443))
        );
    }

    #[test]
    fn parses_ipv6_endpoints_from_proc() {
        assert_eq!(
            parse_endpoint("00000000000000000000000001000000:1F90", true),
            Some(("::1".into(), 8080))
        );
    }

    #[test]
    fn detects_non_loopback_addresses() {
        assert!(is_external("0.0.0.0"));
        assert!(is_external("::"));
        assert!(is_external("192.168.1.10"));
        assert!(!is_external("127.0.0.1"));
        assert!(!is_external("::1"));
    }

    #[test]
    fn matches_port_pid_process_and_address() {
        let port = OpenPort {
            transport: Transport::Tcp,
            address: "0.0.0.0".into(),
            port: 8080,
            state: "LISTEN",
            inode: 123,
            pid: Some(42),
            process: Some("vesktop".into()),
        };

        assert!(port.matches("8080"));
        assert!(port.matches("42"));
        assert!(port.matches("VESK"));
        assert!(port.matches("0.0.0.0"));
        assert!(!port.matches("nginx"));
        assert!(port.is_exposed());
    }
}
