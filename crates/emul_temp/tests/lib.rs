use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
    process,
};

use emul_temp::{RESET_TARGET, set_fake_batt_temp, set_thermal_zone_emul_temp};

fn temp_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("emul_temp_{}_{}", process::id(), name))
}

#[test]
fn sets_emul_temp_on_zones_with_node_and_skips_missing_ones() -> anyhow::Result<()> {
    let root = temp_dir("thermal");
    let _ = fs::remove_dir_all(&root);
    let with_node = root.join("thermal_zone0");
    let without_node = root.join("thermal_zone1");
    fs::create_dir_all(&with_node)?;
    fs::create_dir_all(&without_node)?;
    fs::write(with_node.join("type"), "shell_back\n")?;
    fs::write(with_node.join("emul_temp"), "999\n")?;

    set_thermal_zone_emul_temp(&root, RESET_TARGET);

    assert_eq!(fs::read_to_string(with_node.join("emul_temp"))?, "0\n");
    assert!(!without_node.join("emul_temp").exists());
    fs::remove_dir_all(&root)?;

    Ok(())
}

#[test]
fn ignores_entries_that_are_not_thermal_zone_dirs() -> anyhow::Result<()> {
    let root = temp_dir("ignored");
    let _ = fs::remove_dir_all(&root);
    let file_zone = root.join("thermal_zone9");
    let other_dir = root.join("cooling_device0");
    fs::create_dir_all(&other_dir)?;
    fs::write(&file_zone, "not a dir\n")?;
    fs::write(other_dir.join("emul_temp"), "1\n")?;

    set_thermal_zone_emul_temp(&root, 12345);

    assert_eq!(fs::read_to_string(other_dir.join("emul_temp"))?, "1\n");
    fs::remove_dir_all(&root)?;

    Ok(())
}

#[test]
fn missing_thermal_dir_is_not_an_error() {
    set_thermal_zone_emul_temp(Path::new("/nonexistent/thermal"), RESET_TARGET);
}

#[test]
fn follows_thermal_zone_symlinks_and_sets_non_shell_zones() -> anyhow::Result<()> {
    let root = temp_dir("symlink");
    let _ = fs::remove_dir_all(&root);
    let device = root.join("sensor");
    fs::create_dir_all(&device)?;
    fs::write(device.join("type"), "cpu\n")?;
    fs::write(device.join("emul_temp"), "0\n")?;
    symlink(&device, root.join("thermal_zone0"))?;

    set_thermal_zone_emul_temp(&root, 43000);

    assert_eq!(fs::read_to_string(device.join("emul_temp"))?, "43000\n");
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn a_failed_zone_does_not_prevent_other_writes() -> anyhow::Result<()> {
    let root = temp_dir("failed_zone");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("thermal_zone0/emul_temp"))?;
    let writable = root.join("thermal_zone1");
    fs::create_dir_all(&writable)?;
    fs::write(writable.join("emul_temp"), "999\n")?;

    set_thermal_zone_emul_temp(&root, RESET_TARGET);

    assert_eq!(fs::read_to_string(writable.join("emul_temp"))?, "0\n");
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn fake_batt_temp_rejects_unwritable_file() {
    let path = Path::new("/nonexistent/shell-temp");
    assert!(set_fake_batt_temp(path).is_err());
}

#[test]
fn fake_batt_temp_writes_readonly_file_and_preserves_ownership() -> anyhow::Result<()> {
    let root = temp_dir("shell_temp");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let path = root.join("shell-temp");
    fs::write(&path, "original\n")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;
    let original = fs::metadata(&path)?;

    set_fake_batt_temp(&path)?;

    let content = fs::read_to_string(&path)?;
    let updated = fs::metadata(&path)?;
    fs::remove_dir_all(&root)?;
    assert_eq!(content, "2 36000\n");
    assert_eq!(updated.permissions().mode() & 0o777, 0o444);
    assert_eq!(
        (updated.uid(), updated.gid()),
        (original.uid(), original.gid())
    );
    Ok(())
}

#[test]
fn fake_batt_temp_restores_readonly_permissions_after_write_failure() -> anyhow::Result<()> {
    let path = temp_dir("write_failure");
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;

    let result = set_fake_batt_temp(&path);

    let mode = fs::metadata(&path)?.permissions().mode() & 0o777;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    fs::remove_dir_all(&path)?;
    assert!(result.is_err());
    assert_eq!(mode, 0o444);
    Ok(())
}
