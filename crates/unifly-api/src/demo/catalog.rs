// ── Static catalog for the demo fleet ──
//
// Names, models, and tables for "Neon Harbor Studios", a fictional creative
// studio. Every address is from documentation or private ranges (RFC 5737,
// RFC 1918, RFC 3849) and every name is invented.

/// Display name of the demo site.
pub(crate) const SITE_NAME: &str = "Neon Harbor Studios";

/// Gateway WAN address (RFC 5737 TEST-NET-3).
pub(crate) const WAN_IP: &str = "203.0.113.42";
/// Upstream ISP gateway (RFC 5737 TEST-NET-3).
pub(crate) const WAN_GATEWAY: &str = "203.0.113.1";
pub(crate) const WAN_IPV6: &str = "2001:db8:4e48::1";
pub(crate) const ISP_NAME: &str = "Tidewater Fiber";
pub(crate) const NAMESERVERS: [&str; 2] = ["192.0.2.53", "198.51.100.53"];

/// Baseline WAN receive rate, bytes/s (about 170 Mbps).
pub(crate) const WAN_RX_BASE: f64 = 21_000_000.0;
/// Baseline WAN transmit rate, bytes/s (about 50 Mbps).
pub(crate) const WAN_TX_BASE: f64 = 6_500_000.0;

pub(crate) const GATEWAY_FIRMWARE: &str = "4.3.6";
pub(crate) const SWITCH_FIRMWARE: &str = "7.2.123";
pub(crate) const AP_FIRMWARE: &str = "8.1.113";
pub(crate) const AP_FIRMWARE_OLD: &str = "8.0.26";

/// A network (VLAN) in the demo fleet.
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct NetworkSpec {
    pub key: &'static str,
    pub name: &'static str,
    pub vlan: u16,
    /// Third octet of `10.40.x.0/24`.
    pub octet: u8,
    pub guest: bool,
    pub internet: bool,
    pub isolated: bool,
    pub ipv6: bool,
}

pub(crate) const NETWORKS: [NetworkSpec; 6] = [
    NetworkSpec {
        key: "mgmt",
        name: "Mgmt",
        vlan: 1,
        octet: 0,
        guest: false,
        internet: true,
        isolated: false,
        ipv6: true,
    },
    NetworkSpec {
        key: "studio",
        name: "Studio",
        vlan: 10,
        octet: 10,
        guest: false,
        internet: true,
        isolated: false,
        ipv6: true,
    },
    NetworkSpec {
        key: "render",
        name: "Render",
        vlan: 20,
        octet: 20,
        guest: false,
        internet: true,
        isolated: false,
        ipv6: false,
    },
    NetworkSpec {
        key: "guest",
        name: "Guest",
        vlan: 30,
        octet: 30,
        guest: true,
        internet: true,
        isolated: true,
        ipv6: false,
    },
    NetworkSpec {
        key: "iot",
        name: "IoT",
        vlan: 40,
        octet: 40,
        guest: false,
        internet: true,
        isolated: false,
        ipv6: false,
    },
    NetworkSpec {
        key: "cameras",
        name: "Cameras",
        vlan: 50,
        octet: 50,
        guest: false,
        internet: false,
        isolated: true,
        ipv6: false,
    },
];

/// An access point placement.
pub(crate) struct ApSpec {
    pub name: &'static str,
    pub model: &'static str,
    pub wifi7: bool,
    pub six_ghz: bool,
    /// Relative share of wireless clients this AP attracts.
    pub weight: u32,
}

/// Names stay at ten characters or fewer so the topology view shows them
/// whole.
pub(crate) const APS: [ApSpec; 9] = [
    ApSpec {
        name: "ap-beacon",
        model: "U7 Pro Max",
        wifi7: true,
        six_ghz: true,
        weight: 6,
    },
    ApSpec {
        name: "ap-gallery",
        model: "U7 Pro",
        wifi7: true,
        six_ghz: true,
        weight: 5,
    },
    ApSpec {
        name: "ap-render",
        model: "U7 Pro",
        wifi7: true,
        six_ghz: true,
        weight: 3,
    },
    ApSpec {
        name: "ap-edit",
        model: "U7 Pro Wall",
        wifi7: true,
        six_ghz: true,
        weight: 4,
    },
    ApSpec {
        name: "ap-atrium",
        model: "U6 Enterprise",
        wifi7: false,
        six_ghz: true,
        weight: 5,
    },
    ApSpec {
        name: "ap-lounge",
        model: "U7 Pro",
        wifi7: true,
        six_ghz: true,
        weight: 4,
    },
    ApSpec {
        name: "ap-loft",
        model: "U6 Mesh",
        wifi7: false,
        six_ghz: false,
        weight: 2,
    },
    ApSpec {
        name: "ap-roof",
        model: "U7 Outdoor",
        wifi7: true,
        six_ghz: false,
        weight: 2,
    },
    ApSpec {
        name: "ap-dock",
        model: "U7 Outdoor",
        wifi7: true,
        six_ghz: false,
        weight: 0,
    },
];

