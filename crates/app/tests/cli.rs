use std::{
    env, fs,
    process::{Command, Stdio},
};

#[test]
fn cli_rejects_an_invalid_config_profile() -> anyhow::Result<()> {
    let profile = env::temp_dir().join(format!(
        "op_charge_controller_{}_invalid_toml",
        std::process::id()
    ));
    let _ = fs::remove_file(&profile);
    fs::write(
        &profile,
        "ufcs_max_vote=-1\nufcs_ramp_step_ma=300\nufcs_taper_step_ma=100\nconstant_voltage_mv=4500\ncharge_cutoff_mv=4570\n",
    )?;

    let output = Command::new(env!("CARGO_BIN_EXE_op_charge_controller"))
        .arg(&profile)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ufcs_max_vote"), "stderr: {stderr}");

    fs::remove_file(&profile)?;

    Ok(())
}
