//! Which network the phone link may use. The decisions are pure (which peers
//! are private, which adapter is the real LAN, whether the network's category
//! allows a session); the readers of Windows' adapter list and of the Network
//! List Manager are the only code that looks at the machine.

use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

use super::Refusal;

/// `IF_TYPE_IEEE80211`.
pub const IF_WIFI: u32 = 71;
/// `IF_TYPE_ETHERNET_CSMACD`.
pub const IF_ETHERNET: u32 = 6;
const IF_PPP: u32 = 23;
const IF_LOOPBACK: u32 = 24;
const IF_TUNNEL: u32 = 131;

/// Whether `ip` is an address of a local network: 10/8, 172.16/12,
/// 192.168/16, link-local 169.254/16, IPv6 fe80::/10 and fc00::/7. Loopback
/// counts only when `allow_loopback` (the debug flag and the tests).
pub fn is_private(ip: IpAddr, allow_loopback: bool) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let o = a.octets();
            a.is_private() || (o[0] == 169 && o[1] == 254) || (a.is_loopback() && allow_loopback)
        }
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4_mapped() {
                return is_private(IpAddr::V4(v4), allow_loopback);
            }
            let first = a.segments()[0];
            (first & 0xffc0) == 0xfe80
                || (first & 0xfe00) == 0xfc00
                || (a.is_loopback() && allow_loopback)
        }
    }
}

/// One network adapter, as much as the choice needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterInfo {
    /// The adapter GUID in braces, as `GetAdaptersAddresses` names it.
    pub guid: String,
    pub name: String,
    pub description: String,
    pub if_type: u32,
    pub up: bool,
    pub has_gateway: bool,
    pub ipv4: Vec<Ipv4Addr>,
}

impl AdapterInfo {
    /// The first address of a local network (a link-local 169.254 address
    /// means no address was given, so it does not count).
    pub fn private_ip(&self) -> Option<Ipv4Addr> {
        self.ipv4
            .iter()
            .copied()
            .find(|a| a.is_private() && !a.is_link_local())
    }

    fn is_virtual(&self) -> bool {
        if matches!(self.if_type, IF_PPP | IF_TUNNEL | IF_LOOPBACK) {
            return true;
        }
        const WORDS: [&str; 12] = [
            "hyper-v",
            "vethernet",
            "wsl",
            "virtualbox",
            "vmware",
            "tap",
            "wireguard",
            "vpn",
            "tailscale",
            "zerotier",
            "loopback",
            "bluetooth",
        ];
        let hay = format!("{} {}", self.name, self.description).to_lowercase();
        WORDS.iter().any(|w| hay.contains(w))
    }
}

/// The adapter the link goes on: up, with a default gateway and a local IPv4
/// address, and not a virtual one (VPN, VM switch, WSL, tunnel, Bluetooth).
/// Wi-Fi beats Ethernet beats anything else; the first of equals wins.
pub fn pick_adapter(adapters: &[AdapterInfo]) -> Option<AdapterInfo> {
    let rank = |a: &AdapterInfo| match a.if_type {
        IF_WIFI => 0,
        IF_ETHERNET => 1,
        _ => 2,
    };
    adapters
        .iter()
        .filter(|a| a.up && a.has_gateway && a.private_ip().is_some() && !a.is_virtual())
        .min_by_key(|a| rank(a))
        .cloned()
}

/// A network's category as Windows files it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Public,
    Private,
    Domain,
}

/// Whether a session may start on a network of this category. A Public one
/// is refused, and so is one whose category could not be read: not knowing
/// is treated as Public (the safe default).
pub fn start_allowed(category: Option<Category>) -> Result<(), Refusal> {
    match category {
        Some(Category::Private | Category::Domain) => Ok(()),
        Some(Category::Public) | None => Err(Refusal::Public {
            network_name: String::new(),
        }),
    }
}