/// Index into [`APS`] of the AP that is offline at launch.
pub(crate) const OFFLINE_AP: usize = 8;
/// Index into [`APS`] of the AP mid-upgrade at launch.
pub(crate) const UPGRADING_AP: usize = 6;
/// Index into [`APS`] of the AP with a firmware update waiting.
pub(crate) const STALE_FIRMWARE_AP: usize = 7;

pub(crate) const CHANNELS_24: [u32; 3] = [1, 6, 11];
pub(crate) const CHANNELS_5: [u32; 8] = [36, 149, 52, 100, 116, 132, 157, 44];
pub(crate) const CHANNELS_6: [u32; 6] = [37, 69, 101, 133, 165, 197];

/// SSIDs: (name, network key, bands, security label).
pub(crate) const SSID_STUDIO: &str = "NeonHarbor";
pub(crate) const SSID_GUEST: &str = "Harbor Guest";
pub(crate) const SSID_IOT: &str = "Harbor-IoT";
pub(crate) const SSID_CREW: &str = "NH-Backstage";

/// Wireless client archetypes: (hostname prefix, device class, OS, network
/// key, count, preferred band in GHz, mobile).
pub(crate) struct ClientKind {
    pub prefix: &'static str,
    pub class: &'static str,
    pub os: &'static str,
    pub network: &'static str,
    pub count: usize,
    pub band: f32,
    pub mobile: bool,
    /// Typical sustained throughput (bytes/s) when active.
    pub rate: f64,
}

pub(crate) const WIRELESS_KINDS: [ClientKind; 11] = [
    ClientKind {
        prefix: "mbp-edit",
        class: "Laptop",
        os: "macOS",
        network: "studio",
        count: 18,
        band: 6.0,
        mobile: true,
        rate: 2_400_000.0,
    },
    ClientKind {
        prefix: "laptop-prod",
        class: "Laptop",
        os: "Windows 11",
        network: "studio",
        count: 12,
        band: 5.0,
        mobile: true,
        rate: 1_200_000.0,
    },
    ClientKind {
        prefix: "phone",
        class: "Smartphone",
        os: "iOS",
        network: "studio",
        count: 22,
        band: 5.0,
        mobile: true,
        rate: 180_000.0,
    },
    ClientKind {
        prefix: "pixel",
        class: "Smartphone",
        os: "Android",
        network: "studio",
        count: 14,
        band: 6.0,
        mobile: true,
        rate: 150_000.0,
    },
    ClientKind {
        prefix: "tablet-board",
        class: "Tablet",
        os: "iPadOS",
        network: "studio",
        count: 7,
        band: 5.0,
        mobile: true,
        rate: 400_000.0,
    },
    ClientKind {
        prefix: "crew-laptop",
        class: "Laptop",
        os: "macOS",
        network: "mgmt",
        count: 4,
        band: 6.0,
        mobile: true,
        rate: 600_000.0,
    },
    ClientKind {
        prefix: "guest",
        class: "Smartphone",
        os: "Android",
        network: "guest",
        count: 14,
        band: 5.0,
        mobile: true,
        rate: 220_000.0,
    },
    ClientKind {
        prefix: "speaker",
        class: "Smart Speaker",
        os: "Embedded",
        network: "iot",
        count: 6,
        band: 2.4,
        mobile: false,
        rate: 40_000.0,
    },
    ClientKind {
        prefix: "sensor",
        class: "IoT",
        os: "Embedded",
        network: "iot",
        count: 10,
        band: 2.4,
        mobile: false,
        rate: 2_000.0,
    },
    ClientKind {
        prefix: "display",
        class: "Smart TV",
        os: "tvOS",
        network: "iot",
        count: 3,
        band: 5.0,
        mobile: false,
        rate: 900_000.0,
    },
    ClientKind {
        prefix: "handheld",
        class: "Game Console",
        os: "Embedded",
        network: "iot",
        count: 2,
        band: 5.0,
        mobile: false,
        rate: 300_000.0,
    },
];

