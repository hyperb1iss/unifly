use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::json;
use unifly_api::Network;

use super::{Field, FormError, NetworkForm, Submit};
use crate::tui::widgets::line_input::LineInput;

/// A gateway-managed network serving DHCP, with `fields` laid over it.
fn network(fields: &serde_json::Value) -> Network {
    let mut record = json!({
        "id": "11111111-1111-1111-1111-111111111111",
        "name": "Lab", "vlan_id": 30, "enabled": true,
        "management": "Gateway", "purpose": "Corporate", "is_default": false,
        "subnet": "10.0.30.0/24", "gateway_ip": "10.0.30.1",
        "isolation_enabled": false, "internet_access_enabled": true,
        "mdns_forwarding_enabled": false,
        "ipv6_enabled": true, "ipv6_mode": "PrefixDelegation", "ipv6_prefix": null,
        "dhcpv6_enabled": false, "slaac_enabled": true,
        "ntp_server": null, "pxe_enabled": false, "tftp_server": null,
        "firewall_zone_id": null, "cellular_backup_enabled": false, "origin": null,
        "dhcp": {
            "enabled": true, "range_start": "10.0.30.100", "range_stop": "10.0.30.200",
            "lease_time_secs": 3600, "dns_servers": ["10.0.30.2"], "gateway": "10.0.30.1"
        }
    });
    for (key, value) in fields.as_object().into_iter().flatten() {
        record[key] = value.clone();
    }
    serde_json::from_value(record).expect("network fixture")
}

fn edit(fields: &serde_json::Value) -> NetworkForm {
    NetworkForm::edit(&network(fields))
}

/// Replace what `field` holds with `text`.
fn fill(form: &mut NetworkForm, field: Field, text: &str) {
    form.focus_on(field);
    form.edit_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    form.edit_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    form.paste(text);
}

fn flip(form: &mut NetworkForm, field: Field) {
    form.focus_on(field);
    assert_eq!(form.focused(), field, "the form shows {field:?}");
    form.toggle();
}

/// The request an edit sends, as JSON.
fn request(form: &NetworkForm) -> serde_json::Value {
    match form.submit() {
        Ok(Submit::Update { request, .. }) => serde_json::to_value(request).expect("JSON"),
        other => panic!("an update, not {other:?}"),
    }
}

fn refusal(form: &NetworkForm) -> FormError {
    form.submit().expect_err("the draft cannot be saved")
}

#[test]
fn a_network_is_offered_the_fields_its_kind_takes() {
    let fields = |fields: serde_json::Value| edit(&fields).fields().to_vec();
    let shared = [Field::Name, Field::Vlan, Field::Enabled];

    assert_eq!(fields(json!({"management": "Unmanaged"})), shared);
    assert_eq!(
        fields(json!({"management": null})),
        shared,
        "a kind not known here gets what every kind takes"
    );
    let switch = fields(json!({"management": "Switch"}));
    assert!(switch.contains(&Field::Dhcp) && switch.contains(&Field::Isolation));
    for gateway_only in [Field::Internet, Field::Mdns, Field::Ipv6] {
        assert!(!switch.contains(&gateway_only), "{gateway_only:?}");
    }
    let gateway = fields(json!({}));
    assert!(gateway.contains(&Field::Internet) && gateway.contains(&Field::Ipv6));
}

/// Turning IPv6 on needs a prefix source and an address assignment the
/// form cannot ask for, so it only offers the way it can go: off.
#[test]
fn ipv6_is_offered_only_where_it_can_be_turned_off_and_says_what_goes() {
    let without = edit(&json!({"ipv6_enabled": false, "ipv6_mode": null}));
    assert!(!without.fields().contains(&Field::Ipv6));

    let mut form = edit(&json!({}));
    assert_eq!(form.note(Field::Ipv6), None);
    flip(&mut form, Field::Ipv6);
    assert_eq!(
        form.note(Field::Ipv6).as_deref(),
        Some("deletes its IPv6 settings")
    );
    assert_eq!(request(&form), json!({"ipv6_enabled": false}));
    let costs = form.consequences().join("\n");
    assert!(costs.contains("Its IPv6 settings are deleted"), "{costs}");

    // Flipped back before saving, nothing is sent.
    flip(&mut form, Field::Ipv6);
    assert!(matches!(form.submit(), Ok(Submit::Unchanged)));
}

#[test]
fn turning_dhcp_off_says_that_its_configuration_is_deleted() {
    let mut form = edit(&json!({}));
    flip(&mut form, Field::Dhcp);

    assert_eq!(
        form.note(Field::Dhcp).as_deref(),
        Some("deletes its range and options")
    );
    assert_eq!(request(&form), json!({"dhcp_enabled": false}));
    assert_eq!(
        form.consequences(),
        [
            "The DHCP server's range, lease time, and options are deleted.",
            "Devices on it can lose their connection while it applies.",
        ]
    );
}

#[test]
fn a_relay_is_named_as_one_and_a_server_says_it_replaces_it() {
    let relay = json!({"dhcp": {
        "enabled": false, "range_start": null, "range_stop": null,
        "lease_time_secs": null, "dns_servers": [], "gateway": "10.0.30.1",
        "relay_servers": ["10.0.0.5", "10.0.0.6"],
    }});
    let mut form = edit(&relay);
    assert_eq!(
        form.note(Field::Dhcp).as_deref(),
        Some("relays to 10.0.0.5 +1")
    );
    assert_eq!(form.consequences(), Vec::<String>::new());

    flip(&mut form, Field::Dhcp);
    fill(&mut form, Field::RangeStart, "10.0.30.100");
    fill(&mut form, Field::RangeStop, "10.0.30.200");
    assert_eq!(form.note(Field::Dhcp), None);
    let costs = form.consequences().join("\n");
    assert!(
        costs.contains("DHCP is relayed to 10.0.0.5 +1 today. A server here replaces the relay."),
        "{costs}"
    );
}

