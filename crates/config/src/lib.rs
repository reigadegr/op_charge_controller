pub mod format_profile;

use std::{env, fs, sync::Arc};

use anyhow::Result;
use arc_swap::{ArcSwap, Guard};
use format_profile::format_toml;
use serde::Deserialize;
use tracing::{error, info, warn};

const DEFAULT_PROFILE: &str = "./op_charge.toml";

#[derive(Deserialize)]
pub struct Config {
    pub ufcs_max_vote: i32,
    pub ufcs_ramp_step_ma: u32,
    pub ufcs_taper_step_ma: u32,
    pub constant_voltage_mv: i32,
    pub charge_cutoff_mv: i32,
}

impl Config {
    fn validate(&self) -> Result<()> {
        if self.ufcs_max_vote <= 0 {
            anyhow::bail!("ufcs_max_vote 必须为正数");
        }
        if self.ufcs_ramp_step_ma == 0 {
            anyhow::bail!("ufcs_ramp_step_ma 必须为正数");
        }
        if self.ufcs_taper_step_ma == 0 {
            anyhow::bail!("ufcs_taper_step_ma 必须为正数");
        }
        if self.constant_voltage_mv <= 0 {
            anyhow::bail!("constant_voltage_mv 必须为正数");
        }
        if self.charge_cutoff_mv <= 0 {
            anyhow::bail!("charge_cutoff_mv 必须为正数");
        }
        if self.charge_cutoff_mv < self.constant_voltage_mv {
            anyhow::bail!("charge_cutoff_mv 不能小于 constant_voltage_mv");
        }

        Ok(())
    }
}

pub struct AtomicConfig {
    inner: ArcSwap<Config>,
    profile: String,
}

impl AtomicConfig {
    pub fn init() -> Result<Self> {
        Self::from_path(profile_path())
    }

    pub fn from_path(profile: impl Into<String>) -> Result<Self> {
        let profile = profile.into();
        let raw_content = fs::read_to_string(&profile)?;
        let config = parse_config(&raw_content)?;
        let formatted_content = format_toml(&raw_content);
        if formatted_content != raw_content
            && let Err(error) = fs::write(&profile, &formatted_content)
        {
            warn!("Failed to format config profile {profile}: {error}.");
        }

        Ok(Self {
            inner: ArcSwap::from(Arc::new(config)),
            profile,
        })
    }

    pub fn get(&self) -> Guard<Arc<Config>> {
        self.inner.load()
    }

    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
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

        let new_config = match parse_config(&raw_content) {
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

fn parse_config(content: &str) -> Result<Config> {
    let config: Config = toml::from_str(content)?;
    config.validate()?;

    Ok(config)
}

#[must_use]
pub fn profile_path() -> String {
    match env::args().nth(1) {
        Some(profile) => profile,
        None => DEFAULT_PROFILE.to_string(),
    }
}
