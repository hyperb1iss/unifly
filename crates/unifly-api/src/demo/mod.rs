//! Synthetic, offline demo fleet.
//!
//! [`Controller::demo`](crate::Controller::demo) builds a controller that
//! never touches the network. On connect it seeds the [`DataStore`] with a
//! fictional creative studio ("Neon Harbor Studios": a gateway, three
//! switches, nine access points, ~140 clients, six VLANs, four SSIDs, and a
//! zone-based firewall) and starts a once-a-second simulation that animates
//! WAN throughput, device telemetry, client traffic, roams, arrivals and
//! departures, and security events.
//!
//! Everything is derived from a seed: the same seed reproduces the same
//! fleet and the same animation sequence, so screenshots and recordings can
//! be reshot identically. All addresses come from documentation or private
//! ranges and every MAC is locally administered.
//!
//! [`DataStore`]: crate::DataStore

mod catalog;
mod commands;
mod diagnostics;
pub(crate) mod fleet;
mod ports;
mod queries;
mod rng;
pub(crate) mod sim;
#[cfg(test)]
mod tests;

use std::sync::{Mutex, PoisonError};

use chrono::Utc;

use fleet::Fleet;
use sim::Simulation;

/// Seed used when none is given. Any `u64` works; this one is just the
/// fleet the docs and screenshots show.
pub const DEFAULT_DEMO_SEED: u64 = 2077;

/// Options for [`Controller::demo`](crate::Controller::demo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DemoFleetOptions {
    /// Seed for the fleet layout and the simulation.
    pub seed: u64,
}

impl DemoFleetOptions {
    /// Options for the fleet generated from `seed`.
    pub fn with_seed(seed: u64) -> Self {
        Self { seed }
    }
}

impl Default for DemoFleetOptions {
    fn default() -> Self {
        Self::with_seed(DEFAULT_DEMO_SEED)
    }
}

/// Demo state carried by a demo-mode controller.
pub(crate) struct DemoRuntime {
    seed: u64,
    sim: Mutex<Simulation>,
}

impl DemoRuntime {
    pub(crate) fn new(options: DemoFleetOptions) -> Self {
        let fleet = Fleet::generate(options.seed, Utc::now());
        Self {
            seed: options.seed,
            sim: Mutex::new(Simulation::new(fleet)),
        }
    }

    pub(crate) fn seed(&self) -> u64 {
        self.seed
    }

    /// Run `f` with exclusive access to the simulation. Never held across
    /// an await.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut Simulation) -> R) -> R {
        let mut guard = self.sim.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut guard)
    }
}
