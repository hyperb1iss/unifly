use std::collections::HashMap;
use std::sync::Arc;

use unifly_api::{Client, ClientType, MacAddress};

/// AP associations from one client snapshot, shared by sorting and rendering.
#[derive(Default)]
pub(super) struct ApClients(HashMap<MacAddress, Vec<Arc<Client>>>);

impl ApClients {
    pub(super) fn new(clients: &[Arc<Client>]) -> Self {
        let mut index: HashMap<MacAddress, Vec<Arc<Client>>> = HashMap::new();
        for client in clients
            .iter()
            .filter(|client| client.client_type == ClientType::Wireless)
        {
            let uplink = client.uplink_device_mac.as_ref();
            let bssid = client
                .wireless
                .as_ref()
                .and_then(|wireless| wireless.bssid.as_ref());
            for mac in uplink
                .into_iter()
                .chain(bssid.filter(|mac| Some(*mac) != uplink))
            {
                index
                    .entry(mac.clone())
                    .or_default()
                    .push(Arc::clone(client));
            }
        }
        Self(index)
    }

    pub(super) fn get(&self, mac: &MacAddress) -> &[Arc<Client>] {
        self.0.get(mac).map_or(&[], Vec::as_slice)
    }
}