/// Suffixes for sensors and speakers, so IoT hostnames read like places.
pub(crate) const PLACES: [&str; 12] = [
    "lobby", "gallery", "lounge", "kitchen", "atrium", "loft", "stage", "dock", "roof", "vault",
    "edit", "foyer",
];

/// Wired endpoints: (hostname, class, OS, network key, switch index, port,
/// fixed host octet, link speed, typical rate).
pub(crate) struct WiredSpec {
    pub name: &'static str,
    pub class: &'static str,
    pub os: &'static str,
    pub network: &'static str,
    pub switch: usize,
    pub port: u32,
    pub host: u8,
    pub speed: u32,
    pub rate: f64,
    pub poe: bool,
}

const fn wired(
    name: &'static str,
    class: &'static str,
    network: &'static str,
    switch: usize,
    port: u32,
    host: u8,
) -> WiredSpec {
    WiredSpec {
        name,
        class,
        os: "Linux",
        network,
        switch,
        port,
        host,
        speed: 2_500,
        rate: 300_000.0,
        poe: false,
    }
}

/// Switch indices: 0 = sw-core, 1 = sw-render, 2 = sw-lounge.
pub(crate) const WIRED: [WiredSpec; 30] = [
    WiredSpec {
        rate: 9_000_000.0,
        speed: 10_000,
        ..wired("nas-vault", "NAS", "render", 1, 17, 5)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-01", "Render Node", "render", 1, 1, 11)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-02", "Render Node", "render", 1, 2, 12)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-03", "Render Node", "render", 1, 3, 13)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-04", "Render Node", "render", 1, 4, 14)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-05", "Render Node", "render", 1, 5, 15)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-06", "Render Node", "render", 1, 6, 16)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-07", "Render Node", "render", 1, 7, 17)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-08", "Render Node", "render", 1, 8, 18)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-09", "Render Node", "render", 1, 9, 19)
    },
    WiredSpec {
        rate: 4_000_000.0,
        ..wired("render-10", "Render Node", "render", 1, 10, 20)
    },
    WiredSpec {
        rate: 2_500_000.0,
        ..wired("dailies-review", "Server", "render", 1, 11, 40)
    },
    WiredSpec {
        rate: 600_000.0,
        ..wired("license-srv", "Server", "render", 1, 12, 41)
    },
    WiredSpec {
        os: "macOS",
        rate: 3_000_000.0,
        ..wired("ws-color-01", "Desktop", "studio", 0, 1, 21)
    },
    WiredSpec {
        os: "macOS",
        rate: 2_000_000.0,
        ..wired("ws-edit-01", "Desktop", "studio", 0, 2, 22)
    },
    WiredSpec {
        os: "macOS",
        rate: 2_000_000.0,
        ..wired("ws-edit-02", "Desktop", "studio", 0, 3, 23)
    },
    WiredSpec {
        os: "Windows 11",
        rate: 2_500_000.0,
        ..wired("ws-vfx-01", "Desktop", "studio", 0, 4, 24)
    },
    WiredSpec {
        os: "Windows 11",
        rate: 2_500_000.0,
        ..wired("ws-vfx-02", "Desktop", "studio", 0, 5, 25)
    },
    WiredSpec {
        os: "Linux",
        rate: 1_500_000.0,
        ..wired("ws-pipeline", "Desktop", "studio", 0, 6, 26)
    },
    WiredSpec {
        os: "Embedded",
        speed: 100,
        rate: 4_000.0,
        ..wired("printer-ops", "Printer", "iot", 0, 7, 30)
    },
    WiredSpec {
        os: "Embedded",
        speed: 1_000,
        rate: 6_000.0,
        ..wired("plotter-01", "Printer", "iot", 0, 8, 31)
    },
    WiredSpec {
        os: "Embedded",
        speed: 1_000,
        rate: 300_000.0,
        poe: true,
        ..wired("cam-lobby", "Camera", "cameras", 0, 9, 21)
    },
    WiredSpec {
        os: "Embedded",
        speed: 1_000,
        rate: 300_000.0,
        poe: true,
        ..wired("cam-gallery", "Camera", "cameras", 0, 10, 22)
    },
    WiredSpec {
        os: "Embedded",
        speed: 1_000,
        rate: 300_000.0,
        poe: true,
        ..wired("cam-stage", "Camera", "cameras", 0, 11, 23)
    },
    WiredSpec {
        os: "Embedded",
        speed: 1_000,
        rate: 300_000.0,
        poe: true,
        ..wired("cam-roof", "Camera", "cameras", 0, 13, 25)
    },
    WiredSpec {
        os: "Embedded",
        speed: 100,
        rate: 1_000.0,
        poe: true,
        ..wired("badge-front", "IoT", "iot", 0, 14, 32)
    },
    WiredSpec {
        os: "Embedded",
        speed: 1_000,
        rate: 400_000.0,
        ..wired("console-lounge", "Game Console", "iot", 2, 2, 50)
    },
    WiredSpec {
        os: "tvOS",
        speed: 1_000,
        rate: 700_000.0,
        ..wired("tv-lounge", "Smart TV", "iot", 2, 3, 51)
    },
    WiredSpec {
        os: "Embedded",
        speed: 1_000,
        rate: 20_000.0,
        ..wired("hub-lighting", "IoT", "iot", 2, 4, 52)
    },
    WiredSpec {
        os: "Embedded",
        speed: 1_000,
        rate: 90_000.0,
        ..wired("dj-booth", "Desktop", "studio", 2, 5, 60)
    },
];

