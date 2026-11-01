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
        .set("OriginalFilename", "papillon.exe")
        .set("CompanyName", "Shhu")
        .set(
            "LegalCopyright",
            "The Papillon contributors, MIT or Apache-2.0",
        )
        .compile()
        .expect("Windows resources (icon) compiled");
}
