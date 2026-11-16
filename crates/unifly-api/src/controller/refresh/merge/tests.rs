use crate::integration_types::ClientResponse;
use crate::model::Client;
use crate::session::models::SessionClientEntry;

#[test]
fn hybrid_clients_take_the_session_network_name() {
    let mut clients: Vec<Client> = vec![
        ClientResponse {
            id: uuid::Uuid::from_u128(1),
            name: "render-07".into(),
            client_type: "WIRED".into(),
            ip_address: Some("10.40.20.17".into()),
            connected_at: None,
            mac_address: Some("3e:bf:ec:41:e9:0c".into()),
            access: serde_json::json!({ "type": "DEFAULT" }),
        }
        .into(),
    ];
    let session: SessionClientEntry = serde_json::from_value(serde_json::json!({
        "_id": "abc",
        "mac": "3e:bf:ec:41:e9:0c",
        "ip": "10.40.20.17",
        "network": "Render",
        "network_id": "5f1a2b3c4d5e6f7a8b9c0d1e"
    }))
    .expect("deserialize");

    super::merge_session_clients(&mut clients, &[session], &[]);

    assert_eq!(clients[0].network_name.as_deref(), Some("Render"));
}

#[test]
fn hybrid_clients_take_the_session_ipv6_addresses() {
    let mut clients: Vec<Client> = vec![
        ClientResponse {
            id: uuid::Uuid::from_u128(1),
            name: "selene".into(),
            client_type: "WIRELESS".into(),
            ip_address: Some("10.4.20.198".into()),
            connected_at: None,
            mac_address: Some("3e:bf:ec:41:e9:0d".into()),
            access: serde_json::json!({ "type": "DEFAULT" }),
        }
        .into(),
    ];
    let session: SessionClientEntry = serde_json::from_value(serde_json::json!({
        "_id": "abc",
        "mac": "3e:bf:ec:41:e9:0d",
        "ip": "10.4.20.198",
        "last_ipv6": ["2001:db8:3fa::9"],
    }))
    .expect("deserialize");

    super::merge_session_clients(&mut clients, &[session], &[]);

    assert!(clients[0].has_address("2001:db8:3fa::9".parse().expect("address")));
}

/// The MAC ties the records together, so a client with no IPv4 address
/// still takes its session addresses.
#[test]
fn an_ipv6_only_client_is_merged_by_its_mac() {
    let mut clients: Vec<Client> = vec![
        ClientResponse {
            id: uuid::Uuid::from_u128(2),
            name: "printer".into(),
            client_type: "WIRED".into(),
            ip_address: None,
            connected_at: None,
            mac_address: Some("3E:BF:EC:41:E9:0E".into()),
            access: serde_json::json!({ "type": "DEFAULT" }),
        }
        .into(),
    ];
    let session: SessionClientEntry = serde_json::from_value(serde_json::json!({
        "_id": "abc",
        "mac": "3e:bf:ec:41:e9:0e",
        "hostname": "printer-lab",
        "ipv6_addresses": ["2001:db8:3fa::7"],
    }))
    .expect("deserialize");

    super::merge_session_clients(&mut clients, &[session], &[]);

    assert_eq!(clients[0].hostname.as_deref(), Some("printer-lab"));
    assert_eq!(clients[0].primary_address(), "2001:db8:3fa::7".parse().ok());
}

/// A record without a MAC falls back to its address, but not to one two
/// stations report.
#[test]
fn an_address_two_stations_share_merges_neither() {
    let mut clients: Vec<Client> = vec![
        ClientResponse {
            id: uuid::Uuid::from_u128(3),
            name: "vpn-peer".into(),
            client_type: "VPN".into(),
            ip_address: Some("10.4.30.5".into()),
            connected_at: None,
            mac_address: None,
            access: serde_json::json!({ "type": "DEFAULT" }),
        }
        .into(),
    ];
    let station = |mac: &str, hostname: &str| -> SessionClientEntry {
        serde_json::from_value(serde_json::json!({
            "_id": mac, "mac": mac, "ip": "10.4.30.5", "hostname": hostname,
            "ipv6_addresses": ["2001:db8:30::5"],
        }))
        .expect("deserialize")
    };
    let sessions = [
        station("3e:bf:ec:41:e9:01", "one"),
        station("3e:bf:ec:41:e9:02", "two"),
    ];

    super::merge_session_clients(&mut clients, &sessions, &[]);

    assert_eq!(clients[0].hostname, None);
    assert_eq!(clients[0].ipv6_addresses, Vec::<std::net::Ipv6Addr>::new());
}
