pub mod format_profile;
use std::{env, fs, sync::Arc};

use anyhow::Result;
use arc_swap::{ArcSwap, Guard};
use format_profile::format_toml;
use log::{error, info};
use serde::Deserialize;

const DEFAULT_PROFILE: &str = "./op_charge.toml";

#[derive(Deserialize)]
pub struct Config {
    pub max_current: i32,
}

pub struct AtomicConfig {
    inner: ArcSwap<Config>,
    profile: String,
}

impl AtomicConfig {
    pub fn init() -> Result<Self> {
        let profile = profile_path();
        let raw_content = fs::read_to_string(&profile)?;
        let formatted_content = format_toml(&raw_content);
        let _ = fs::write(&profile, formatted_content);

        let config = toml::from_str(&raw_content)?;

        Ok(Self {
            inner: ArcSwap::from(Arc::new(config)),
            profile,
        })
    }

    pub fn get(&self) -> Guard<Arc<Config>> {
        self.inner.load()
    }

    pub fn reload(&self) {
        let raw_content = match fs::read_to_string(&self.profile) {
            Ok(raw_content) => raw_content,
            Err(e) => {
                error!(
                    "Failed to reload config: cannot read {}: {e}.",
                    self.profile
                );
                return;
            }
        };

        let new_config = match toml::from_str(&raw_content) {
            Ok(new_config) => new_config,
            Err(e) => {
                error!(
                    "Failed to reload config: cannot parse {}: {e}.",
                    self.profile
                );
                return;
            }
        };

        self.inner.store(Arc::new(new_config));
        info!("Config profile reloaded successfully.");
    }
}

pub fn profile_path() -> String {
    match env::args().nth(1) {
        Some(profile) => profile,
        None => DEFAULT_PROFILE.to_string(),
    }
}