/// The camera whose switch port is down: (name, sw-core port).
pub(crate) const OFFLINE_CAMERA: (&str, u32) = ("cam-dock", 12);

/// Neighboring networks seen over the air. Empty names are hidden SSIDs.
pub(crate) const NEIGHBOR_SSIDS: [&str; 16] = [
    "Saltline Coffee",
    "Dockside-Guest",
    "Pier 9 Cowork",
    "Tidewater Loft",
    "Ferry Terminal WiFi",
    "Brightwater Dental",
    "harbor-printshop",
    "DIRECT-7F-Plotter",
    "Gallery Annex 5G",
    "Mooring Bistro",
    "",
    "Lantern Yoga",
    "",
    "SeaGlass-Apartments",
    "Wharf Bikes",
    "Anchor Ramen",
];

/// DPI applications: (id, name, category id, share of traffic).
/// Names stay at fourteen characters or fewer to fit the Stats panels.
pub(crate) const DPI_APPS: [(u32, &str, u32, f64); 14] = [
    (101, "Asset Sync", 2, 0.22),
    (102, "Render Uplink", 2, 0.17),
    (103, "Review Stream", 3, 0.12),
    (104, "Music", 3, 0.07),
    (105, "Video Calls", 5, 0.08),
    (106, "Team Chat", 0, 0.03),
    (107, "Code Hosting", 13, 0.04),
    (108, "OS Updates", 14, 0.06),
    (109, "Web Browsing", 13, 0.08),
    (110, "Plugin Store", 13, 0.02),
    (111, "Remote Desktop", 9, 0.04),
    (112, "Online Games", 7, 0.03),
    (113, "Social Media", 24, 0.02),
    (114, "Email", 4, 0.02),
];

/// DPI category ids used above, with short display names.
pub(crate) const DPI_CATEGORIES: [(u32, &str); 10] = [
    (0, "Messaging"),
    (2, "Transfers"),
    (3, "Streaming"),
    (4, "Mail"),
    (5, "VoIP"),
    (7, "Games"),
    (9, "Remote"),
    (13, "Web"),
    (14, "Updates"),
    (24, "Social"),
];

/// External addresses that show up in security events (RFC 5737).
pub(crate) const HOSTILE_IPS: [&str; 6] = [
    "198.51.100.23",
    "198.51.100.187",
    "192.0.2.77",
    "192.0.2.140",
    "203.0.113.201",
    "198.51.100.9",
];

/// IPS signatures for security events.
pub(crate) const IPS_SIGNATURES: [&str; 5] = [
    "ET SCAN Potential SSH Scan",
    "ET SCAN Suspicious inbound to mySQL port 3306",
    "ET POLICY Outbound plaintext credentials",
    "ET SCAN NMAP OS Detection Probe",
    "ET DROP Spamhaus DROP Listed Traffic Inbound",
];
