use std::{env, fs, path::PathBuf, process};

use config::AtomicConfig;

fn profile_path(name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "op_charge_controller_{}_{}_toml",
        process::id(),
        name
    ))
}

fn write_profile(path: &PathBuf, max_vote: i32) -> std::io::Result<()> {
    fs::write(
        path,
        format!(
            "\
ufcs_max_vote={max_vote}
ufcs_ramp_step_ma=300
ufcs_taper_step_ma=100
constant_voltage_mv=4500
charge_cutoff_mv=4570
"
        ),
    )?;

    Ok(())
}

#[test]
fn atomic_config_reloads_valid_profiles_and_keeps_the_last_valid_one() -> anyhow::Result<()> {
    let path = profile_path("reload");
    let _ = fs::remove_file(&path);
    write_profile(&path, 6200)?;

    let config = AtomicConfig::from_path(path.to_string_lossy())?;
    assert_eq!(config.profile(), path.to_string_lossy().as_ref());
    assert_eq!(config.get().ufcs_max_vote, 6200);
    assert!(fs::read_to_string(&path)?.contains("ufcs_max_vote = 6200"));

    write_profile(&path, 5000)?;
    config.reload();
    assert_eq!(config.get().ufcs_max_vote, 5000);

    write_profile(&path, -1)?;
    config.reload();
    assert_eq!(config.get().ufcs_max_vote, 5000);

    fs::remove_file(&path)?;

    Ok(())
}

#[test]
fn invalid_profiles_are_rejected_before_formatting() -> anyhow::Result<()> {
    let path = profile_path("invalid");
    let _ = fs::remove_file(&path);
    let invalid = "ufcs_max_vote=-1\nufcs_ramp_step_ma=300\nufcs_taper_step_ma=100\nconstant_voltage_mv=4500\ncharge_cutoff_mv=4570\n";
    fs::write(&path, invalid)?;

    assert!(AtomicConfig::from_path(path.to_string_lossy()).is_err());
    assert_eq!(fs::read_to_string(&path)?, invalid);

    fs::remove_file(&path)?;

    Ok(())
}
