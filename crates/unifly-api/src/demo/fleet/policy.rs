// ── Demo policy: SSIDs, firewall zones and policies, NAT, ACLs ──

use super::{Fleet, network_index, uuid_from};
use crate::demo::catalog;
use crate::demo::rng::DemoRng;
use crate::model::common::DataSource;
use crate::model::{
    AclAction, AclRule, AclRuleType, EntityId, EntityOrigin, FirewallAction, FirewallPolicy,
    FirewallZone, IpSpec, IpVersion, NatPolicy, NatType, PolicyEndpoint, PortSpec, TrafficFilter,
    WifiBroadcast, WifiBroadcastType, WifiSecurityMode,
};

pub(super) fn build(fleet: &mut Fleet, rng: &mut DemoRng) {
    fleet.wifi = wifi_broadcasts(fleet, rng);
    fleet.zones = zones(fleet, rng);
    for zone in &fleet.zones {
        for network_id in &zone.network_ids {
            if let Some(network) = fleet
                .networks
                .iter_mut()
                .find(|network| &network.id == network_id)
            {
                network.firewall_zone_id = Some(zone.id.clone());
            }
        }
    }
    fleet.policies = policies(fleet, rng);
    fleet.nat = nat_policies(fleet, rng);
    fleet.acls = acl_rules(rng);
}

fn net_id(fleet: &Fleet, key: &str) -> EntityId {
    fleet.networks[network_index(key)].id.clone()
}

fn wifi_broadcasts(fleet: &Fleet, rng: &mut DemoRng) -> Vec<WifiBroadcast> {
    let ssid = |rng: &mut DemoRng,
                name: &str,
                key: &str,
                security: WifiSecurityMode,
                frequencies: &[f32]| WifiBroadcast {
        id: EntityId::Uuid(uuid_from(rng)),
        name: name.into(),
        enabled: true,
        broadcast_type: WifiBroadcastType::Standard,
        security,
        network_id: Some(net_id(fleet, key)),
        frequencies_ghz: frequencies.to_vec(),
        hidden: false,
        client_isolation: false,
        band_steering: frequencies.len() > 1,
        mlo_enabled: false,
        fast_roaming: false,
        hotspot_enabled: false,
        origin: Some(EntityOrigin::UserDefined),
        source: DataSource::IntegrationApi,
    };

    let mut studio = ssid(
        rng,
        catalog::SSID_STUDIO,
        "studio",
        WifiSecurityMode::Wpa2Wpa3Personal,
        &[2.4, 5.0, 6.0],
    );
    studio.mlo_enabled = true;
    studio.fast_roaming = true;

    let mut guest = ssid(
        rng,
        catalog::SSID_GUEST,
        "guest",
        WifiSecurityMode::Wpa2Personal,
        &[2.4, 5.0],
    );
    guest.client_isolation = true;
    guest.hotspot_enabled = true;

    let mut iot = ssid(
        rng,
        catalog::SSID_IOT,
        "iot",
        WifiSecurityMode::Wpa2Personal,
        &[2.4, 5.0],
    );
    iot.broadcast_type = WifiBroadcastType::IotOptimized;

    let mut crew = ssid(
        rng,
        catalog::SSID_CREW,
        "mgmt",
        WifiSecurityMode::Wpa3Enterprise,
        &[5.0, 6.0],
    );
    crew.hidden = true;
    crew.fast_roaming = true;

    vec![studio, guest, iot, crew]
}

fn zones(fleet: &Fleet, rng: &mut DemoRng) -> Vec<FirewallZone> {
    let zone = |rng: &mut DemoRng, name: &str, keys: &[&str], origin: EntityOrigin| FirewallZone {
        id: EntityId::Uuid(uuid_from(rng)),
        name: name.into(),
        network_ids: keys.iter().map(|key| net_id(fleet, key)).collect(),
        origin: Some(origin),
        source: DataSource::IntegrationApi,
    };
    vec![
        zone(
            rng,
            "Internal",
            &["mgmt", "studio"],
            EntityOrigin::SystemDefined,
        ),
        zone(rng, "Render", &["render"], EntityOrigin::UserDefined),
        zone(rng, "IoT", &["iot", "cameras"], EntityOrigin::UserDefined),
        zone(rng, "Hotspot", &["guest"], EntityOrigin::SystemDefined),
        zone(rng, "External", &[], EntityOrigin::SystemDefined),
        zone(rng, "Gateway", &[], EntityOrigin::SystemDefined),
        zone(rng, "VPN", &[], EntityOrigin::SystemDefined),
        zone(rng, "DMZ", &[], EntityOrigin::SystemDefined),
    ]
}

struct PolicySpec<'a> {
    name: &'a str,
    action: FirewallAction,
    from: &'a str,
    to: &'a str,
    source: Option<TrafficFilter>,
    destination: Option<TrafficFilter>,
    protocol: Option<&'a str>,
    /// Short source and destination labels for the policy table.
    labels: (&'a str, &'a str),
}

