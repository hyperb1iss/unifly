//! Flipping a firewall rule's switch (enabled, or a policy's logging) and
//! checking afterwards that the controller did.

use tracing::warn;

use unifly_api::{
    Command, Controller, EntityId, UpdateAclRuleRequest, UpdateFirewallPolicyRequest,
    UpdateNatPolicyRequest,
};

use super::App;
use crate::tui::action::{Notification, RuleSwitch};

/// The rule's name and the switch's position, as the store has them.
fn read(controller: &Controller, id: &EntityId, switch: RuleSwitch) -> Option<(String, bool)> {
    match switch {
        RuleSwitch::PolicyEnabled | RuleSwitch::PolicyLogging => controller
            .firewall_policies_snapshot()
            .iter()
            .find(|policy| &policy.id == id)
            .map(|policy| {
                let on = if switch == RuleSwitch::PolicyLogging {
                    policy.logging_enabled
                } else {
                    policy.enabled
                };
                (policy.name.clone(), on)
            }),
        RuleSwitch::AclEnabled => controller
            .acl_rules_snapshot()
            .iter()
            .find(|rule| &rule.id == id)
            .map(|rule| (rule.name.clone(), rule.enabled)),
        RuleSwitch::NatEnabled => controller
            .nat_policies_snapshot()
            .iter()
            .find(|rule| &rule.id == id)
            .map(|rule| (rule.name.clone(), rule.enabled)),
    }
}

/// The write that sets the switch.
///
/// Whether a policy is enabled goes through the full update, where
/// `enabled` is a field of the request. The controller's PATCH takes only
/// logging and flow reporting, which is what logging uses: it reaches the
/// built-in policies too.
fn command(id: &EntityId, switch: RuleSwitch, on: bool) -> Command {
    let id = id.clone();
    match switch {
        RuleSwitch::PolicyEnabled => Command::UpdateFirewallPolicy {
            id,
            update: UpdateFirewallPolicyRequest {
                enabled: Some(on),
                ..UpdateFirewallPolicyRequest::default()
            },
        },
        RuleSwitch::PolicyLogging => Command::PatchFirewallPolicy {
            id,
            enabled: None,
            logging: Some(on),
            report_flow_enabled: None,
        },
        RuleSwitch::AclEnabled => Command::UpdateAclRule {
            id,
            update: UpdateAclRuleRequest {
                enabled: Some(on),
                ..UpdateAclRuleRequest::default()
            },
        },
        RuleSwitch::NatEnabled => Command::UpdateNatPolicy {
            id,
            update: UpdateNatPolicyRequest {
                enabled: Some(on),
                ..UpdateNatPolicyRequest::default()
            },
        },
    }
}

/// What the toasts say for one flip of a switch.
struct Wording {
    pending: String,
    done: String,
    /// The controller accepted the write and the switch did not move.
    unchanged: String,
}

fn wording(switch: RuleSwitch, on: bool, name: &str) -> Wording {
    if switch == RuleSwitch::PolicyLogging {
        let (to, still) = if on { ("on", "off") } else { ("off", "on") };
        Wording {
            pending: format!("Turning logging {to} for {name}\u{2026}"),
            done: format!("Logging is {to} for {name}"),
            unchanged: format!(
                "The controller accepted the change, but logging is still {still} for {name}"
            ),
        }
    } else {
        let (doing, done, still) = if on {
            ("Enabling", "Enabled", "disabled")
        } else {
            ("Disabling", "Disabled", "enabled")
        };
        Wording {
            pending: format!("{doing} {name}\u{2026}"),
            done: format!("{done} {name}"),
            unchanged: format!("The controller accepted the change, but {name} is still {still}"),
        }
    }
}

impl App {
    /// Set a firewall rule's switch on the controller, fetch its data
    /// again, and report what the rule then is. A controller can accept a
    /// write and not apply it, and "Disabled X" over a rule that still
    /// matches traffic is the one thing this must never say.
    pub(super) fn set_rule_switch(&mut self, id: &EntityId, switch: RuleSwitch, on: bool) {
        let Some(controller) = self.controller.clone() else {
            self.show_notification(Notification::error("Not connected"));
            return;
        };
        let Some((name, _)) = read(&controller, id, switch) else {
            self.report_gone("That rule");
            return;
        };
        let name = self.shown_name(&name);
        let wording = wording(switch, on, &name);
        let command = command(id, switch, on);
        let id = id.clone();
        self.spawn_op(&wording.pending.clone(), async move {
            if let Err(error) = controller.execute(command).await {
                warn!(%error, "rule switch failed");
                return Notification::error(format!("{error}"));
            }
            if let Err(error) = controller.full_refresh().await {
                warn!(%error, "refresh after a rule switch failed");
                return Notification::warning(format!(
                    "The change to {name} was sent, but the result could not be read back: \
                     {error}"
                ));
            }
            match read(&controller, &id, switch) {
                Some((_, now)) if now == on => Notification::success(wording.done),
                Some(_) => Notification::error(wording.unchanged),
                None => Notification::warning(format!("{name} is no longer on the controller")),
            }
        });
    }
}

#[cfg(test)]
mod tests;
