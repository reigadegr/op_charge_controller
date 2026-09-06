use std::{env, fs, io, path::Path, process};

use utils::{masks_dir, write_mask_file};

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
    let masks_dir = env::temp_dir().join(format!("op_charge_controller_{}_masks", process::id()));
    let _ = fs::remove_dir_all(&masks_dir);

    let mask = write_mask_file(&masks_dir, "1\n")?;

    assert_eq!(fs::read_to_string(&mask)?, "1\n");
    fs::remove_dir_all(masks_dir)
}
