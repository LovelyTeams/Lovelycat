fn main() {
    println!("cargo::rerun-if-changed=assets/lovelycat.rc");
    println!("cargo::rerun-if-changed=assets/lovelycat.ico");
    embed_resource::compile("assets/lovelycat.rc", embed_resource::NONE)
        .manifest_required()
        .expect("unable to embed the Lovelycat application icon");
}
