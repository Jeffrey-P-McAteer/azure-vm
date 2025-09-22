
use std::fs;
use std::io;
use std::path::Path;


pub fn process_usb_passthrough_to_qemu_args(vendor_and_product_ids: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
  let mut split = vendor_and_product_ids.split(':');
  let vendor_hex = split.nth(0).ok_or("no vendor specified")?;
  let product_hex = split.nth(0).ok_or("no product specified")?;

  let mut args: Vec<String> = Vec::with_capacity(4);
  args.extend(["-device".to_string(), "qemu-xhci,id=usb".to_string()]);

  args.extend(qemu_usb_args(vendor_hex, product_hex)?);

  Ok(args)
}


fn prepare_usb_for_qemu(vendor: &str, product: &str) -> Result<Option<QemuUsbArgs>, Box<dyn std::error::Error>> {
    let sysfs_root = "/sys/bus/usb/devices";

    for entry in fs::read_dir(sysfs_root)? {
        let entry = entry?;
        let path = entry.path();

        let id_vendor = fs::read_to_string(path.join("idVendor")).unwrap_or_default();
        let id_product = fs::read_to_string(path.join("idProduct")).unwrap_or_default();

        if id_vendor.trim().eq_ignore_ascii_case(vendor)
            && id_product.trim().eq_ignore_ascii_case(product)
        {
            println!("Found USB device {}:{} at {:?}", vendor, product, path.display());
            // Found the device
            let busnum = fs::read_to_string(path.join("busnum"))?.trim().to_string();
            let devnum = fs::read_to_string(path.join("devnum"))?.trim().to_string();

            let hostbus: u8 = busnum.parse().unwrap();
            let hostaddr: u8 = devnum.parse().unwrap();

            // Try to unbind from current driver
            let driver_path = path.join("driver");
            if driver_path.exists() {
                if let Ok(driver_target) = fs::read_link(&driver_path) {
                    if let Some(driver_name) = driver_target.file_name() {
                        let unbind_path =
                            Path::new("/sys/bus/usb/drivers").join(driver_name).join("unbind");
                        if unbind_path.exists() {
                            let dev_name = path.file_name().unwrap().to_string_lossy();
                            sudo_tee(&unbind_path, &dev_name)?;
                            println!("Unbound device {} from driver {}", dev_name, driver_name.to_string_lossy());
                        }
                    }
                }
            }

            let dev_bus_path = format!("/dev/bus/usb/{:03}/{:03}", hostbus, hostaddr);
            make_world_rw(&dev_bus_path)?;
            bind_usb_stub(vendor, product)?;

            return Ok(Some(QemuUsbArgs { hostbus, hostaddr }));
        }
    }

    Ok(None)
}

/// Make a USB device invisible to the host by binding it to usb-stub
fn bind_usb_stub(vendor: &str, product: &str) -> io::Result<()> {
    // Load usb-stub if not already loaded
    let _ = std::process::Command::new("sudo").arg("modprobe").arg("usb-stub").status();

    // Bind device to usb-stub
    let new_id_path = Path::new("/sys/bus/usb/drivers/usb-stub/new_id");
    sudo_tee(&new_id_path, &format!("{} {}", vendor, product))?;
    println!(
        "Bound USB device {:04x}:{:04x} to usb-stub (invisible to host)",
        u16::from_str_radix(vendor, 16).unwrap(),
        u16::from_str_radix(product, 16).unwrap()
    );
    Ok(())
}

fn make_world_rw(path:&str) -> io::Result<()> {
  let status = std::process::Command::new("sudo")
        .arg("chmod")
        .arg("a+rwx")
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()?
        .wait()?;
  if !status.success() {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("chmod failed for {}", path),
        ))
    } else {
        Ok(())
    }
}

fn sudo_tee(path: &Path, value: &str) -> io::Result<()> {
    let status = std::process::Command::new("sudo")
        .arg("tee")
        .arg(path.to_string_lossy().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(value.as_bytes())?;
            }
            child.wait()
        })?;

    if !status.success() {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!("tee command failed for {}", path.display()),
        ))
    } else {
        Ok(())
    }
}



/// Construct QEMU arguments for the USB device
fn qemu_usb_args(vendor: &str, product: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    if let Some(_usb) = prepare_usb_for_qemu(vendor, product)? {
        // let arg = format!(
        //     "usb-host,hostbus={},hostaddr={}",
        //     usb.hostbus, usb.hostaddr
        // );
        let arg = format!(
            "usb-host,vendorid=0x{},productid=0x{}",
            vendor, product,
        );
        Ok(vec!["-device".to_string(), arg])
    } else {
        Ok(Vec::new())
    }
}


/// Represents the data needed for QEMU USB passthrough
#[allow(dead_code)]
#[derive(Debug)]
struct QemuUsbArgs {
    hostbus: u8,
    hostaddr: u8,
}
