fn main() {
    println!("cargo:rerun-if-changed=assets/claudehud.rc");
    println!("cargo:rerun-if-changed=assets/claudehud.manifest");
    println!("cargo:rerun-if-changed=assets/claudehud.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/claudehud.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to compile assets/claudehud.rc");
    }
}
