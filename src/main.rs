
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;

use std::io::Write;


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
    let first_arg = args[1].clone();

    let rt = tokio::runtime::Builder::new_multi_thread()
      .enable_all()
      .worker_threads(2)
      .build()
      .expect("Could not build tokio runtime!");

    return rt.block_on(vm_manager(first_arg));

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


async fn ensure_file_downloaded(url: &str, local_file: &std::path::Path) {
  use futures::StreamExt;

  let local_file_path = local_file.to_owned();
  if let Some(local_parent_dir) = local_file_path.parent() {
    println!("Ensuring {} exists...", local_parent_dir.display());
    dump_error!( tokio::fs::create_dir_all(local_parent_dir).await );
  }

  dump_error!(
    tokio::process::Command::new("wget")
      .args(&["--continue", url, "-O", format!("{}", local_file.display()).as_str() ])
      .status()
      .await
  );

  // if ! local_file_path.exists() {
  //   // Download it
  //   println!("Downloading {} to {}", url, local_file.display());
  //   let mut local_virtio_iso_file = tokio::fs::File::create(local_file).await.expect("Could not create file!");
  //   let conn = reqwest::get(url).await.expect("Could not connect!");
  //   let mut download_stream = conn.bytes_stream();

  //   while let Some(item) = download_stream.next().await {
  //     dump_error!( tokio::io::copy(&mut item.unwrap().as_ref(), &mut local_virtio_iso_file).await );
  //   }

  // }
}


async fn handle_exit_signals() {
  let mut int_stream = dump_error_and_ret!(
    tokio::signal::unix::signal(
      tokio::signal::unix::SignalKind::interrupt()
    )
  );
  let mut term_stream = dump_error_and_ret!(
    tokio::signal::unix::signal(
      tokio::signal::unix::SignalKind::terminate()
    )
  );
  loop {
    let mut want_shutdown = false;
    tokio::select!{
      _sig_int = int_stream.recv() => { want_shutdown = true; }
      _sig_term = term_stream.recv() => { want_shutdown = true; }
    };
    if want_shutdown {
      do_shutdown().await;
    }
  }
}

async fn do_shutdown() {
  println!("Got SIG{{TERM/INT}}, shutting down!");
  
  let qemu_pid = QEMU_PROC_PID.load(std::sync::atomic::Ordering::SeqCst);
  if qemu_pid > 3 {
    for signal in &[nix::sys::signal::Signal::SIGCONT, nix::sys::signal::Signal::SIGINT, nix::sys::signal::Signal::SIGTERM] {
      dump_error!(
        nix::sys::signal::kill(
          nix::unistd::Pid::from_raw( qemu_pid ), *signal
        )
      );
      tokio::time::sleep( tokio::time::Duration::from_millis(50) ).await;
    }
  }

  // Allow spawned futures to complete...
  tokio::time::sleep( tokio::time::Duration::from_millis(400) ).await;
  println!("Goodbye!");
  std::process::exit(0);
}

static QEMU_PROC_PID: once_cell::sync::Lazy<std::sync::atomic::AtomicI32> = once_cell::sync::Lazy::new(||
  std::sync::atomic::AtomicI32::new( 0 )
);


