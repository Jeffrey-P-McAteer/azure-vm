
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;

use std::io::Write;
use std::path::PathBuf;

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
  // use futures::StreamExt;

  if url.len() < 2 {
    return;
  }

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

  let mut sys = sysinfo::System::new_all();
  sys.refresh_all();
  let sys_mem_mb = sys.total_memory() / (1024 * 1024);
  println!("sys_mem_mb = {:?}", sys_mem_mb);

  let _signal_task = tokio::spawn(handle_exit_signals());

  println!("Reading {}", &path_to_config);
  let vm_file_content = tokio::fs::read_to_string(path_to_config).await.expect("Could not read config file!");
  let vm_config: VMConfig = toml::from_str(&vm_file_content).expect("Could not parse config!");

  println!("vm_config={:?}", vm_config);

  let vm_is_physical_disk = vm_config.vm.disk_partuuid.len() > 1;

  if !vm_is_physical_disk {
    ensure_file_downloaded(&vm_config.install.boot_iso_url, &vm_config.install.boot_iso).await;
    dump_error!( filetime::set_file_mtime(&vm_config.install.boot_iso, filetime::FileTime::now()) );
  }

  // If the disk image does not exist, create dirs + then the image in qcow2 format
  if !vm_is_physical_disk && ! vm_config.vm.disk_image.exists() {
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

  ensure_virtio_win_iso_exists().await;

  // Spawn any require sub-processes the VM wants
  let mut preboot_children = vec![];
  for preboot_cmd in vm_config.vm.preboot_cmds.iter() {
    match  tokio::process::Command::new("sh")
            .args(&["-c", preboot_cmd])
            .kill_on_drop(true)
            .spawn() {
      Ok(preboot_child) => {
        preboot_children.push(preboot_child);
      }
      Err(e) => {
        eprintln!("{:?}", e);
      }
    }
  }

  if ! vm_is_physical_disk {
    // Check for install
    let install_flag = vm_config.vm.flag_path(".installed");
    println!("install_flag = {:?}", install_flag);

    if ! install_flag.exists() {
      println!("");
      println!("install_flag file {:?} does not exist, launching w/ install media connected.", install_flag);
      println!("Please install the OS and then run: ");
      println!("  touch {:?}", install_flag);
      println!("");

      let qemu_args: Vec<String> = vec![
        "-bios".into(), vm_config.vm.bios_override,
        "-drive".into(), format!("format=qcow2,file={}", vm_config.vm.disk_image.to_string_lossy() ),
        "-enable-kvm".into(),
        "-m".into(), format!("{}M", vm_config.vm.ram_mb ),
        "-cpu".into(), "host,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time".into(),
        "-smp".into(), "2".into(),
        "-machine".into(), "type=pc,accel=kvm,kernel_irqchip=on".into(), // "type=pc,accel=kvm,kernel_irqchip=on".into(),

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

        "-boot".into(), "d".into(), // c == first hd, d == first cd-rom drive

        //"-boot".into(), "menu=on,splash-time=18".into(),

      ];

      // If we request > 1/2 system RAM, limit to just the first 1/2 minus 1gb.
      let sys_mem_limit_mb = (sys_mem_mb/2) - 1024;
      if vm_config.vm.ram_mb <= sys_mem_limit_mb as usize {
        let debug_qemu_args = qemu_args.join(" ");
        println!(">>>");
        println!(">>> qemu-system-x86_64 {}", debug_qemu_args);
        println!(">>>");

        dump_error!(
          tokio::process::Command::new("qemu-system-x86_64")
            .args(&qemu_args)
            .status()
            .await
        );
      }
      else {
        // Throw inside systemd-run and limit real ram to 8000mb

        let mut systemd_run_args: Vec<String> = vec![];

        systemd_run_args.push("--scope".to_string());

        systemd_run_args.push("-p".to_string());
        systemd_run_args.push(format!("MemoryHigh={}M", sys_mem_limit_mb));

        systemd_run_args.push("-p".to_string());
        systemd_run_args.push("MemorySwapMax=999G".to_string());

        systemd_run_args.push("--user".to_string());
        systemd_run_args.push("qemu-system-x86_64".to_string());

        systemd_run_args.extend(qemu_args);

        let debug_systemd_run_args = systemd_run_args.join(" ");
        println!(">>>");
        println!(">>> systemd-run {}", debug_systemd_run_args);
        println!(">>>");

        dump_error!(
          tokio::process::Command::new("systemd-run")
            .args(&systemd_run_args)
            .status()
            .await
        );

      }
      return;
    }
  }

  // Now run the regular VM

  let spice_socket = vm_config.vm.flag_path(".spice.sock");
  let qmp_socket = vm_config.vm.flag_path(".qmp.sock");

  println!("Spice socket file = {}", spice_socket.display() );
  println!("QMP socket file = {}", qmp_socket.display() );

  if qmp_socket.exists() {
    dump_error!( tokio::fs::remove_file(&qmp_socket).await );
  }

  let vm_root_drive_arg: String;
  if vm_is_physical_disk {
    // Lookup disk holding vm_config.vm.disk_partuuid, and check if it exists.
    let dev_rel_link = PathBuf::from(format!("/dev/disk/by-partuuid/{}", &vm_config.vm.disk_partuuid));
    let mut dev_reg_path = tokio::fs::canonicalize(dev_rel_link).await.expect("Cannot find disk_partuuid! is it connected?");
    // trim last char in dev_reg_path assuming it's a partition, then see if it exists.
    let dev_part_name = dev_reg_path.file_name().expect("No file name!");
    let mut dev_part_name = dev_part_name.to_str().expect("Bad file name!").to_owned();
    dev_part_name.pop(); // remove last character, eg "sda2" becomes "sda"

    dev_reg_path.set_file_name(dev_part_name);

    vm_root_drive_arg = format!("format=raw,file={}", dev_reg_path.display() );

  }
  else {
    vm_root_drive_arg = format!("format=qcow2,file={}", vm_config.vm.disk_image.to_string_lossy() );
  }

  let mut qemu_args: Vec<String> = vec![
    "-bios".into(), (&vm_config.vm.bios_override).to_string(), // "-bios" MUST always be in this position, b/c we remove these if bios_override.len() < 1

    "-drive".into(), vm_root_drive_arg,
    "-enable-kvm".into(),
    "-m".into(), format!("{}M", vm_config.vm.ram_mb ),
    //"-cpu".into(), "host,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time".into(),
    "-cpu".into(), "host".into(),
    "-smp".into(), vm_config.vm.smp_override.into(),
    "-machine".into(), "type=pc,accel=kvm,kernel_irqchip=on".into(),

    "-qmp".into(), format!("unix:{},server=on,wait=off", qmp_socket.display() ),

    // Possible CAC reader fwd ( lsusb -t )
    // "-usb".into(), "-device".into(), "usb-host,hostbus=1,hostport=2".into(),

    // Use pulse API to talk to pipewire
    //"-audiodev".into(), "id=pa,driver=pa,server=/run/user/1000/pulse/native".into(),
    "-audiodev".into(), "id=alsa,driver=alsa".into(), // yay -S qemu-audio-alsa
    "-device".into(), "intel-hda".into(), "-device".into(), "hda-output,audiodev=alsa".into(), // frontend HW presented to VM

    // Hmmm... likely want more config in future.
    "-nic".into(), "user,id=winnet0,id=mynet0,net=192.168.90.0/24,dhcpstart=192.168.90.10,hostfwd=tcp::3389-:3389,hostfwd=udp::3389-:3389".into(),

    // Assume guest drivers are installed during install phase, use spice UI
    // "-device".into(), "virtio-gpu-pci".into(),
    "-device".into(), "virtio-serial-pci".into(),

    "-spice".into(), // /dev/dri/by-path/pci-0000:00:02.0-render is the intel GPU
      format!("unix=on,addr={},gl={},disable-ticketing=on", spice_socket.display(), vm_config.vm.spice_gl_override),
      //format!("unix=on,addr={},gl={},rendernode={},disable-ticketing=on", spice_socket.display(), vm_config.vm.spice_gl_override, vm_config.vm.spice_rendernode_override ),

    "-device".into(), "virtserialport,chardev=spicechannel0,name=com.redhat.spice.0".into(),
    "-chardev".into(), "spicevmc,id=spicechannel0,name=vdagent".into(),

    // "-vga".into(), "virtio".into(), // alternatively; -vga std?

    "-drive".into(), format!("file={},if=ide,index=2,media=cdrom", VIRTIO_WIN_ISO_LOCAL_PATH ),

    "-boot".into(), "c".into(), // c == first hd, d == first cd-rom drive

  ];

  { // Magic stuff
    if vm_config.vm.bios_override.len() < 1 {
      qemu_args.drain(0..2); // Remove "-bios", "" b/c empty string sent in
    }

    // If "-boot" appears in addtl_args, remove the LAST two arguments
    if vm_config.vm.addtl_args.iter().any(|e| e == "-boot" ) {
      qemu_args.drain(qemu_args.len()-2..qemu_args.len()); // Remove "-boot", "" b/c empty string sent in
    }
  }

  qemu_args.extend(vm_config.vm.addtl_args);
  let qemu_args = qemu_args;

  // If we request > 1/2 system RAM, limit to just the first 1/2 minus 1gb.
  let sys_mem_limit_mb = (sys_mem_mb/2) - 1024;
  let mut qemu_proc = if vm_config.vm.ram_mb <= sys_mem_limit_mb as usize {
    let debug_qemu_args = qemu_args.join(" ");
    println!(">>>");
    println!(">>> qemu-system-x86_64 {}", debug_qemu_args);
    println!(">>>");

    tokio::process::Command::new("qemu-system-x86_64")
          .args(&qemu_args)
          .spawn()
          .expect("Could not spawn child proc")
  }
  else {
    // Throw inside systemd-run and limit real ram to sys_mem_limit_mb

    let mut systemd_run_args: Vec<String> = vec![];

    systemd_run_args.push("--scope".to_string());

    systemd_run_args.push("-p".to_string());
    systemd_run_args.push(format!("MemoryHigh={}M", sys_mem_limit_mb));

    systemd_run_args.push("-p".to_string());
    systemd_run_args.push("MemorySwapMax=999G".to_string());

    systemd_run_args.push("--user".to_string());
    systemd_run_args.push("qemu-system-x86_64".to_string());

    systemd_run_args.extend(qemu_args);

    let debug_systemd_run_args = systemd_run_args.join(" ");
    println!(">>>");
    println!(">>> systemd-run {}", debug_systemd_run_args);
    println!(">>>");

    // Attempt to swapon another 16gb of ram iff it exists on /mnt/scratch/
    for swap_n in 1..4 {
      if std::path::Path::new("/mnt/scratch/swap-files").exists() {
        dump_error!(
          tokio::process::Command::new("sudo")
            .args(&[
              "swapon",
              format!("/mnt/scratch/swap-files/swap-{}", swap_n).as_str(),
            ])
            .status()
            .await
        );
      }
      if std::path::Path::new("/mnt/azure-data").exists() {
        dump_error!(
          tokio::process::Command::new("sudo")
            .args(&[
              "swapon",
              format!("/mnt/azure-data/swap-files/swap-{}", swap_n).as_str(),
            ])
            .status()
            .await
        );
      }
    }

    tokio::process::Command::new("systemd-run")
        .args(&systemd_run_args)
        .spawn()
        .expect("Could not spawn child proc")
  };

  let qemu_pid = qemu_proc.id().unwrap_or(0);
  QEMU_PROC_PID.store(qemu_pid as i32, std::sync::atomic::Ordering::SeqCst);

  tokio::time::sleep(tokio::time::Duration::from_millis(1200)).await;

  let mut input_lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();

  print!("> "); // prompt
  dump_error!( std::io::stdout().flush() );
  dump_error!( tokio::io::stdout().flush().await );

  let qapi_stream = dump_error_and_ret!( qapi::futures::QgaStreamTokio::open_uds(qmp_socket).await );
  let (qga, _handle) = qapi_stream.spawn_tokio();

  let _sync_value = &qga as *const _ as usize as i32;
  //dump_error!( qga.guest_sync(sync_value).await ); // TODO re-investigate setting this up


  while let Ok(Some(line)) = input_lines.next_line().await {

    match qemu_proc.try_wait() {
      Ok(Some(exit_code)) => { println!("Qemu exited with {}, exiting!", exit_code ); break; },
      _ => { /* don't care */ }
    }

    let line = line.trim();

    if line.len() > 0 {
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
      else if line.starts_with("rdp") {
        let mut rdp_args: Vec<String> = vec![];

        rdp_args.push("/cert:ignore".to_string());

        rdp_args.push("/w:1920".to_string());
        rdp_args.push("/h:1080".to_string());

        rdp_args.push("/drive:DOWNLOADS,/j/downloads".to_string());
        rdp_args.push("/dynamic-resolution".to_string());

        if vm_config.vm.rdp_uname.len() > 0 {
          rdp_args.push( format!("/u:{}", vm_config.vm.rdp_uname) );
        }
        if vm_config.vm.rdp_pass.len() > 0 {
          rdp_args.push( format!("/p:{}", vm_config.vm.rdp_pass) );
        }
        rdp_args.push("/v:127.0.0.1".to_string());

        // Audio config - we want to send the aduio over the RDP connection
        rdp_args.push("/audio-mode:0".to_string());
        rdp_args.push("/microphone:sys:alsa".to_string());

        rdp_args.push("/auto-reconnect-max-retries:64".to_string());



        if let Some(cmd_exe) = line.split(" ").nth(1) {
          println!("cmd_exe = {:?}", cmd_exe);
          rdp_args.push(format!("/app:{}", cmd_exe));

          // Append up to 10 args
          let mut cmd_arg_args = cmd_exe.to_string();
          for i in 2..12 {
            if let Some(cmd_arg) = line.split(" ").nth(i) {
              // println!("cmd_arg = {:?}", cmd_arg);
              cmd_arg_args += &(" ".to_string() + cmd_arg);
            }
          }
          if cmd_arg_args.len() > 0 {
            rdp_args.push(format!("/app-cmd:{}", cmd_arg_args));
          }
        }

        rdp_args.extend(vm_config.vm.addtl_rdp_args.clone());

        let rdp_args = rdp_args;

        let freerdp_bin = match std::env::var("WAYLAND_DISPLAY") {
          Ok(waland_disp_val) => {
            if waland_disp_val.len() > 0 {
              "wlfreerdp"
            }
            else {
              "xfreerdp"
            }
          }
          Err(_) => {
            "xfreerdp"
          }
        };

        {
          let debug_rdp_args = rdp_args.join(" ");
          println!("{} {}", freerdp_bin, debug_rdp_args );
        }

        dump_error!(
          tokio::process::Command::new(freerdp_bin)
            .args(&rdp_args)
            .status()
            .await
        );
      }
      else if line == "help" || line == "h" {
        println!(r#"Commands:
  - gui
      Opens SPICE client
  - rdp
      Opens RDP client to 127.0.0.1:3389
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

  // Try to kill other children as well, murder is fun
  for preboot_child in preboot_children.iter_mut() {
    dump_error!( preboot_child.kill().await );
  }

  do_shutdown().await;

}


