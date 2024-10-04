
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


  #[serde(default = "default_bios_override_val")]
  pub bios_override: String,
  #[serde(default = "default_spice_gl_override")]
  pub spice_gl_override: String,
  #[serde(default = "default_spice_rendernode_override")]
  pub spice_rendernode_override: String,

  #[serde(default = "default_smp_override")]
  pub smp_override: String,

  #[serde(default = "default_cpu_override")]
  pub cpu_override: String,

  #[serde(default = "default_machine_override")]
  pub machine_override: String,

  pub ram_mb: usize,

  #[serde(default = "empty_vec_string")]
  pub addtl_args: Vec<String>,

  #[serde(default = "empty_string")]
  pub rdp_uname: String,
  #[serde(default = "empty_string")]
  pub rdp_pass: String,

  #[serde(default = "empty_vec_string")]
  pub addtl_rdp_args: Vec<String>,

  #[serde(default = "empty_vec_string")]
  pub preboot_cmds: Vec<String>,

}

impl VMBlock {
  pub fn flag_path(&self, flag: &str) -> PathBuf {
    if self.disk_partuuid.len() > 1 {
      if flag.chars().next() == Some('.') { // If  a '.' is already specified, avoid creating 2 dots in file path
        PathBuf::from(format!("/mnt/scratch/vms/_flag_{}{}", &self.disk_partuuid, flag))
      }
      else {
        PathBuf::from(format!("/mnt/scratch/vms/_flag_{}.{}", &self.disk_partuuid, flag))
      }
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

fn default_bios_override_val() -> String {
  "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd".into()
}

fn default_spice_gl_override() -> String {
  "on".into() // or "off"
}

fn default_spice_rendernode_override() -> String {
  "/dev/dri/by-path/pci-0000:00:02.0-render".into()
}

fn default_smp_override() -> String {
  "4".into()
}

fn default_cpu_override() -> String {
  "host".into()
}

fn default_machine_override() -> String {
  "type=pc,accel=kvm,kernel_irqchip=on".into()
}

impl VMConfig {
  pub fn apply_env_overrides(&mut self) {
    if let Ok(var_val) = std::env::var("spice_gl_override") {
      if var_val.len() > 0 {
        self.vm.spice_gl_override = var_val.into();
      }
    }
    if let Ok(var_val) = std::env::var("smp_override") {
      if var_val.len() > 0 {
        self.vm.smp_override = var_val.into();
      }
    }

  }
}
