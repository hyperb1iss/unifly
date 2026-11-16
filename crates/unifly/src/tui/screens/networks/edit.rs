//! The network form: one network's settings as a draft, for editing an
//! existing network or creating a new one. The draft is validated here and
//! turned into the request the controller takes; only changed fields are
//! sent on an edit.

use std::net::Ipv4Addr;

use crossterm::event::KeyEvent;
use unifly_api::model::NetworkManagement;
use unifly_api::{CreateNetworkRequest, DhcpUpdate, EntityId, Network, UpdateNetworkRequest};

use crate::tui::widgets::line_input::{Edit, LineInput};

/// One setting the form shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Name,
    Vlan,
    Gateway,
    Enabled,
    Dhcp,
    RangeStart,
    RangeStop,
    Dns,
    Isolation,
    Internet,
    Mdns,
    Ipv6,
}

impl Field {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Vlan => "VLAN ID",
            Self::Gateway => "Gateway/prefix",
            Self::Enabled => "Enabled",
            Self::Dhcp => "DHCP server",
            Self::RangeStart => "Start",
            Self::RangeStop => "Stop",
            Self::Dns => "DNS servers",
            Self::Isolation => "Isolation",
            Self::Internet => "Internet",
            Self::Mdns => "mDNS forward",
            Self::Ipv6 => "IPv6",
        }
    }

    /// What the field holds, as the confirmation names it.
    fn noun(self) -> &'static str {
        match self {
            Self::RangeStart => "DHCP start",
            Self::RangeStop => "DHCP stop",
            other => other.label(),
        }
    }

    pub(super) fn is_toggle(self) -> bool {
        matches!(
            self,
            Self::Enabled | Self::Dhcp | Self::Isolation | Self::Internet | Self::Mdns | Self::Ipv6
        )
    }

    /// Whether the field belongs to the DHCP server and sits under its
    /// toggle.
    pub(super) fn is_dhcp_setting(self) -> bool {
        matches!(self, Self::RangeStart | Self::RangeStop | Self::Dns)
    }

    /// Whether the field opens a new group in the form.
    pub(super) fn starts_group(self) -> bool {
        matches!(self, Self::Dhcp | Self::Isolation)
    }

    /// What an empty text field shows, as guidance.
    pub(super) fn placeholder(self) -> &'static str {
        match self {
            Self::Gateway => "10.0.60.1/24, or empty for VLAN only",
            Self::RangeStart | Self::RangeStop => "IPv4 address",
            Self::Dns => "the gateway's own",
            _ => "",
        }
    }

    /// Whether a typed or pasted character belongs in the field.
    fn accepts(self, ch: char) -> bool {
        match self {
            Self::Name => true,
            Self::Vlan => ch.is_ascii_digit(),
            Self::Gateway => ch.is_ascii_digit() || matches!(ch, '.' | '/'),
            Self::RangeStart | Self::RangeStop => ch.is_ascii_digit() || ch == '.',
            Self::Dns => ch.is_ascii_digit() || matches!(ch, '.' | ',' | ' '),
            _ => false,
        }
    }
}

/// What the form saves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    /// An existing network, by ID, with the name it had when the form
    /// opened.
    Edit {
        id: EntityId,
        name: String,
    },
    Create,
}

/// Why the draft cannot be saved, and the field to fix. A failure the
/// controller reports belongs to no field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FormError {
    pub(super) field: Option<Field>,
    pub(super) message: String,
}

impl FormError {
    fn at(field: Field, message: impl Into<String>) -> Self {
        Self {
            field: Some(field),
            message: message.into(),
        }
    }
}

/// What a valid draft asks the controller for.
#[derive(Debug)]
pub(super) enum Submit {
    /// An edit that changed nothing.
    Unchanged,
    Update {
        id: EntityId,
        request: Box<UpdateNetworkRequest>,
    },
    Create(Box<CreateNetworkRequest>),
}

/// Every field's value as text, for telling what changed.
type Snapshot = Vec<(Field, String)>;

#[allow(clippy::struct_excessive_bools)]
pub(super) struct NetworkForm {
    pub(super) target: Target,
    /// The fields this network can take, in form order.
    fields: Vec<Field>,
    /// Index of the focused field in `fields`.
    focus: usize,
    name: LineInput,
    vlan: LineInput,
    gateway: LineInput,
    range_start: LineInput,
    range_stop: LineInput,
    dns: LineInput,
    enabled: bool,
    dhcp: bool,
    isolation: bool,
    internet: bool,
    mdns: bool,
    ipv6: bool,
    /// Whether this is the site's default network, whose VLAN is fixed.
    is_default: bool,
    /// The servers the network relays DHCP to, when it is a relay.
    relay: Vec<String>,
    original: Snapshot,
    /// Set while the controller has the save: the form takes no input.
    pub(super) saving: bool,
    pub(super) error: Option<FormError>,
}