#[test]
fn an_emptied_dns_field_asks_for_the_gateways_own_servers() {
    let mut form = edit(&json!({}));
    fill(&mut form, Field::Dns, "");
    assert_eq!(request(&form), json!({"dhcp": {"dns_servers": []}}));
}

#[test]
fn dns_servers_are_at_most_four_ipv4_addresses() {
    let mut form = edit(&json!({}));
    fill(&mut form, Field::Dns, "1.1.1.1, 1.0.0.1 8.8.8.8,8.8.4.4");
    assert_eq!(
        request(&form)["dhcp"]["dns_servers"],
        json!(["1.1.1.1", "1.0.0.1", "8.8.8.8", "8.8.4.4"])
    );

    fill(
        &mut form,
        Field::Dns,
        "1.1.1.1, 1.0.0.1, 8.8.8.8, 8.8.4.4, 9.9.9.9",
    );
    let error = refusal(&form);
    assert_eq!(error.field, Some(Field::Dns));
    assert_eq!(error.message, "A DHCP server takes at most 4 DNS servers");

    // An IPv6 address cannot be typed: the field takes no colon.
    fill(&mut form, Field::Dns, "2001:db8::53");
    assert_eq!(form.input(Field::Dns).map(LineInput::text), Some("2001853"));
    assert_eq!(refusal(&form).message, "2001853 is not an IPv4 address");
}

#[test]
fn a_vlan_id_is_one_the_controller_takes() {
    let mut form = edit(&json!({}));
    for refused in ["1", "4010", "0"] {
        fill(&mut form, Field::Vlan, refused);
        let error = refusal(&form);
        assert_eq!(error.field, Some(Field::Vlan), "{refused}");
        assert_eq!(error.message, "VLAN ID must be between 2 and 4009");
    }
    for taken in ["2", "4009"] {
        fill(&mut form, Field::Vlan, taken);
        assert_eq!(
            request(&form),
            json!({"vlan_id": taken.parse::<u16>().ok()})
        );
    }

    let mut default = edit(&json!({"is_default": true, "vlan_id": 1}));
    fill(&mut default, Field::Vlan, "20");
    assert_eq!(
        refusal(&default).message,
        "The default network keeps VLAN 1"
    );
}

#[test]
fn a_prefix_and_a_dhcp_range_are_ones_the_controller_takes() {
    let mut form = edit(&json!({}));
    for prefix in ["10.0.30.1/31", "10.0.30.1/7"] {
        fill(&mut form, Field::Gateway, prefix);
        let error = refusal(&form);
        assert_eq!(error.field, Some(Field::Gateway), "{prefix}");
        assert_eq!(error.message, "The prefix must be between /8 and /30");
    }

    let mut form = edit(&json!({}));
    for (start, stop, field, message) in [
        // The subnet's own address and its broadcast address.
        (
            "10.0.30.0",
            "10.0.30.200",
            Field::RangeStart,
            "Must be inside 10.0.30.1 to 10.0.30.254",
        ),
        (
            "10.0.30.100",
            "10.0.30.255",
            Field::RangeStop,
            "Must be inside 10.0.30.1 to 10.0.30.254",
        ),
        (
            "10.0.30.1",
            "10.0.30.200",
            Field::RangeStart,
            "The range must not include the gateway's own address, 10.0.30.1",
        ),
    ] {
        fill(&mut form, Field::RangeStart, start);
        fill(&mut form, Field::RangeStop, stop);
        let error = refusal(&form);
        assert_eq!(
            (error.field, error.message.as_str()),
            (Some(field), message)
        );
    }
    fill(&mut form, Field::RangeStart, "10.0.30.2");
    fill(&mut form, Field::RangeStop, "10.0.30.254");
    assert_eq!(
        request(&form)["dhcp"],
        json!({"range_start": "10.0.30.2", "range_stop": "10.0.30.254"})
    );
}

/// A new network with no gateway address is created VLAN-only, which
/// has neither setting. They used to be dropped without a word.
#[test]
fn a_new_vlan_only_network_cannot_be_isolated_or_cut_off() {
    let draft = || {
        let mut form = NetworkForm::create();
        fill(&mut form, Field::Name, "Trunk");
        fill(&mut form, Field::Vlan, "60");
        form
    };
    for (field, message) in [
        (
            Field::Isolation,
            "A network needs a gateway address to be isolated",
        ),
        (
            Field::Internet,
            "A network needs a gateway address to have its internet access cut",
        ),
    ] {
        let mut form = draft();
        flip(&mut form, field);
        let error = refusal(&form);
        assert_eq!(
            (error.field, error.message.as_str()),
            (Some(field), message)
        );

        fill(&mut form, Field::Gateway, "10.0.60.1/24");
        assert!(matches!(form.submit(), Ok(Submit::Create(_))), "{field:?}");
    }
    assert!(matches!(draft().submit(), Ok(Submit::Create(_))));
}
