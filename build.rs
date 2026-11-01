// On Windows, the .exe gets the app icon (assets/icon.ico, rendered from
// assets/icon.svg).
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.contains("windows") {
        return;
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
    winresource::WindowsResource::new()
        .set_icon("assets/icon.ico")
        .set("ProductName", "Papillon")
        .set("FileDescription", "Papillon")
        .compile()
        .expect("Windows resources (icon) compiled");
}