/// The fields this network can take. The controller only keeps IPv4 and
/// DHCP settings and isolation for networks a gateway or switch manages,
/// and internet, mDNS, and IPv6 settings for gateway networks. A kind
/// this version does not know gets what every kind takes.
///
/// IPv6 shows only where it is on, because it can only be turned off
/// here: turning it on needs a prefix source and an address assignment
/// that a yes/no field cannot give.
fn fields_for(network: &Network) -> Vec<Field> {
    let (addressed, routed) = match network.management {
        Some(NetworkManagement::Gateway) => (true, true),
        Some(NetworkManagement::Switch) => (true, false),
        Some(NetworkManagement::Unmanaged) | None => (false, false),
    };
    let mut fields = vec![Field::Name, Field::Vlan];
    if addressed {
        fields.push(Field::Gateway);
    }
    fields.push(Field::Enabled);
    if addressed {
        fields.extend([
            Field::Dhcp,
            Field::RangeStart,
            Field::RangeStop,
            Field::Dns,
            Field::Isolation,
        ]);
    }
    if routed {
        fields.extend([Field::Internet, Field::Mdns]);
        if network.ipv6_enabled {
            fields.push(Field::Ipv6);
        }
    }
    fields
}

/// The gateway address with the subnet's prefix length, the form the
/// controller takes a subnet in.
fn gateway_with_prefix(network: &Network) -> String {
    let prefix = network
        .subnet
        .as_deref()
        .and_then(|subnet| subnet.split_once('/'))
        .map(|(_, prefix)| prefix);
    match (network.gateway_ip, prefix) {
        (Some(gateway), Some(prefix)) => format!("{gateway}/{prefix}"),
        _ => String::new(),
    }
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

impl NetworkForm {
    /// A form for editing `network`.
    pub(super) fn edit(network: &Network) -> Self {
        let address = |ip: Option<Ipv4Addr>| ip.map_or_else(String::new, |ip| ip.to_string());
        let dhcp = network.dhcp.as_ref();
        let mut form = Self {
            target: Target::Edit {
                id: network.id.clone(),
                name: network.name.clone(),
            },
            fields: fields_for(network),
            focus: 0,
            name: LineInput::new(network.name.clone()),
            vlan: LineInput::new(
                network
                    .vlan_id
                    .map_or_else(String::new, |vlan| vlan.to_string()),
            ),
            gateway: LineInput::new(gateway_with_prefix(network)),
            range_start: LineInput::new(address(dhcp.and_then(|dhcp| dhcp.range_start))),
            range_stop: LineInput::new(address(dhcp.and_then(|dhcp| dhcp.range_stop))),
            dns: LineInput::new(dhcp.map_or_else(String::new, |dhcp| {
                dhcp.dns_servers
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            })),
            enabled: network.enabled,
            dhcp: dhcp.is_some_and(|dhcp| dhcp.enabled),
            isolation: network.isolation_enabled,
            internet: network.internet_access_enabled,
            mdns: network.mdns_forwarding_enabled,
            ipv6: network.ipv6_enabled,
            is_default: network.is_default,
            relay: dhcp.map_or_else(Vec::new, |dhcp| {
                dhcp.relay_servers.iter().map(ToString::to_string).collect()
            }),
            original: Vec::new(),
            saving: false,
            error: None,
        };
        form.original = form.snapshot();
        form
    }

    /// An empty form for a new network: on, with internet access, and no
    /// DHCP server until a gateway address says where it would serve.
    pub(super) fn create() -> Self {
        let mut form = Self {
            target: Target::Create,
            fields: vec![
                Field::Name,
                Field::Vlan,
                Field::Gateway,
                Field::Dhcp,
                Field::RangeStart,
                Field::RangeStop,
                Field::Dns,
                Field::Isolation,
                Field::Internet,
            ],
            focus: 0,
            name: LineInput::default(),
            vlan: LineInput::default(),
            gateway: LineInput::default(),
            range_start: LineInput::default(),
            range_stop: LineInput::default(),
            dns: LineInput::default(),
            enabled: true,
            dhcp: false,
            isolation: false,
            internet: true,
            mdns: false,
            ipv6: false,
            is_default: false,
            relay: Vec::new(),
            original: Vec::new(),
            saving: false,
            error: None,
        };
        form.original = form.snapshot();
        form
    }

    pub(super) fn fields(&self) -> &[Field] {
        &self.fields
    }

    pub(super) fn focused(&self) -> Field {
        self.fields.get(self.focus).copied().unwrap_or(Field::Name)
    }

    /// Move the focus `delta` fields, wrapping at either end.
    pub(super) fn step_focus(&mut self, delta: isize) {
        let len = isize::try_from(self.fields.len()).unwrap_or(1).max(1);
        let focus = isize::try_from(self.focus).unwrap_or(0);
        self.focus = usize::try_from((focus + delta).rem_euclid(len)).unwrap_or(0);
    }

    /// Put the focus on `field`, when the form shows it.
    pub(super) fn focus_on(&mut self, field: Field) {
        if let Some(index) = self.fields.iter().position(|other| *other == field) {
            self.focus = index;
        }
    }

    pub(super) fn input(&self, field: Field) -> Option<&LineInput> {
        match field {
            Field::Name => Some(&self.name),
            Field::Vlan => Some(&self.vlan),
            Field::Gateway => Some(&self.gateway),
            Field::RangeStart => Some(&self.range_start),
            Field::RangeStop => Some(&self.range_stop),
            Field::Dns => Some(&self.dns),
            _ => None,
        }
    }

    fn input_mut(&mut self, field: Field) -> Option<&mut LineInput> {
        match field {
            Field::Name => Some(&mut self.name),
            Field::Vlan => Some(&mut self.vlan),
            Field::Gateway => Some(&mut self.gateway),
            Field::RangeStart => Some(&mut self.range_start),
            Field::RangeStop => Some(&mut self.range_stop),
            Field::Dns => Some(&mut self.dns),
            _ => None,
        }
    }

    /// A yes/no field's value.
    pub(super) fn flag(&self, field: Field) -> bool {
        match field {
            Field::Enabled => self.enabled,
            Field::Dhcp => self.dhcp,
            Field::Isolation => self.isolation,
            Field::Internet => self.internet,
            Field::Mdns => self.mdns,
            Field::Ipv6 => self.ipv6,
            _ => false,
        }
    }

    /// Flip the focused yes/no field.
    pub(super) fn toggle(&mut self) {
        let flag = match self.focused() {
            Field::Enabled => &mut self.enabled,
            Field::Dhcp => &mut self.dhcp,
            Field::Isolation => &mut self.isolation,
            Field::Internet => &mut self.internet,
            Field::Mdns => &mut self.mdns,
            Field::Ipv6 => &mut self.ipv6,
            _ => return,
        };
        *flag = !*flag;
        self.error = None;
    }

    /// Give an editing key to the focused text field. Returns whether the
    /// field took it.
    pub(super) fn edit_key(&mut self, key: KeyEvent) -> bool {
        let field = self.focused();
        let Some(input) = self.input_mut(field) else {
            return false;
        };
        let edit = input.handle_key_accepting(key, |ch| field.accepts(ch));
        if edit == Edit::Changed {
            self.error = None;
        }
        edit.consumed()
    }

    /// Put pasted text into the focused text field, keeping only what the
    /// field takes.
    pub(super) fn paste(&mut self, text: &str) {
        let field = self.focused();
        if let Some(input) = self.input_mut(field)
            && input.paste(text, |ch| field.accepts(ch)) == Edit::Changed
        {
            self.error = None;
        }
    }

    /// A field's value as it would be saved.
    fn value(&self, field: Field) -> String {
        match self.input(field) {
            Some(input) => input.text().trim().to_owned(),
            None => on_off(self.flag(field)).to_owned(),
        }
    }

    fn snapshot(&self) -> Snapshot {
        self.fields
            .iter()
            .map(|field| (*field, self.value(*field)))
            .collect()
    }

    pub(super) fn changed(&self, field: Field) -> bool {
        self.original
            .iter()
            .any(|(other, value)| *other == field && *value != self.value(field))
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.fields.iter().any(|field| self.changed(*field))
    }

    /// The changed fields as `Label: before → after` lines, for the
    /// confirmation.
    pub(super) fn changes(&self) -> Vec<String> {
        self.original
            .iter()
            .filter(|(field, _)| self.changed(*field))
            .map(|(field, before)| {
                let show = |value: &str| {
                    if value.is_empty() {
                        "none".to_owned()
                    } else {
                        value.to_owned()
                    }
                };
                format!(
                    "{}: {} \u{2192} {}",
                    field.noun(),
                    show(before),
                    show(&self.value(*field))
                )
            })
            .collect()
    }

    /// Where the network relays DHCP to, as `10.0.0.5` or `10.0.0.5 +1`.
    fn relay_target(&self) -> Option<String> {
        let (first, rest) = self.relay.split_first()?;
        Some(if rest.is_empty() {
            first.clone()
        } else {
            format!("{first} +{}", rest.len())
        })
    }

    /// What to say beside a yes/no field: what its position means now,
    /// or what saving it would delete.
    pub(super) fn note(&self, field: Field) -> Option<String> {
        match field {
            Field::Dhcp if self.dhcp => None,
            Field::Dhcp if self.changed(Field::Dhcp) => {
                Some("deletes its range and options".into())
            }
            Field::Dhcp => self
                .relay_target()
                .map(|servers| format!("relays to {servers}")),
            Field::Ipv6 if !self.ipv6 => Some("deletes its IPv6 settings".into()),
            _ => None,
        }
    }

    /// What saving costs beyond the listed changes, one line each. A
    /// draft with any of these asks before it saves: it deletes
    /// configuration, cuts devices off, or can drop their connection
    /// while it applies.
    pub(super) fn consequences(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.changed(Field::Dhcp) {
            if !self.dhcp {
                lines.push("The DHCP server's range, lease time, and options are deleted.".into());
            } else if let Some(servers) = self.relay_target() {
                lines.push(format!(
                    "DHCP is relayed to {servers} today. A server here replaces the relay."
                ));
            }
        }
        if self.changed(Field::Ipv6) && !self.ipv6 {
            lines.push(
                "Its IPv6 settings are deleted. Only the controller's own interface can set \
                 IPv6 up again."
                    .into(),
            );
        }
        if self.changed(Field::Internet) && !self.internet {
            lines.push("Devices on it lose internet access.".into());
        }
        if self.changed(Field::Isolation) && self.isolation {
            lines.push("Devices on it can no longer reach other networks.".into());
        }
        let readdressed = [
            Field::Vlan,
            Field::Gateway,
            Field::Dhcp,
            Field::RangeStart,
            Field::RangeStop,
            Field::Dns,
            Field::Ipv6,
        ]
        .into_iter()
        .any(|field| self.changed(field));
        if readdressed || (self.changed(Field::Enabled) && !self.enabled) {
            lines.push("Devices on it can lose their connection while it applies.".into());
        }
        lines
    }

    /// Validate the draft and build what to send. An error names the
    /// field to fix.
    pub(super) fn submit(&self) -> Result<Submit, FormError> {
        match &self.target {
            Target::Edit { id, .. } => {
                if !self.is_dirty() {
                    return Ok(Submit::Unchanged);
                }
                self.check()?;
                Ok(Submit::Update {
                    id: id.clone(),
                    request: Box::new(self.update_request()?),
                })
            }
            Target::Create => {
                self.check()?;
                Ok(Submit::Create(Box::new(self.create_request()?)))
            }
        }
    }

    fn update_request(&self) -> Result<UpdateNetworkRequest, FormError> {
        let text = |field: Field| self.changed(field).then(|| self.value(field));
        let flag = |field: Field| self.changed(field).then(|| self.flag(field));
        let dhcp_changed = [Field::RangeStart, Field::RangeStop, Field::Dns]
            .into_iter()
            .any(|field| self.changed(field));
        Ok(UpdateNetworkRequest {
            name: text(Field::Name),
            vlan_id: self
                .changed(Field::Vlan)
                .then(|| self.vlan_id())
                .transpose()?,
            subnet: text(Field::Gateway),
            dhcp_enabled: flag(Field::Dhcp),
            enabled: flag(Field::Enabled),
            isolation_enabled: flag(Field::Isolation),
            internet_access_enabled: flag(Field::Internet),
            mdns_forwarding_enabled: flag(Field::Mdns),
            ipv6_enabled: flag(Field::Ipv6),
            dhcp: if dhcp_changed {
                Some(DhcpUpdate {
                    range_start: text(Field::RangeStart),
                    range_stop: text(Field::RangeStop),
                    dns_servers: self
                        .changed(Field::Dns)
                        .then(|| self.dns_servers())
                        .transpose()?,
                })
            } else {
                None
            },
        })
    }

    fn create_request(&self) -> Result<CreateNetworkRequest, FormError> {
        let filled = |field: Field| Some(self.value(field)).filter(|value| !value.is_empty());
        let dns = self.dns_servers()?;
        Ok(CreateNetworkRequest {
            name: self.value(Field::Name),
            vlan_id: Some(self.vlan_id()?),
            subnet: filled(Field::Gateway),
            management: None,
            device_id: None,
            purpose: None,
            dhcp_enabled: self.dhcp,
            enabled: true,
            dhcp_range_start: filled(Field::RangeStart).filter(|_| self.dhcp),
            dhcp_range_stop: filled(Field::RangeStop).filter(|_| self.dhcp),
            dhcp_lease_time: None,
            dns_servers: Some(dns).filter(|dns| self.dhcp && !dns.is_empty()),
            firewall_zone_id: None,
            isolation_enabled: self.isolation,
            internet_access_enabled: self.internet,
        })
    }
}

mod check;
#[cfg(test)]
mod tests;
