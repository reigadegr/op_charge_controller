use std::{
    env, fs,
    io::{self, ErrorKind},
    os::unix::fs::symlink,
    path::PathBuf,
    process,
};

use utils::{BatteryCapacityReader, BccParams, BccParamsReader, ChargeTypeReader, SysfsReader};

fn write_content(name: &str, content: &str) -> io::Result<PathBuf> {
    let path = env::temp_dir().join(format!(
        "op_charge_controller_{}_{}_battery",
        process::id(),
        name
    ));
    fs::write(&path, content)?;

    Ok(path)
}

#[test]
fn parses_bcc_params_battery_fields() -> io::Result<()> {
    let path = write_content("bcc_params", "0,1,2,3,4,5,4400,7,-5000,9,10,4390\n")?;
    let params = BccParamsReader::from_path(&path)?.read()?;

    assert_eq!(
        params,
        BccParams {
            cell_voltage_1_mv: 4400.0,
            cell_voltage_2_mv: 4390.0,
            current_ma: -5000.0,
        }
    );
    fs::remove_file(path)
}

#[test]
fn rejects_missing_bcc_params_fields() -> io::Result<()> {
    let path = write_content("bcc_params_missing", "0,1,2")?;
    assert!(matches!(
        BccParamsReader::from_path(&path)?.read(),
        Err(error) if error.kind() == ErrorKind::InvalidData
    ));
    fs::remove_file(path)
}

#[test]
fn rejects_non_finite_bcc_params_fields() -> io::Result<()> {
    for (name, content) in [
        ("bcc_params_nan", "0,1,2,3,4,5,NaN,7,-5000,9,10,4390"),
        ("bcc_params_inf", "0,1,2,3,4,5,4400,7,inf,9,10,4390"),
    ] {
        let path = write_content(name, content)?;
        assert!(matches!(
            BccParamsReader::from_path(&path)?.read(),
            Err(error) if error.kind() == ErrorKind::InvalidData
        ));
        fs::remove_file(path)?;
    }

    Ok(())
}

#[test]
fn parses_charge_type_field() -> io::Result<()> {
    for (name, content, expected) in [
        ("charge_type_15", "0,1,2,3,4,5,6,7,8,15,10,11,5000\n", 15),
        ("charge_type_14", " 0,1,2,3,4,5,6,7,8, 14 ,10,11\n", 14),
    ] {
        let path = write_content(name, content)?;
        assert_eq!(ChargeTypeReader::from_path(&path)?.read()?, expected);
        fs::remove_file(path)?;
    }

    Ok(())
}

#[test]
fn parses_battery_capacity() -> io::Result<()> {
    for (name, content, expected) in [("capacity_65", "65\n", 65), ("capacity_2", " 2 ", 2)] {
        let path = write_content(name, content)?;
        assert_eq!(BatteryCapacityReader::from_path(&path)?.read()?, expected);
        fs::remove_file(path)?;
    }

    Ok(())
}

#[test]
fn rejects_missing_charge_type_field() -> io::Result<()> {
    let path = write_content("charge_type_missing", "0,1,2")?;
    assert!(matches!(
        ChargeTypeReader::from_path(&path)?.read(),
        Err(error) if error.kind() == ErrorKind::InvalidData
    ));
    fs::remove_file(path)
}

#[test]
fn rejects_invalid_bcc_params_fields() -> io::Result<()> {
    let path = write_content(
        "bcc_params_invalid",
        "0,1,2,3,4,5,invalid,7,-5000,9,10,4390",
    )?;
    assert!(matches!(
        BccParamsReader::from_path(&path)?.read(),
        Err(error) if error.kind() == ErrorKind::InvalidData
    ));
    fs::remove_file(path)
}

#[test]
fn sysfs_reader_reopens_after_read_failure() -> io::Result<()> {
    let path = env::temp_dir().join(format!(
        "op_charge_controller_{}_sysfs_reader",
        process::id()
    ));
    let _ = fs::remove_file(&path);
    symlink(env::temp_dir(), &path)?;
    let mut reader = SysfsReader::new(&path, 16)?;

    assert!(reader.read().is_err());
    fs::remove_file(&path)?;
    fs::write(&path, "first\n")?;
    assert_eq!(reader.read()?, "first\n");
    fs::write(&path, "second\n")?;
    assert_eq!(reader.read()?, "second\n");
    fs::remove_file(&path)
}
