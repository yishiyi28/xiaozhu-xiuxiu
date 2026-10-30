#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=assets/xiaozhu-xiuxiu.ico");

    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/xiaozhu-xiuxiu.ico")
        .set("ProductName", "Xiaozhu Xiuxiu")
        .set("FileDescription", "Xiaozhu Xiuxiu photo editor")
        .set("LegalCopyright", "MIT License");
    resource
        .compile()
        .expect("embed the Windows application icon");
}

#[cfg(not(windows))]
fn main() {}
