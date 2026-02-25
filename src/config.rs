// SPDX-License-Identifier: GPL-3.0

use cosmic::cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub enum AuthMethod {
    #[default]
    GhCli,
    Pat,
}

#[derive(Debug, Clone, CosmicConfigEntry, Eq, PartialEq)]
#[version = 2]
pub struct Config {
    pub auth_method: AuthMethod,
    pub github_pat: String,
    pub poll_interval_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auth_method: AuthMethod::GhCli,
            github_pat: String::new(),
            poll_interval_secs: 60,
        }
    }
}
