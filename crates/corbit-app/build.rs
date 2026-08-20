use std::env;

fn main() {
    const ICON_PATH: &str = "../../assets/brand/corbit.ico";

    println!("cargo:rerun-if-changed={ICON_PATH}");

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