/// How often the adapter list is read again to see that the IP is still ours.
pub const BOUND_EVERY: Duration = Duration::from_secs(5);

/// Rate limit for "is the IP still on an adapter": reads at most every
/// `BOUND_EVERY`, otherwise answers what it last found.
pub struct BoundCheck {
    last: Option<Instant>,
    ok: bool,
}

impl BoundCheck {
    pub fn new() -> Self {
        Self {
            last: None,
            ok: true,
        }
    }

    pub fn check(
        &mut self,
        now: Instant,
        ip: Ipv4Addr,
        read: impl FnOnce() -> Vec<AdapterInfo>,
    ) -> bool {
        let due = self
            .last
            .is_none_or(|t| now.saturating_duration_since(t) >= BOUND_EVERY);
        if due {
            self.last = Some(now);
            self.ok = read().iter().any(|a| a.up && a.ipv4.contains(&ip));
        }
        self.ok
    }
}

/// Windows' adapter list (`GetAdaptersAddresses`, IPv4 only).
pub fn read_adapters() -> Vec<AdapterInfo> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_SKIP_ANYCAST
        | GAA_FLAG_SKIP_MULTICAST
        | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size = 15_000u32;
    // The list can grow between the two calls: a few tries.
    let mut buf: Vec<u64> = Vec::new();
    let mut ok = false;
    for _ in 0..4 {
        buf = vec![0u64; (size as usize).div_ceil(8)];
        let status = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET.0),
                flags,
                None,
                Some(buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
                &mut size,
            )
        };
        if status == 0 {
            ok = true;
            break;
        }
        // ERROR_BUFFER_OVERFLOW: `size` now holds what is needed.
        if status != 111 {
            break;
        }
    }
    if !ok {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut p = buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !p.is_null() {
        // SAFETY: the list lives in `buf`, which outlives this loop, and the
        // system filled it as a chain of `IP_ADAPTER_ADDRESSES_LH`.
        let a = unsafe { &*p };
        let mut ipv4 = Vec::new();
        let mut u = a.FirstUnicastAddress;
        while !u.is_null() {
            let ua = unsafe { &*u };
            let sa = ua.Address.lpSockaddr;
            if !sa.is_null() && unsafe { (*sa).sa_family } == AF_INET {
                let sin = unsafe { &*sa.cast::<SOCKADDR_IN>() };
                let raw = unsafe { sin.sin_addr.S_un.S_addr };
                // Stored in network order: the first octet is the low byte.
                ipv4.push(Ipv4Addr::from(raw.to_ne_bytes()));
            }
            u = ua.Next;
        }
        let wide = |s: windows::core::PWSTR| {
            if s.is_null() {
                String::new()
            } else {
                unsafe { s.to_string() }.unwrap_or_default()
            }
        };
        let guid = if a.AdapterName.is_null() {
            String::new()
        } else {
            let raw = unsafe { std::ffi::CStr::from_ptr(a.AdapterName.0.cast()) };
            raw.to_string_lossy().into_owned()
        };
        out.push(AdapterInfo {
            guid,
            name: wide(a.FriendlyName),
            description: wide(a.Description),
            if_type: a.IfType,
            up: a.OperStatus == IfOperStatusUp,
            has_gateway: !a.FirstGatewayAddress.is_null(),
            ipv4,
        });
        p = a.Next;
    }
    out
}

