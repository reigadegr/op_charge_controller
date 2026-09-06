use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::LazyLock,
    time::{SystemTime, UNIX_EPOCH},
};

use rustix::{
    fs::{Gid, Mode, Uid, chmod, chown},
    mount::{UnmountFlags, mount_bind, unmount},
};

static MASKS_DIR: LazyLock<io::Result<PathBuf>> = LazyLock::new(|| masks_dir(&env::current_exe()?));

fn masks_dir(executable: &Path) -> io::Result<PathBuf> {
    executable
        .parent()
        .map(|directory| directory.join("masks"))
        .ok_or_else(|| io::Error::other("无法获取当前 ELF 所在目录"))
}

pub fn lock_val(value: &str, path: &Path) -> io::Result<()> {
    let file = path.canonicalize()?;
    let value = format!("{value}\n");
    lock_value(&file, &value)
}

fn lock_value(file: &Path, value: &str) -> io::Result<()> {
    let _ = unmount(file, UnmountFlags::empty());
    chown(file, Some(Uid::ROOT), Some(Gid::ROOT))?;
    chmod(file, Mode::from_raw_mode(0o644))?;
    fs::write(file, value)?;
    chmod(file, Mode::from_raw_mode(0o444))?;

    Ok(())
}

pub fn mask_val(value: &str, path: &Path) -> io::Result<()> {
    let masks_dir = MASKS_DIR
        .as_ref()
        .map_err(|error| io::Error::new(error.kind(), format!("获取 masks 目录失败: {error}")))?;
    let file = path.canonicalize()?;
    let value = format!("{value}\n");
    let mask = write_mask_file(masks_dir, &value)?;
    if let Err(error) = lock_value(&file, &value) {
        let _ = fs::remove_file(&mask);
        return Err(error);
    }
    if let Err(error) = mount_bind(&mask, &file) {
        let _ = fs::remove_file(&mask);
        return Err(error.into());
    }

    let _ = Command::new("/system/bin/restorecon")
        .args(["-R", "-F"])
        .arg(file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    Ok(())
}

fn write_mask_file(masks_dir: &Path, value: &str) -> io::Result<PathBuf> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mask = masks_dir.join(format!("mask_{time}"));
    if let Err(error) = fs::write(&mask, value)
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error);
    }
    if !mask.exists() {
        fs::create_dir_all(masks_dir)?;
        fs::write(&mask, value)?;
    }

    Ok(mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_dir_is_next_to_executable() {
        let masks_dir = masks_dir(Path::new("/system/bin/op_charge_controller"));

        assert!(matches!(
            masks_dir,
            Ok(path) if path == Path::new("/system/bin/masks")
        ));
    }

    #[test]
    fn masks_dir_rejects_executable_without_parent() {
        assert!(masks_dir(Path::new("/")).is_err());
    }

    #[test]
    fn write_mask_file_creates_missing_masks_directory() -> io::Result<()> {
        let masks_dir =
            std::env::temp_dir().join(format!("op_charge_controller_{}_masks", std::process::id()));
        let _ = std::fs::remove_dir_all(&masks_dir);

        let mask = write_mask_file(&masks_dir, "1\n")?;

        assert_eq!(std::fs::read_to_string(&mask)?, "1\n");
        std::fs::remove_dir_all(masks_dir)
    }
}
