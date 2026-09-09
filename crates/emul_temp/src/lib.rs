use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use anyhow::{Context, Result};
use tracing::{error, info, warn};

const THERMAL_ZONES_DIR: &str = "/sys/class/thermal";
const SHELL_TEMP_PATH: &str = "/proc/shell-temp";
const FAKE_BATT_TEMP: i32 = 36000;
const SHELL_TEMP_INDEX_MAX: u32 = 2;

pub const RESET_TARGET: i32 = 0;

pub fn apply(target: i32) -> Result<()> {
    set_thermal_zone_emul_temp(Path::new(THERMAL_ZONES_DIR), target);
    set_fake_batt_temp(Path::new(SHELL_TEMP_PATH))
}

pub fn set_thermal_zone_emul_temp(thermal_dir: &Path, target: i32) {
    let entries = match fs::read_dir(thermal_dir) {
        Ok(entries) => entries,
        Err(error) => {
            warn!("无法枚举 thermal zone {}: {error}", thermal_dir.display());
            return;
        }
    };

    for entry in entries.flatten() {
        let zone = entry.path();
        let is_thermal_zone = zone
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("thermal_zone"));
        if !is_thermal_zone || !zone.is_dir() {
            continue;
        }

        let zone_type = match fs::read_to_string(zone.join("type")) {
            Ok(zone_type) => zone_type.trim().to_owned(),
            Err(_) => String::new(),
        };
        let emul_temp = zone.join("emul_temp");
        if !emul_temp.exists() {
            info!(
                zone = %zone.display(),
                zone_type,
                "跳过 thermal zone：缺少 emul_temp"
            );
            continue;
        }

        if let Err(error) = fs::write(&emul_temp, format!("{target}\n")) {
            error!(zone = %zone.display(), "写入 emul_temp 失败: {error}");
            continue;
        }
        info!(
            zone = %zone.display(),
            zone_type,
            target,
            "emul_temp 已设置"
        );
    }
}

pub fn set_fake_batt_temp(shell_temp: &Path) -> Result<()> {
    fs::set_permissions(shell_temp, fs::Permissions::from_mode(0o644))
        .with_context(|| format!("设置 {} 为可写失败", shell_temp.display()))?;

    // 尝试写入所有索引，最后统一恢复只读权限。
    let mut result = Ok(());
    for payload in fake_batt_temp_payloads() {
        let write_result = fs::write(shell_temp, format!("{payload}\n"))
            .with_context(|| format!("写入 {} ({payload}) 失败", shell_temp.display()));
        result = result.and(write_result);
    }
    fs::set_permissions(shell_temp, fs::Permissions::from_mode(0o444))
        .with_context(|| format!("恢复 {} 只读权限失败", shell_temp.display()))?;
    result
}

fn fake_batt_temp_payloads() -> impl Iterator<Item = String> {
    (0..=SHELL_TEMP_INDEX_MAX).map(|index| format!("{index} {FAKE_BATT_TEMP}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_batt_temp_payloads_cover_every_index() {
        let payloads: Vec<String> = fake_batt_temp_payloads().collect();
        assert_eq!(payloads, ["0 36000", "1 36000", "2 36000"]);
    }
}