fn ports(items: &[&str]) -> PortSpec {
    PortSpec::Values {
        items: items.iter().map(|item| (*item).to_owned()).collect(),
        match_opposite: false,
    }
}

#[allow(clippy::too_many_lines)]
fn policies(fleet: &Fleet, rng: &mut DemoRng) -> Vec<FirewallPolicy> {
    let networks = |keys: &[&str]| TrafficFilter::Network {
        network_ids: keys.iter().map(|key| net_id(fleet, key)).collect(),
        match_opposite: false,
        mac_addresses: Vec::new(),
        ports: None,
    };
    let port_filter = |items: &[&str]| TrafficFilter::Port {
        ports: ports(items),
    };

    let specs = [
        PolicySpec {
            name: "Studio to render farm",
            action: FirewallAction::Allow,
            from: "Internal",
            to: "Render",
            source: Some(networks(&["studio"])),
            destination: Some(port_filter(&["22", "445", "2049", "8443"])),
            protocol: Some("TCP"),
            labels: ("Studio", "Render :22 +3"),
        },
        PolicySpec {
            name: "Render cloud uplink",
            action: FirewallAction::Allow,
            from: "Render",
            to: "External",
            source: None,
            destination: Some(TrafficFilter::IpAddress {
                addresses: vec![IpSpec::Subnet {
                    value: "198.51.100.0/24".into(),
                }],
                match_opposite: false,
                mac_addresses: Vec::new(),
                ports: Some(ports(&["443"])),
            }),
            protocol: Some("TCP"),
            labels: ("Render", "Cloud :443"),
        },
        PolicySpec {
            name: "Render stays offline",
            action: FirewallAction::Block,
            from: "Render",
            to: "External",
            source: None,
            destination: None,
            protocol: None,
            labels: ("Render", "External"),
        },
        PolicySpec {
            name: "Isolate IoT",
            action: FirewallAction::Block,
            from: "IoT",
            to: "Internal",
            source: None,
            destination: None,
            protocol: None,
            labels: ("IoT", "Internal"),
        },
        PolicySpec {
            name: "Casting to IoT",
            action: FirewallAction::Allow,
            from: "Internal",
            to: "IoT",
            source: Some(networks(&["studio"])),
            destination: Some(port_filter(&["7000", "8009", "5353"])),
            protocol: Some("TCP/UDP"),
            labels: ("Studio", "IoT :7000 +2"),
        },
        PolicySpec {
            name: "Cameras stay local",
            action: FirewallAction::Block,
            from: "IoT",
            to: "External",
            source: Some(networks(&["cameras"])),
            destination: None,
            protocol: None,
            labels: ("Cameras", "External"),
        },
        PolicySpec {
            name: "Guests internet only",
            action: FirewallAction::Block,
            from: "Hotspot",
            to: "Internal",
            source: None,
            destination: None,
            protocol: None,
            labels: ("Hotspot", "Internal"),
        },
        PolicySpec {
            name: "Drop inbound SSH",
            action: FirewallAction::Block,
            from: "External",
            to: "Gateway",
            source: None,
            destination: Some(port_filter(&["22"])),
            protocol: Some("TCP"),
            labels: ("External", "Gateway :22"),
        },
        PolicySpec {
            name: "Crew VPN to studio",
            action: FirewallAction::Allow,
            from: "VPN",
            to: "Internal",
            source: None,
            destination: Some(networks(&["studio", "render"])),
            protocol: None,
            labels: ("VPN", "Studio, Render"),
        },
        PolicySpec {
            name: "Vendor remote session",
            action: FirewallAction::Allow,
            from: "External",
            to: "Render",
            source: Some(TrafficFilter::IpAddress {
                addresses: vec![IpSpec::Address {
                    value: "192.0.2.10".into(),
                }],
                match_opposite: false,
                mac_addresses: Vec::new(),
                ports: None,
            }),
            destination: Some(port_filter(&["3389"])),
            protocol: Some("TCP"),
            labels: ("192.0.2.10", "Render :3389"),
        },
        PolicySpec {
            name: "Allow Return Traffic",
            action: FirewallAction::Allow,
            from: "Internal",
            to: "External",
            source: None,
            destination: None,
            protocol: None,
            labels: ("Internal", "External"),
        },
        PolicySpec {
            name: "Block Invalid Traffic",
            action: FirewallAction::Block,
            from: "External",
            to: "Internal",
            source: None,
            destination: None,
            protocol: None,
            labels: ("External", "Internal"),
        },
    ];

    let zone_id = |name: &str| {
        fleet
            .zones
            .iter()
            .find(|zone| zone.name == name)
            .map(|zone| zone.id.clone())
    };

    specs
        .into_iter()
        .zip(1..)
        .map(|(spec, index)| {
            let system =
                spec.name.starts_with("Allow Return") || spec.name.starts_with("Block Inv");
            let (source_summary, destination_summary) = spec.labels;
            FirewallPolicy {
                id: EntityId::Uuid(uuid_from(rng)),
                name: spec.name.into(),
                description: None,
                enabled: spec.name != "Vendor remote session",
                index: Some(index),
                action: spec.action,
                ip_version: IpVersion::Both,
                source: PolicyEndpoint {
                    zone_id: zone_id(spec.from),
                    filter: spec.source,
                },
                destination: PolicyEndpoint {
                    zone_id: zone_id(spec.to),
                    filter: spec.destination,
                },
                source_summary: Some(source_summary.into()),
                destination_summary: Some(destination_summary.into()),
                protocol_summary: spec.protocol.map(Into::into),
                schedule: (spec.name == "Vendor remote session")
                    .then(|| "Weekdays 09:00-17:00".into()),
                ipsec_mode: None,
                connection_states: if spec.name.starts_with("Allow Return") {
                    vec!["ESTABLISHED".into(), "RELATED".into()]
                } else if spec.name.starts_with("Block Inv") {
                    vec!["INVALID".into()]
                } else {
                    Vec::new()
                },
                logging_enabled: matches!(spec.action, FirewallAction::Block) && !system,
                origin: Some(if system {
                    EntityOrigin::SystemDefined
                } else {
                    EntityOrigin::UserDefined
                }),
                data_source: DataSource::IntegrationApi,
            }
        })
        .collect()
}