/// A network's category and name for the adapter `guid` ("{...}"), from the
/// Network List Manager. Needs COM on the calling thread (the server thread
/// initialises it). `None` when nothing says what the adapter's network is.
pub fn network_for(guid: &str) -> Option<(Category, String)> {
    use windows::Win32::Networking::NetworkListManager::{
        INetworkListManager, NLM_NETWORK_CATEGORY_PRIVATE,
        NLM_NETWORK_CATEGORY_PUBLIC, NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED,
        NetworkListManager,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};

    let want = guid.trim_matches(['{', '}']).to_ascii_lowercase();
    unsafe {
        let nlm: INetworkListManager = CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()?;
        let conns = nlm.GetNetworkConnections().ok()?;
        loop {
            let mut one = [None];
            let mut fetched = 0u32;
            if conns.Next(&mut one, Some(&mut fetched)).is_err() || fetched == 0 {
                return None;
            }
            let conn = one[0].take()?;
            let Ok(id) = conn.GetAdapterId() else {
                continue;
            };
            if guid_text(&id) != want {
                continue;
            }
            let net = conn.GetNetwork().ok()?;
            let cat = net.GetCategory().ok()?;
            let name = net.GetName().map(|n| n.to_string()).unwrap_or_default();
            let category = if cat == NLM_NETWORK_CATEGORY_PUBLIC {
                Category::Public
            } else if cat == NLM_NETWORK_CATEGORY_PRIVATE {
                Category::Private
            } else if cat == NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED {
                Category::Domain
            } else {
                return Some((Category::Public, name));
            };
            return Some((category, name));
        }
    }
}

