use std::env;

fn main() {
    const ICON_PATH: &str = "../../assets/brand/corbit.ico";

    println!("cargo:rerun-if-changed={ICON_PATH}");
    println!("cargo:rerun-if-env-changed=CORBIT_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=CORBIT_DAEMON_VERSION");

    let channel = env::var("CORBIT_BUILD_CHANNEL").unwrap_or_else(|_| "release".to_owned());
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=CORBIT_BUILD_CHANNEL={channel}");
    println!("cargo:rustc-env=CORBIT_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=CORBIT_BUILD_TARGET={target}");
    let daemon_version = env::var("CORBIT_DAEMON_VERSION")
        .unwrap_or_else(|_| env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "unknown".into()));
    println!("cargo:rustc-env=CORBIT_DAEMON_VERSION={daemon_version}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(ICON_PATH)
        .set("ProductName", "Corbit")
        .set("FileDescription", "Corbit Desktop")
        .set("InternalName", "corbit")
        .set("OriginalFilename", "Corbit.exe");
    resource
        .compile()
        .expect("failed to compile the Corbit Windows resources");
}
