
use std::path::PathBuf;

use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct VMConfig {
  pub install: VMInstallBlock,
  pub vm: VMBlock,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VMInstallBlock {
  pub boot_iso: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VMBlock {
  pub name: String,
  
  pub disk_image: PathBuf,
  pub disk_image_gb: usize,

  pub mount_windows_virtio_iso: bool,

  pub ram_mb: usize,

}

impl VMBlock {
  pub fn flag_path(&self, flag: &str) -> PathBuf {
    let mut flag_file_path = self.disk_image.clone();
    let mut file_name = flag_file_path.file_name().unwrap_or(std::ffi::OsStr::new(&self.name)).to_owned();
    file_name.push( &std::ffi::OsStr::new(flag) );
    flag_file_path.set_file_name(file_name);
    flag_file_path
  }
}


