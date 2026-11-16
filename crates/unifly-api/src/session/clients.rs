// Session API client (station) endpoints
//
// Client management via stat/sta (read) and cmd/stamgr (commands).
// Covers listing, blocking, kicking, forgetting, and guest authorization.

use serde_json::json;
use tracing::debug;

use crate::error::Error;
use crate::session::client::SessionClient;
use crate::session::models::{SessionClientEntry, SessionUserEntry};

impl SessionClient {
    /// List all currently connected clients (stations).
    ///
    /// `GET /api/s/{site}/stat/sta`
    pub async fn list_clients(&self) -> Result<Vec<SessionClientEntry>, Error> {
        let url = self.site_url("stat/sta");
        debug!("listing connected clients");
        self.get(url).await
    }

    /// Block a client by MAC address.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "block-sta", "mac": "..."}`
    pub async fn block_client(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "blocking client");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "block-sta",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Unblock a client by MAC address.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "unblock-sta", "mac": "..."}`
    pub async fn unblock_client(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "unblocking client");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "unblock-sta",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Disconnect (kick) a client.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "kick-sta", "mac": "..."}`
    pub async fn kick_client(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "kicking client");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "kick-sta",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    /// Forget (permanently remove) a client by MAC address.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "forget-sta", "macs": [...]}`
    pub async fn forget_client(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "forgetting client");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "forget-sta",
                    "macs": [mac],
                }),
            )
            .await?;
        Ok(())
    }

    /// Authorize a guest client on the hotspot portal.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with guest authorization parameters.
    ///
    /// - `mac`: Client MAC address
    /// - `minutes`: Authorization duration in minutes
    /// - `up_kbps`: Optional upload bandwidth limit (Kbps)
    /// - `down_kbps`: Optional download bandwidth limit (Kbps)
    /// - `quota_mb`: Optional data transfer quota (MB)
    pub async fn authorize_guest(
        &self,
        mac: &str,
        minutes: u32,
        up_kbps: Option<u32>,
        down_kbps: Option<u32>,
        quota_mb: Option<u32>,
    ) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, minutes, "authorizing guest");

        let mut body = json!({
            "cmd": "authorize-guest",
            "mac": mac,
            "minutes": minutes,
        });

        let obj = body
            .as_object_mut()
            .expect("json! macro always produces an object");
        if let Some(up) = up_kbps {
            obj.insert("up".into(), json!(up));
        }
        if let Some(down) = down_kbps {
            obj.insert("down".into(), json!(down));
        }
        if let Some(quota) = quota_mb {
            obj.insert("bytes".into(), json!(quota));
        }

        let _: Vec<serde_json::Value> = self.post(url, &body).await?;
        Ok(())
    }

    /// Revoke guest authorization for a client.
    ///
    /// `POST /api/s/{site}/cmd/stamgr` with `{"cmd": "unauthorize-guest", "mac": "..."}`
    pub async fn unauthorize_guest(&self, mac: &str) -> Result<(), Error> {
        let url = self.site_url("cmd/stamgr");
        debug!(mac, "revoking guest authorization");
        let _: Vec<serde_json::Value> = self
            .post(
                url,
                &json!({
                    "cmd": "unauthorize-guest",
                    "mac": mac,
                }),
            )
            .await?;
        Ok(())
    }

    // ── DHCP reservation (rest/user) ──────────────────────────────

    /// List all known users (includes offline clients with reservations).
    ///
    /// `GET /api/s/{site}/rest/user`
    pub async fn list_users(&self) -> Result<Vec<SessionUserEntry>, Error> {
        let url = self.site_url("rest/user");
        debug!("listing known users");
        self.get(url).await
    }

    /// Set a fixed IP (DHCP reservation) for a client.
    ///
    /// `network_id` is the `rest/networkconf` `_id` of the network the
    /// address belongs to, never an Integration UUID.
    ///
    /// Looks up the client in `rest/user` by MAC. If already known, PUTs an
    /// update; otherwise POSTs a new user entry. A client that already
    /// holds this reservation is left alone.
    pub async fn set_client_fixed_ip(
        &self,
        mac: &str,
        ip: &str,
        network_id: &str,
    ) -> Result<(), Error> {
        debug!(mac, ip, network_id, "setting client fixed IP");

        let users = self.list_users().await?;
        let normalized_mac = mac.to_lowercase();
        let existing = users
            .iter()
            .find(|u| u.mac.to_lowercase() == normalized_mac);

        if let Some(user) = existing {
            if user.use_fixedip == Some(true)
                && user.fixed_ip.as_deref() == Some(ip)
                && user.network_id.as_deref() == Some(network_id)
            {
                debug!(mac, "client already holds this reservation");
                return Ok(());
            }
            let url = self.site_url(&format!("rest/user/{}", user.id));
            let _: Vec<serde_json::Value> = self
                .put(
                    url,
                    &json!({
                        "use_fixedip": true,
                        "fixed_ip": ip,
                        "network_id": network_id,
                    }),
                )
                .await?;
        } else {
            let url = self.site_url("rest/user");
            let _: Vec<serde_json::Value> = self
                .post(
                    url,
                    &json!({
                        "mac": normalized_mac,
                        "use_fixedip": true,
                        "fixed_ip": ip,
                        "network_id": network_id,
                    }),
                )
                .await?;
        }
        Ok(())
    }

    /// Remove a fixed IP (DHCP reservation) from a client.
    ///
    /// If `network_id` is provided, only the reservation on that network
    /// is removed. Otherwise all reservations for the MAC are cleared.
    pub async fn remove_client_fixed_ip(
        &self,
        mac: &str,
        network_id: Option<&str>,
    ) -> Result<(), Error> {
        debug!(mac, ?network_id, "removing client fixed IP");

        let users = self.list_users().await?;
        let normalized_mac = mac.to_lowercase();
        let matches: Vec<&SessionUserEntry> = users
            .iter()
            .filter(|u| {
                u.mac.to_lowercase() == normalized_mac
                    && network_id.is_none_or(|nid| u.network_id.as_deref() == Some(nid))
            })
            .collect();

        if matches.is_empty() {
            return Err(Error::SessionApi {
                message: if let Some(nid) = network_id {
                    format!("no reservation for MAC {mac} on network {nid}")
                } else {
                    format!("no known user with MAC {mac}")
                },
            });
        }

        for user in matches {
            let url = self.site_url(&format!("rest/user/{}", user.id));
            let _: Vec<serde_json::Value> = self
                .put(
                    url,
                    &json!({
                        "use_fixedip": false,
                    }),
                )
                .await?;
        }
        Ok(())
    }
}
