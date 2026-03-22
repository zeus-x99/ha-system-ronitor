use sysinfo::System;

fn main() {
    let name = System::name().unwrap_or_else(|| "<none>".to_string());
    let os_version = System::os_version().unwrap_or_else(|| "<none>".to_string());
    let long_os_version = System::long_os_version().unwrap_or_else(|| "<none>".to_string());

    println!("System::name()           = {name}");
    println!("System::os_version()     = {os_version}");
    println!("System::long_os_version()= {long_os_version}");
}
