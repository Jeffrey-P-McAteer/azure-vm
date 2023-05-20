
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

  // if vm_config.vm.mount_windows_virtio_iso {
  //   ensure_virtio_win_iso_exists().await; // Now we can ensure passing VIRTIO_WIN_ISO_LOCAL_PATH is safe
  // }
  ensure_virtio_win_iso_exists().await;

  let install_flag = vm_config.vm.flag_path(".installed");
  if ! install_flag.exists() {
    println!("");
    println!("install_flag file {:?} does not exist, launching w/ install media connected.", install_flag);
    println!("Please install the OS and then run: ");
    println!("  touch {:?}", install_flag);
    println!("");

    dump_error!(
      tokio::process::Command::new("qemu-system-x86_64")
        .args(&[
          "-drive", format!("format=qcow2,file={}", vm_config.vm.disk_image.to_string_lossy() ).as_str(),
          "-enable-kvm", "-m", format!("{}M", vm_config.vm.ram_mb ).as_str(),
          "-cpu", "host,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time",
          "-smp", "2",
          "-machine", "type=pc,accel=kvm,kernel_irqchip=on",

          // Possible CAC reader fwd ( lsusb -t )
          "-usb", "-device", "usb-host,hostbus=1,hostport=2",

          // Use pulse API to talk to pipewire
          "-audiodev", "id=pa,driver=pa,server=/run/user/1000/pulse/native",

          // Hmmm... likely want more config in future.
          "-nic", "user,id=winnet0,id=mynet0,net=192.168.90.0/24,dhcpstart=192.168.90.10",

          "-device", "virtio-vga", // gl=on,max_outputs=1 where do these get set ???
          "-display", "gtk",

          // Attach boot ISO
          "-drive", format!("file={},if=ide,index=1,media=cdrom", vm_config.install.boot_iso.display() ).as_str(),

          // Attach drivers
          "-drive", format!("file={},if=ide,index=2,media=cdrom", VIRTIO_WIN_ISO_LOCAL_PATH ).as_str(),

          "-boot", "d", // c == first hd, d == first cd-rom drive


        ])
        .status()
        .await
    );
    return;
  }

  // Now run the regular VM

  let spice_socket = vm_config.vm.flag_path(".spice.sock");

  println!("Spice socket file = {}", spice_socket.display() );

  let mut qemu_proc = tokio::process::Command::new("qemu-system-x86_64")
        .args(&[
          "-drive", format!("format=qcow2,file={}", vm_config.vm.disk_image.to_string_lossy() ).as_str(),
          "-enable-kvm", "-m", format!("{}M", vm_config.vm.ram_mb ).as_str(),
          "-cpu", "host,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time",
          "-smp", "2",
          "-machine", "type=pc,accel=kvm,kernel_irqchip=on",

          // Possible CAC reader fwd ( lsusb -t )
          "-usb", "-device", "usb-host,hostbus=1,hostport=2",

          // Use pulse API to talk to pipewire
          "-audiodev", "id=pa,driver=pa,server=/run/user/1000/pulse/native",

          // Hmmm... likely want more config in future.
          "-nic", "user,id=winnet0,id=mynet0,net=192.168.90.0/24,dhcpstart=192.168.90.10",

          // Assume guest drivers are installed during install phase, use spice UI
          "-vga", "qxl",
          "-device", "virtio-serial-pci",

          "-spice", // /dev/dri/by-path/pci-0000:00:02.0-render is the intel GPU
            format!("unix=on,addr={},gl=on,rendernode=/dev/dri/by-path/pci-0000:00:02.0-render,disable-ticketing=on", spice_socket.display() ).as_str(),

          "-device", "virtserialport,chardev=spicechannel0,name=com.redhat.spice.0",
          "-chardev", "spicevmc,id=spicechannel0,name=vdagent",

          "-boot", "c", // c == first hd, d == first cd-rom drive

        ])
        .spawn()
        .expect("Could not spawn child proc");

  // Run spice client sync
  for _ in 0..10 {
    
    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;

    dump_error!(
      tokio::process::Command::new("spicy")
        .args(&[
          format!("--uri=spice+unix://{}", spice_socket.display()).as_str()
        ])
        .status()
        .await
    );
  }

  println!("Killing qemu...");

  // And kill child on exit
  dump_error!( qemu_proc.kill().await );

}


