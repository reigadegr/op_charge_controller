use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

use config::AtomicConfig;

fn profile_path(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "op_charge_controller_{}_{}_toml",
        process::id(),
        name
    ))
}

fn valid_profile(max_vote: i32) -> String {
    format!(
        "\
ufcs_max_vote={max_vote}
ufcs_ramp_step_ma=300
ufcs_taper_step_ma=100
constant_voltage_mv=4500
charge_cutoff_mv=4570
"
    )
}

fn write_profile(path: &Path, content: &str) -> std::io::Result<()> {
    fs::write(path, content)
}

#[test]
fn atomic_config_reloads_valid_profiles_and_keeps_the_last_valid_one() -> anyhow::Result<()> {
    let path = profile_path("reload");
    let _ = fs::remove_file(&path);
    write_profile(&path, &valid_profile(6200))?;

    let config = AtomicConfig::from_path(path.to_string_lossy())?;
    assert_eq!(config.profile(), path.to_string_lossy().as_ref());
    assert_eq!(config.get().ufcs_max_vote, 6200);
    assert!(config.get().shell_back_emul_temp_enabled);
    assert!(fs::read_to_string(&path)?.contains("ufcs_max_vote = 6200"));

    write_profile(&path, &valid_profile(5000))?;
    config.reload();
    assert_eq!(config.get().ufcs_max_vote, 5000);

    write_profile(&path, &valid_profile(-1))?;
    config.reload();
    assert_eq!(config.get().ufcs_max_vote, 5000);

    fs::remove_file(&path)?;

    Ok(())
}

#[test]
fn shell_back_emul_temp_is_enabled_by_default_and_can_be_disabled() -> anyhow::Result<()> {
    let path = profile_path("emul_temp");
    let _ = fs::remove_file(&path);

    write_profile(&path, &valid_profile(6200))?;
    let config = AtomicConfig::from_path(path.to_string_lossy())?;
    assert!(config.get().shell_back_emul_temp_enabled);

    write_profile(
        &path,
        "ufcs_max_vote=6200\nufcs_ramp_step_ma=300\nufcs_taper_step_ma=100\nconstant_voltage_mv=4500\ncharge_cutoff_mv=4570\nshell_back_emul_temp_enabled=false\n",
    )?;
    config.reload();
    assert!(!config.get().shell_back_emul_temp_enabled);

    fs::remove_file(&path)?;

    Ok(())
}

#[test]
fn invalid_profiles_are_rejected_before_formatting() -> anyhow::Result<()> {
    let invalid_profiles = [
        (
            "invalid_ramp_step",
            "ufcs_max_vote=6200\nufcs_ramp_step_ma=0\nufcs_taper_step_ma=100\nconstant_voltage_mv=4500\ncharge_cutoff_mv=4570\n",
        ),
        (
            "invalid_taper_step",
            "ufcs_max_vote=6200\nufcs_ramp_step_ma=300\nufcs_taper_step_ma=0\nconstant_voltage_mv=4500\ncharge_cutoff_mv=4570\n",
        ),
        (
            "invalid_cutoff",
            "ufcs_max_vote=6200\nufcs_ramp_step_ma=300\nufcs_taper_step_ma=100\nconstant_voltage_mv=4570\ncharge_cutoff_mv=4500\n",
        ),
        ("malformed", "ufcs_max_vote = 6200\n"),
    ];

    for (name, invalid) in invalid_profiles {
        let path = profile_path(name);
        let _ = fs::remove_file(&path);
        write_profile(&path, invalid)?;

        assert!(AtomicConfig::from_path(path.to_string_lossy()).is_err());
        assert_eq!(fs::read_to_string(&path)?, invalid);

        fs::remove_file(&path)?;
    }

    Ok(())
}
