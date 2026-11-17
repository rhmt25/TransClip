fn main() {
    println!("cargo:rerun-if-changed=assets");
    if let Err(e) =
        embed_resource::compile("assets/app.rc", embed_resource::NONE).manifest_required()
    {
        panic!("cannot embed the manifest and icons: {e}");
    }
}