fn nat_policies(fleet: &Fleet, rng: &mut DemoRng) -> Vec<NatPolicy> {
    let wan = Some(EntityId::Uuid(uuid_from(rng)));
    let nat = |rng: &mut DemoRng, name: &str, nat_type: NatType| NatPolicy {
        id: EntityId::Uuid(uuid_from(rng)),
        name: name.into(),
        description: None,
        enabled: true,
        nat_type,
        interface_id: wan.clone(),
        protocol: None,
        src_address: None,
        src_port: None,
        dst_address: None,
        dst_port: None,
        translated_address: None,
        translated_port: None,
        origin: Some(EntityOrigin::UserDefined),
        data_source: DataSource::SessionApi,
    };

    let mut masquerade = nat(rng, "WAN masquerade", NatType::Masquerade);
    masquerade.src_address = Some("10.40.0.0/16".into());
    masquerade.origin = Some(EntityOrigin::SystemDefined);

    let mut review = nat(rng, "Dailies review portal", NatType::Destination);
    review.protocol = Some("tcp".into());
    review.dst_address = Some(catalog::WAN_IP.into());
    review.dst_port = Some("443".into());
    review.translated_address = Some("10.40.20.40".into());
    review.translated_port = Some("8443".into());

    let mut dashboard = nat(rng, "Render farm dashboard", NatType::Destination);
    dashboard.enabled = false;
    dashboard.protocol = Some("tcp".into());
    dashboard.dst_address = Some(catalog::WAN_IP.into());
    dashboard.dst_port = Some("8080".into());
    dashboard.translated_address = Some("10.40.20.41".into());
    dashboard.translated_port = Some("8080".into());

    let mut egress = nat(rng, "Render egress", NatType::Source);
    egress
        .src_address
        .clone_from(&fleet.network("render").subnet);
    egress.translated_address = Some("203.0.113.43".into());

    vec![masquerade, review, dashboard, egress]
}

fn acl_rules(rng: &mut DemoRng) -> Vec<AclRule> {
    let rule = |rng: &mut DemoRng,
                name: &str,
                rule_type: AclRuleType,
                action: AclAction,
                (src, dst): (&str, &str)| AclRule {
        id: EntityId::Uuid(uuid_from(rng)),
        name: name.into(),
        enabled: true,
        rule_type,
        action,
        source_summary: Some(src.into()),
        destination_summary: Some(dst.into()),
        origin: Some(EntityOrigin::UserDefined),
        source: DataSource::IntegrationApi,
    };
    let mut quarantine = rule(
        rng,
        "Quarantine unknown MACs",
        AclRuleType::Mac,
        AclAction::Block,
        ("unlisted MACs", "any"),
    );
    quarantine.enabled = false;
    vec![
        rule(
            rng,
            "IoT off the management VLAN",
            AclRuleType::Ipv4,
            AclAction::Block,
            ("10.40.40.0/24", "10.40.0.0/24"),
        ),
        rule(
            rng,
            "Render to NAS fast path",
            AclRuleType::Ipv4,
            AclAction::Allow,
            ("10.40.20.0/24", "10.40.20.5"),
        ),
        rule(
            rng,
            "Cameras to recorder only",
            AclRuleType::Ipv4,
            AclAction::Allow,
            ("10.40.50.0/24", "10.40.20.5"),
        ),
        quarantine,
    ]
}
