
use std::path::PathBuf;

use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct VMConfig {
  pub install: VMInstallBlock,
  pub vm: VMBlock,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VMInstallBlock {
  #[serde(default = "empty_string")]
  pub boot_iso_url: String,
  pub boot_iso: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VMBlock {
  pub name: String,

  #[serde(default = "dev_null_pathbuf")]
  pub disk_image: PathBuf,
  #[serde(default = "zero_usize")]
  pub disk_image_gb: usize,

  #[serde(default = "empty_string")]
  pub disk_partuuid: String,

  #[serde(default = "false_bool")]
  pub mount_windows_virtio_iso: bool,

  pub ram_mb: usize,

  #[serde(default = "empty_vec_string")]
  pub addtl_args: Vec<String>,

  #[serde(default = "empty_string")]
  pub rdp_uname: String,
  #[serde(default = "empty_string")]
  pub rdp_pass: String,

  #[serde(default = "empty_vec_string")]
  pub addtl_rdp_args: Vec<String>,

}

impl VMBlock {
  pub fn flag_path(&self, flag: &str) -> PathBuf {
    if self.disk_partuuid.len() > 1 {
      PathBuf::from(format!("/mnt/scratch/vms/_flag_{}.{}", &self.disk_partuuid, flag))
    }
    else {
      let mut flag_file_path = self.disk_image.clone();
      let mut file_name = flag_file_path.file_name().unwrap_or(std::ffi::OsStr::new(&self.name)).to_owned();
      file_name.push( &std::ffi::OsStr::new(flag) );
      flag_file_path.set_file_name(file_name);
      flag_file_path
    }
  }
}


fn empty_string() -> String {
  String::new()
}

fn dev_null_pathbuf() -> PathBuf {
  PathBuf::from("/dev/null")
}

fn zero_usize() -> usize {
  0
}

fn false_bool() -> bool {
  false
}

fn empty_vec_string() -> Vec<String> {
  vec![]
}

