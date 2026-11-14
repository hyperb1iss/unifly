//! Guest and blocked state for Integration clients.
//!
//! The Integration client list says whether a client is a guest but not
//! whether it is blocked, and its records carry no state at all on
//! controllers that answer with an empty `access` object. The Session
//! station list and user records hold both, keyed by MAC.

use std::collections::HashMap;

use crate::model::{Client, GuestAuth};
use crate::session::models::{SessionClientEntry, SessionUserEntry};

pub(super) fn merge(
    clients: &mut [Client],
    session_clients: &[SessionClientEntry],
    session_users: &[SessionUserEntry],
) {
    if session_clients.is_empty() && session_users.is_empty() {
        return;
    }
    let stations: HashMap<String, &SessionClientEntry> = session_clients
        .iter()
        .map(|station| (station.mac.to_lowercase(), station))
        .collect();
    let users: HashMap<String, &SessionUserEntry> = session_users
        .iter()
        .map(|user| (user.mac.to_lowercase(), user))
        .collect();

    for client in clients {
        let mac = client.mac.as_str().to_lowercase();
        let station = stations.get(&mac);
        if station.is_some_and(|station| station.is_guest == Some(true)) {
            client.is_guest = true;
        }
        if client.is_guest {
            let auth = client.guest_auth.get_or_insert(GuestAuth {
                authorized: false,
                method: None,
                expires_at: None,
                tx_bytes: None,
                rx_bytes: None,
                elapsed_minutes: None,
            });
            if let Some(authorized) = station.and_then(|station| station.authorized) {
                auth.authorized = authorized;
            }
        }
        let blocked = station.and_then(|station| station.blocked) == Some(true)
            || users.get(&mac).and_then(|user| user.blocked) == Some(true);
        client.blocked |= blocked;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::merge;
    use crate::integration_types::ClientResponse;
    use crate::model::Client;
    use crate::session::models::{SessionClientEntry, SessionUserEntry};

    const MAC: &str = "aa:bb:cc:dd:ee:ff";

    fn integration_client(access: &serde_json::Value) -> Client {
        serde_json::from_value::<ClientResponse>(json!({
            "id": "c56a4180-65aa-42ec-a945-5fd21dec0538",
            "name": "Visitor phone",
            "type": "WIRELESS",
            // The station list and Integration disagree on the address,
            // as they do across a lease change: the MAC is the key.
            "ipAddress": "10.0.60.21",
            "macAddress": MAC,
            "access": access,
        }))
        .expect("deserialize")
        .into()
    }

    fn station(fields: &serde_json::Value) -> SessionClientEntry {
        let mut entry = json!({ "_id": "sta-1", "mac": "AA:BB:CC:DD:EE:FF", "ip": "10.0.60.99" });
        entry
            .as_object_mut()
            .expect("an object")
            .extend(fields.as_object().expect("an object").clone());
        serde_json::from_value(entry).expect("deserialize")
    }

    fn user(blocked: bool) -> SessionUserEntry {
        serde_json::from_value(json!({ "_id": "user-1", "mac": MAC, "blocked": blocked }))
            .expect("deserialize")
    }

    #[test]
    fn a_guest_and_its_authorization_come_from_the_station_by_mac() {
        let mut clients = vec![integration_client(&json!({}))];
        merge(
            &mut clients,
            &[station(&json!({ "is_guest": true, "authorized": true }))],
            &[],
        );
        assert!(clients[0].is_guest);
        assert!(
            clients[0]
                .guest_auth
                .as_ref()
                .is_some_and(|auth| auth.authorized)
        );
    }

    #[test]
    fn the_station_list_updates_an_integration_guests_authorization() {
        let mut clients = vec![integration_client(
            &json!({ "type": "GUEST", "authorized": false }),
        )];
        merge(
            &mut clients,
            &[station(&json!({ "is_guest": true, "authorized": true }))],
            &[],
        );
        assert!(
            clients[0]
                .guest_auth
                .as_ref()
                .is_some_and(|auth| auth.authorized)
        );

        // A station row that does not say keeps what Integration reported.
        let mut clients = vec![integration_client(
            &json!({ "type": "GUEST", "authorized": true }),
        )];
        merge(&mut clients, &[station(&json!({}))], &[]);
        assert!(clients[0].is_guest);
        assert!(
            clients[0]
                .guest_auth
                .as_ref()
                .is_some_and(|auth| auth.authorized)
        );
    }

    #[test]
    fn blocked_comes_from_the_station_or_the_user_record() {
        let mut by_station = vec![integration_client(&json!({ "type": "DEFAULT" }))];
        merge(
            &mut by_station,
            &[station(&json!({ "blocked": true }))],
            &[],
        );
        assert!(by_station[0].blocked);
        assert!(!by_station[0].is_guest);

        let mut by_user = vec![integration_client(&json!({ "type": "DEFAULT" }))];
        merge(&mut by_user, &[], &[user(true)]);
        assert!(by_user[0].blocked);

        let mut neither = vec![integration_client(&json!({ "type": "DEFAULT" }))];
        merge(&mut neither, &[station(&json!({}))], &[user(false)]);
        assert!(!neither[0].blocked);
    }
}