/// A GUID as lower-case text without braces.
fn guid_text(g: &windows::core::GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g.data1,
        g.data2,
        g.data3,
        g.data4[0],
        g.data4[1],
        g.data4[2],
        g.data4[3],
        g.data4[4],
        g.data4[5],
        g.data4[6],
        g.data4[7]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv6Addr;

    fn v4(s: &str) -> IpAddr {
        IpAddr::V4(s.parse().expect("ip"))
    }

    fn v6(s: &str) -> IpAddr {
        IpAddr::V6(s.parse::<Ipv6Addr>().expect("ip"))
    }

    #[test]
    fn local_ranges_are_private_and_the_rest_is_not() {
        for ip in [
            "10.0.0.1",
            "10.255.255.255",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.1.1",
            "169.254.3.4",
        ] {
            assert!(is_private(v4(ip), false), "{ip}");
        }
        for ip in [
            "8.8.8.8",
            "172.15.0.1",
            "172.32.0.1",
            "192.169.0.1",
            "11.0.0.1",
            "100.64.0.1",
            "0.0.0.0",
            "255.255.255.255",
            "1.1.1.1",
        ] {
            assert!(!is_private(v4(ip), false), "{ip}");
        }
    }

    #[test]
    fn loopback_counts_only_when_allowed() {
        assert!(!is_private(v4("127.0.0.1"), false));
        assert!(is_private(v4("127.0.0.1"), true));
        assert!(!is_private(v6("::1"), false));
        assert!(is_private(v6("::1"), true));
        // Allowing loopback does not let a public address in.
        assert!(!is_private(v4("8.8.8.8"), true));
    }

    #[test]
    fn ipv6_local_ranges() {
        for ip in ["fe80::1", "febf::1", "fc00::1", "fd12:3456::1"] {
            assert!(is_private(v6(ip), false), "{ip}");
        }
        for ip in ["2001:4860:4860::8888", "fec0::1", "::", "ff02::1", "2606:4700::1111"] {
            assert!(!is_private(v6(ip), false), "{ip}");
        }
        // A v4 address wrapped in v6 is judged as the v4 one.
        assert!(is_private(v6("::ffff:192.168.1.5"), false));
        assert!(!is_private(v6("::ffff:8.8.8.8"), false));
    }

    fn ad(name: &str, if_type: u32, ip: &str) -> AdapterInfo {
        AdapterInfo {
            guid: format!("{{{name}}}"),
            name: name.to_owned(),
            description: String::new(),
            if_type,
            up: true,
            has_gateway: true,
            ipv4: vec![ip.parse().expect("ip")],
        }
    }

    #[test]
    fn wifi_beats_ethernet_and_the_first_of_equals_wins() {
        let list = [
            ad("Ethernet", IF_ETHERNET, "192.168.1.5"),
            ad("Wi-Fi", IF_WIFI, "192.168.18.42"),
            ad("Wi-Fi 2", IF_WIFI, "192.168.18.43"),
        ];
        assert_eq!(pick_adapter(&list).expect("pick").name, "Wi-Fi");
        assert_eq!(
            pick_adapter(&list[..1]).expect("pick").private_ip(),
            Some("192.168.1.5".parse().expect("ip"))
        );
    }

    #[test]
    fn virtual_down_gatewayless_and_public_adapters_are_skipped() {
        let mut vm = ad("vEthernet (Default Switch)", IF_ETHERNET, "172.20.0.1");
        vm.description = "Hyper-V Virtual Ethernet Adapter".into();
        let mut vpn = ad("Ethernet 2", IF_ETHERNET, "10.8.0.2");
        vpn.description = "TAP-Windows Adapter V9".into();
        let mut wsl = ad("vEthernet (WSL)", IF_ETHERNET, "172.30.0.1");
        wsl.description = "Hyper-V Virtual Ethernet Adapter".into();
        let mut ts = ad("Tailscale", IF_ETHERNET, "10.1.2.3");
        ts.description = "Tailscale Tunnel".into();
        let ppp = ad("Dial-up", IF_PPP, "10.0.0.9");
        let tunnel = ad("Teredo", IF_TUNNEL, "10.0.0.8");
        let mut down = ad("Wi-Fi", IF_WIFI, "192.168.1.9");
        down.up = false;
        let mut no_gw = ad("Ethernet 3", IF_ETHERNET, "192.168.5.5");
        no_gw.has_gateway = false;
        let public = ad("Ethernet 4", IF_ETHERNET, "203.0.113.5");
        let apipa = ad("Ethernet 5", IF_ETHERNET, "169.254.1.1");
        let mut bt = ad("Bluetooth Network Connection", IF_ETHERNET, "192.168.44.1");
        bt.description = "Bluetooth Device (Personal Area Network)".into();
        let all = [
            vm, vpn, wsl, ts, ppp, tunnel, down, no_gw, public, apipa, bt,
        ];
        assert_eq!(pick_adapter(&all), None);
        // The real one among them is found.
        let mut with_real = all.to_vec();
        with_real.push(ad("Wi-Fi", IF_WIFI, "192.168.18.42"));
        assert_eq!(pick_adapter(&with_real).expect("pick").name, "Wi-Fi");
        assert_eq!(pick_adapter(&[]), None);
    }

    #[test]
    fn a_public_or_unknown_network_is_refused() {
        assert_eq!(start_allowed(Some(Category::Private)), Ok(()));
        assert_eq!(start_allowed(Some(Category::Domain)), Ok(()));
        assert!(matches!(
            start_allowed(Some(Category::Public)),
            Err(Refusal::Public { .. })
        ));
        assert!(matches!(start_allowed(None), Err(Refusal::Public { .. })));
    }

    #[test]
    fn the_bound_check_reads_at_most_every_five_seconds() {
        let ip: Ipv4Addr = "192.168.1.5".parse().expect("ip");
        let t0 = Instant::now();
        let mut b = BoundCheck::new();
        let mut reads = 0;
        let mut now = t0;
        let mut ask = |b: &mut BoundCheck, now: Instant, there: bool| {
            b.check(now, ip, || {
                reads += 1;
                if there {
                    vec![ad("Wi-Fi", IF_WIFI, "192.168.1.5")]
                } else {
                    vec![ad("Wi-Fi", IF_WIFI, "10.0.0.5")]
                }
            })
        };
        assert!(ask(&mut b, now, true));
        now += Duration::from_secs(2);
        // Too soon: the last answer stands even though the list changed.
        assert!(ask(&mut b, now, false));
        now += Duration::from_secs(4);
        assert!(!ask(&mut b, now, false));
        drop(ask);
        assert_eq!(reads, 2);
    }
}
