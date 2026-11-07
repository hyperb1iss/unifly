use unifly_api::{Controller, TrafficFilterSpec};

use super::PolicyFilterInput;
use crate::cli::commands::util::lookup;
use crate::cli::error::CliError;

impl PolicyFilterInput {
    pub(in crate::cli::commands::firewall) fn resolve_network_refs(
        &mut self,
        controller: &Controller,
    ) -> Result<(), CliError> {
        let networks = controller.networks_snapshot();
        for filter in [&mut self.source_filter, &mut self.destination_filter]
            .into_iter()
            .flatten()
        {
            if let TrafficFilterSpec::Network { network_ids, .. } = filter {
                for identifier in network_ids {
                    *identifier = lookup::resolve(&networks, identifier)?.id.to_string();
                }
            }
        }
        Ok(())
    }
}
