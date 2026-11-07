use unifly_api::{DhcpUpdate, EntityId, Network, UpdateNetworkRequest};

/// Editable fields for a network. Initialized from the selected `Network`.
#[allow(clippy::struct_excessive_bools)]
pub(super) struct NetworkEditState {
    /// The network being edited; the save goes to this ID.
    pub(super) network_id: EntityId,
    original: [String; Self::FIELD_COUNT],
    range_start: String,
    range_stop: String,
    name: String,
    vlan_id: String,
    dhcp_enabled: bool,
    isolation_enabled: bool,
    internet_access_enabled: bool,
    mdns_forwarding_enabled: bool,
    ipv6_enabled: bool,
    enabled: bool,
    /// Which field is currently focused (0-indexed).
    pub(super) field_idx: usize,
}

impl NetworkEditState {
    pub(super) const FIELD_COUNT: usize = 10;

    pub(super) fn from_network(network: &Network) -> Self {
        let mut edit = Self {
            original: std::array::from_fn(|_| String::new()),
            range_start: network
                .dhcp
                .as_ref()
                .and_then(|dhcp| dhcp.range_start)
                .map_or_else(String::new, |ip| ip.to_string()),
            range_stop: network
                .dhcp
                .as_ref()
                .and_then(|dhcp| dhcp.range_stop)
                .map_or_else(String::new, |ip| ip.to_string()),
            network_id: network.id.clone(),
            name: network.name.clone(),
            vlan_id: network
                .vlan_id
                .map_or_else(String::new, |value| value.to_string()),
            dhcp_enabled: network.dhcp.as_ref().is_some_and(|dhcp| dhcp.enabled),
            isolation_enabled: network.isolation_enabled,
            internet_access_enabled: network.internet_access_enabled,
            mdns_forwarding_enabled: network.mdns_forwarding_enabled,
            ipv6_enabled: network.ipv6_enabled,
            enabled: network.enabled,
            field_idx: 0,
        };
        edit.original = std::array::from_fn(|index| edit.field_value(index));
        edit
    }

    pub(super) fn field_label(index: usize) -> &'static str {
        match index {
            0 => "Name",
            1 => "VLAN ID",
            2 => "Enabled",
            3 => "DHCP",
            4 => "Isolation",
            5 => "Internet",
            6 => "mDNS Fwd",
            7 => "IPv6",
            8 => "DHCP start",
            9 => "DHCP stop",
            _ => "",
        }
    }

    pub(super) fn field_value(&self, index: usize) -> String {
        match index {
            0 => self.name.clone(),
            1 => self.vlan_id.clone(),
            2 => bool_label(self.enabled),
            3 => bool_label(self.dhcp_enabled),
            4 => bool_label(self.isolation_enabled),
            5 => bool_label(self.internet_access_enabled),
            6 => bool_label(self.mdns_forwarding_enabled),
            7 => bool_label(self.ipv6_enabled),
            8 => self.range_start.clone(),
            9 => self.range_stop.clone(),
            _ => String::new(),
        }
    }

    pub(super) fn is_bool_field(index: usize) -> bool {
        (2..=7).contains(&index)
    }

    pub(super) fn toggle_bool(&mut self) {
        match self.field_idx {
            2 => self.enabled = !self.enabled,
            3 => self.dhcp_enabled = !self.dhcp_enabled,
            4 => self.isolation_enabled = !self.isolation_enabled,
            5 => self.internet_access_enabled = !self.internet_access_enabled,
            6 => self.mdns_forwarding_enabled = !self.mdns_forwarding_enabled,
            7 => self.ipv6_enabled = !self.ipv6_enabled,
            _ => {}
        }
    }

    pub(super) fn handle_text_input(&mut self, ch: char) {
        match self.field_idx {
            0 => self.name.push(ch),
            1 if ch.is_ascii_digit() => self.vlan_id.push(ch),
            8 if ch.is_ascii_digit() || ch == '.' => self.range_start.push(ch),
            9 if ch.is_ascii_digit() || ch == '.' => self.range_stop.push(ch),
            _ => {}
        }
    }

    pub(super) fn handle_backspace(&mut self) {
        match self.field_idx {
            0 => {
                self.name.pop();
            }
            1 => {
                self.vlan_id.pop();
            }
            8 => {
                self.range_start.pop();
            }
            9 => {
                self.range_stop.pop();
            }
            _ => {}
        }
    }

    fn changed(&self, index: usize) -> bool {
        self.original
            .get(index)
            .is_some_and(|original| *original != self.field_value(index))
    }

    pub(super) fn build_request(&self) -> Result<Option<UpdateNetworkRequest>, String> {
        if !(0..Self::FIELD_COUNT).any(|index| self.changed(index)) {
            return Ok(None);
        }
        if self.changed(0) && self.name.trim().is_empty() {
            return Err("Network name cannot be empty".into());
        }
        let vlan_id = if self.changed(1) {
            Some(
                self.vlan_id
                    .parse::<u16>()
                    .ok()
                    .filter(|vlan| (1..=4094).contains(vlan))
                    .ok_or("VLAN ID must be between 1 and 4094")?,
            )
        } else {
            None
        };
        let dhcp = self.dhcp_update()?;
        Ok(Some(UpdateNetworkRequest {
            name: self.changed(0).then(|| self.name.clone()),
            vlan_id,
            enabled: self.changed(2).then_some(self.enabled),
            dhcp_enabled: self.changed(3).then_some(self.dhcp_enabled),
            isolation_enabled: self.changed(4).then_some(self.isolation_enabled),
            internet_access_enabled: self.changed(5).then_some(self.internet_access_enabled),
            mdns_forwarding_enabled: self.changed(6).then_some(self.mdns_forwarding_enabled),
            ipv6_enabled: self.changed(7).then_some(self.ipv6_enabled),
            subnet: None,
            dhcp,
        }))
    }

    fn dhcp_update(&self) -> Result<Option<DhcpUpdate>, String> {
        let range_changed = self.changed(8) || self.changed(9);
        if range_changed || (self.changed(3) && self.dhcp_enabled) {
            let start = self
                .range_start
                .parse::<std::net::Ipv4Addr>()
                .map_err(|_| "DHCP start must be an IPv4 address")?;
            let stop = self
                .range_stop
                .parse::<std::net::Ipv4Addr>()
                .map_err(|_| "DHCP stop must be an IPv4 address")?;
            if start > stop {
                return Err("DHCP start must not be after DHCP stop".into());
            }
        }
        Ok(range_changed.then(|| DhcpUpdate {
            range_start: self.changed(8).then(|| self.range_start.clone()),
            range_stop: self.changed(9).then(|| self.range_stop.clone()),
            ..DhcpUpdate::default()
        }))
    }
}

fn bool_label(value: bool) -> String {
    if value {
        "Enabled".into()
    } else {
        "Disabled".into()
    }
}
