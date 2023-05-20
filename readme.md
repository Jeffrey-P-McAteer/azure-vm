
# Azure-VM


Rust frontent to QEMU designed to read VM config from a `.toml` file
and interact with the VM.

Should contain cgrouping capabilities to allow live changes to the VM's CPU time slice
and enough networking to allow for quick remoteapp RDP connections to the host.


## Example Use

```
cargo build --release

/j/bins/azure-vm/target/release/azure-vm /j/bins/azure-vm/vms/tiny11.toml


```

