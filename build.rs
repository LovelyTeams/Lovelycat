fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo::rerun-if-changed=assets/lovelycat.rc");
        println!("cargo::rerun-if-changed=assets/lovelycat.ico");
        embed_resource::compile("assets/lovelycat.rc", embed_resource::NONE)
            .manifest_required()
            .expect("unable to embed the Lovelycat application icon");
    }
}
