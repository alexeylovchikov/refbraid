// Windows only: put the app icon and version details into the .exe.
fn main() {
    println!("cargo:rerun-if-changed=packaging/windows/refbraid.rc");
    println!("cargo:rerun-if-changed=packaging/windows/refbraid.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("packaging/windows/refbraid.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