async fn vm_manager(mut path_to_config: String) {

  if ! std::path::Path::new(&path_to_config).exists() {
    // Scan under /j/bins/azure-contain/containers for a file containing this & use that
    let mut containers_dir_o = dump_error_and_ret!( tokio::fs::read_dir("/j/bins/azure-vm/vms").await );
    while let Some(container_toml) = dump_error_and_ret!( containers_dir_o.next_entry().await ) {
      if container_toml.file_name().into_string().unwrap_or_default().contains(&path_to_config) {
        path_to_config = container_toml.path().into_os_string().into_string().unwrap_or_default();
        break;
      }
    }
  }

  let _signal_task = tokio::spawn(handle_exit_signals());

  println!("Reading {}", &path_to_config);
  let vm_file_content = tokio::fs::read_to_string(path_to_config).await.expect("Could not read config file!");
  let vm_config: VMConfig = toml::from_str(&vm_file_content).expect("Could not parse config!");
  
  println!("vm_config={:?}", vm_config);

  // if vm_config.install.boot_iso.exists() {
  //   // Touch install media assuming it's under downloads
  //   dump_error!( filetime::set_file_mtime(&vm_config.install.boot_iso, filetime::FileTime::now()) );
  // }
  // else {
  //   // Download it from vm_config.install.boot_iso_url
  //   ensure_file_downloaded(&vm_config.install.boot_iso_url, &vm_config.install.boot_iso).await;
  // }
  ensure_file_downloaded(&vm_config.install.boot_iso_url, &vm_config.install.boot_iso).await;
  dump_error!( filetime::set_file_mtime(&vm_config.install.boot_iso, filetime::FileTime::now()) );

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
  println!("install_flag = {:?}", install_flag);

  if ! install_flag.exists() {
    println!("");
    println!("install_flag file {:?} does not exist, launching w/ install media connected.", install_flag);
    println!("Please install the OS and then run: ");
    println!("  touch {:?}", install_flag);
    println!("");

    let qemu_args: Vec<String> = vec![
      "-drive".into(), format!("format=qcow2,file={}", vm_config.vm.disk_image.to_string_lossy() ),
      "-enable-kvm".into(), "-m".into(), format!("{}M", vm_config.vm.ram_mb ),
      "-cpu".into(), "host,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time".into(),
      "-smp".into(), "2".into(),
      "-machine".into(), "type=pc,accel=kvm,kernel_irqchip=on".into(),

      // Possible CAC reader fwd ( lsusb -t )
      "-usb".into(), "-device".into(), "usb-host,hostbus=1,hostport=2".into(),

      // Use pulse API to talk to pipewire
      "-audiodev".into(), "id=pa,driver=pa,server=/run/user/1000/pulse/native".into(),

      // Hmmm... likely want more config in future.
      "-nic".into(), "user,id=winnet0,id=mynet0,net=192.168.90.0/24,dhcpstart=192.168.90.10".into(),

      "-device".into(), "virtio-vga".into(), // gl=on,max_outputs=1 where do these get set ???
      "-display".into(), "gtk".into(),

      // Attach boot ISO
      "-drive".into(), format!("file={},if=ide,index=1,media=cdrom", vm_config.install.boot_iso.display() ),

      // Attach drivers
      "-drive".into(), format!("file={},if=ide,index=2,media=cdrom", VIRTIO_WIN_ISO_LOCAL_PATH ),

      //"-boot", "d", // c == first hd, d == first cd-rom drive

      "-boot".into(), "menu=on,splash-time=18".into(),

    ];

    let debug_qemu_args = qemu_args.join(" ");
    println!(">>> qemu-system-x86_64 {}", debug_qemu_args);

    dump_error!(
      tokio::process::Command::new("qemu-system-x86_64")
        .args(&qemu_args)
        .status()
        .await
    );
    return;
  }

  // Now run the regular VM

  let spice_socket = vm_config.vm.flag_path(".spice.sock");
  let qmp_socket = vm_config.vm.flag_path(".qmp.sock");

  println!("Spice socket file = {}", spice_socket.display() );
  println!("QMP socket file = {}", qmp_socket.display() );

  if qmp_socket.exists() {
    dump_error!( tokio::fs::remove_file(&qmp_socket).await );
  }

  let mut qemu_proc = tokio::process::Command::new("qemu-system-x86_64")
        .args(&[
          "-drive", format!("format=qcow2,file={}", vm_config.vm.disk_image.to_string_lossy() ).as_str(),
          "-enable-kvm", "-m", format!("{}M", vm_config.vm.ram_mb ).as_str(),
          "-cpu", "host,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time",
          "-smp", "2",
          "-machine", "type=pc,accel=kvm,kernel_irqchip=on",

          "-qmp", format!("unix:{},server=on,wait=off", qmp_socket.display() ).as_str(),

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

          "-drive", format!("file={},if=ide,index=2,media=cdrom", VIRTIO_WIN_ISO_LOCAL_PATH ).as_str(),

          "-boot", "c", // c == first hd, d == first cd-rom drive

        ])
        .spawn()
        .expect("Could not spawn child proc");

  let qemu_pid = qemu_proc.id().unwrap_or(0);
  QEMU_PROC_PID.store(qemu_pid as i32, std::sync::atomic::Ordering::SeqCst);

  tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;

  let mut input_lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();

  print!("> "); // prompt
  dump_error!( std::io::stdout().flush() );
  dump_error!( tokio::io::stdout().flush().await );

  let qapi_stream = dump_error_and_ret!( qapi::futures::QgaStreamTokio::open_uds(qmp_socket).await );
  let (qga, handle) = qapi_stream.spawn_tokio();

  let sync_value = &qga as *const _ as usize as i32;
  dump_error!( qga.guest_sync(sync_value).await );


  while let Ok(Some(line)) = input_lines.next_line().await {

    match qemu_proc.try_wait() {
      Ok(Some(exit_code)) => { println!("Qemu exited with {}, exiting!", exit_code ); break; },
      _ => { /* don't care */ }
    }

    let line = line.trim();

    if line == "gui" {
      println!("Launching SPICE client...");
      dump_error!(
        tokio::process::Command::new("spicy")
          .args(&[
            format!("--uri=spice+unix://{}", spice_socket.display()).as_str()
          ])
          .status()
          .await
      );
    }
    else if line == "help" {
      println!(r#"Commands:
  - gui
      Opens SPICE client
  - help
      Show this help
  - exit / quit
      Kill VM and exit
  - *
      Run as command in VM, returning output.
"#);
    }
    else if line == "quit" || line == "exit" {
      break;
    }
    else {
      // Connect to QMP and send line in verbatim
      println!("Sending to QMP: {}", line);

      match qga.execute(qapi::qga::guest_info { }).await {
        Ok(info) => {
          println!("Guest Agent version: {}", info.version);
        }
        Err(e) => {
          println!("Error: {:?}", e);
        }
      }


    }

    print!("> "); // prompt
    dump_error!( std::io::stdout().flush() );
    dump_error!( tokio::io::stdout().flush().await );
  }

  println!("");
  dump_error!( std::io::stdout().flush() );
  dump_error!( tokio::io::stdout().flush().await );

  println!("Killing qemu...");

  // And kill child on exit
  dump_error!( qemu_proc.kill().await );

  do_shutdown().await;

}


