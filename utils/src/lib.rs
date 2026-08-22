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

    let _ = unmount(&file, UnmountFlags::empty());
    chown(&file, Some(Uid::ROOT), Some(Gid::ROOT))?;
    chmod(&file, Mode::from_raw_mode(0o644))?;
    fs::write(&file, format!("{value}\n"))?;
    chmod(&file, Mode::from_raw_mode(0o444))?;

    Ok(())
}

pub fn mask_val(value: &str, path: &Path) -> io::Result<()> {
    let masks_dir = MASKS_DIR
        .as_ref()
        .map_err(|error| io::Error::new(error.kind(), format!("获取 masks 目录失败: {error}")))?;
    let file = path.canonicalize()?;
    lock_val(value, &file)?;

    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mask = masks_dir.join(format!("mask_{time}"));
    let value = format!("{value}\n");
    if let Err(err) = fs::write(&mask, &value) {
        if err.kind() != io::ErrorKind::NotFound {
            return Err(err);
        }
        fs::create_dir_all(masks_dir)?;
        fs::write(&mask, value)?;
    }
    mount_bind(&mask, &file)?;

    let _ = Command::new("/system/bin/restorecon")
        .args(["-R", "-F"])
        .arg(file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    Ok(())
}
