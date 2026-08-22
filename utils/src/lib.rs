use std::{
    fs, io,
    path::Path,
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use rustix::{
    fs::{Gid, Mode, Uid, chmod, chown},
    mount::{UnmountFlags, mount_bind, unmount},
};

pub fn lock_val(value: &str, path: &Path) -> io::Result<()> {
    let file = path.canonicalize()?;

    let _ = unmount(&file, UnmountFlags::empty());
    chown(&file, Some(Uid::ROOT), Some(Gid::ROOT))?;
    chmod(&file, Mode::from_raw_mode(0o644))?;
    fs::write(&file, format!("{value}\n"))?;
    chmod(&file, Mode::from_raw_mode(0o444))?;

    Ok(())
}

pub fn mask_val(value: &str, path: &Path, masks_dir: &Path) -> io::Result<()> {
    let file = path.canonicalize()?;
    lock_val(value, &file)?;

    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let mask = masks_dir.join(format!("mask_{time}"));
    fs::write(&mask, format!("{value}\n"))?;
    mount_bind(&mask, &file)?;

    let _ = Command::new("restorecon")
        .args(["-R", "-F"])
        .arg(file)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    Ok(())
}
