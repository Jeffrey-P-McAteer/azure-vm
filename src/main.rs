
mod structs;
use structs::*;

#[macro_use]
pub mod macros;

fn main() {
  let args: Vec<String> = std::env::args().collect();
  if args.len() < 2 {
    return dump_help();
  }
  else {
    let first_arg = &args[1];

    if first_arg.ends_with(".toml") {
      let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
        .expect("Could not build tokio runtime!");

      return rt.block_on(vm_manager(first_arg));
    }
    else {
      println!("TODO build client system to handle arg {}", first_arg);
    }
  }
}

fn dump_help() {
  println!(r#"Usage:
  {exe} /path/to/vm.toml
    
    Runs the VM

  {exe} TODO more runtime VM control stuff

"#,
  exe=std::env::current_exe().unwrap_or(std::path::PathBuf::from("/dev/null")).display()
);
}

// We'll auto-download this & re-use for all vms requesting mount_windows_virtio_iso = true
const VIRTIO_WIN_ISO_URL: &'static str = "https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/stable-virtio/virtio-win.iso";
const VIRTIO_WIN_ISO_LOCAL_PATH: &'static str = "/mnt/scratch/vms/virtio-win.iso";

async fn ensure_virtio_win_iso_exists() {
  use futures::StreamExt;

  let virtio_iso_path = std::path::PathBuf::from(VIRTIO_WIN_ISO_LOCAL_PATH);
  if let Some(virtio_parent_dir) = virtio_iso_path.parent() {
    println!("Ensuring {} exists...", virtio_parent_dir.display());
    dump_error!( tokio::fs::create_dir_all(virtio_parent_dir).await );
  }
  if ! virtio_iso_path.exists() {
    // Download it
    println!("Downloading {} to {}", VIRTIO_WIN_ISO_URL, VIRTIO_WIN_ISO_LOCAL_PATH);
    let mut local_virtio_iso_file = tokio::fs::File::create(VIRTIO_WIN_ISO_LOCAL_PATH).await.expect("Could not create file!");
    let conn = reqwest::get(VIRTIO_WIN_ISO_URL).await.expect("Could not connect!");
    let mut download_stream = conn.bytes_stream();

    while let Some(item) = download_stream.next().await {
      dump_error!( tokio::io::copy(&mut item.unwrap().as_ref(), &mut local_virtio_iso_file).await );
    }

  }
}


async fn vm_manager(path_to_config: &str) {
  println!("Reading {}", &path_to_config);
  let vm_file_content = tokio::fs::read_to_string(path_to_config).await.expect("Could not read config file!");
  let vm_config: VMConfig = toml::from_str(&vm_file_content).expect("Could not parse config!");
  
  println!("vm_config={:?}", vm_config);

  if vm_config.install.boot_iso.exists() {
    // Touch install media assuming it's under downloads
    dump_error!( filetime::set_file_mtime(&vm_config.install.boot_iso, filetime::FileTime::now()) );
  }

  // If the disk image does not exist, create dirs + then the image in qcow2 format
  if ! vm_config.vm.disk_image.exists() {
    if let Some(vm_image_dir) = vm_config.vm.disk_image.parent() {
      println!("Ensuring {} exists...", vm_image_dir.display());
      dump_error!( tokio::fs::create_dir_all(vm_image_dir).await );
    }

    // create a vm_config.vm.disk_image_gb sized image at vm_config.vm.disk_image
    dump_error!(
      tokio::process::Command::new("qemu-img")
        .args(&["create", "-f", "qcow2", &vm_config.vm.disk_image.to_string_lossy(), format!("{}G", vm_config.vm.disk_image_gb).as_str() ])
        .status()
        .await
    );

  }

  if vm_config.vm.mount_windows_virtio_iso {
    ensure_virtio_win_iso_exists().await; // Now we can ensure passing VIRTIO_WIN_ISO_LOCAL_PATH is safe
  }

  let install_flag = vm_config.vm.install_flag_file();
  if ! install_flag.exists() {
    println!("");
    println!("install_flag file {:?} does not exist, launching w/ install media connected.", install_flag);
    println!("Please install the OS and then run: ");
    println!("  touch {:?}", install_flag);
    println!("");

    dump_error!(
      tokio::process::Command::new("qemu-img")
        .args(&["create", "-f", "qcow2", &vm_config.vm.disk_image.to_string_lossy(), format!("{}G", vm_config.vm.disk_image_gb).as_str() ])
        .status()
        .await
    );
    return;
  }


}


async fn vm_task(vm_config: &VMConfig) {

}
