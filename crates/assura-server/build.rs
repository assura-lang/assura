fn main() -> Result<(), Box<dyn std::error::Error>> {
    // SAFETY: build scripts are single-threaded; no other thread reads PROTOC.
    unsafe {
        std::env::set_var(
            "PROTOC",
            protoc_bin_vendored::protoc_bin_path().expect("protoc-bin-vendored provides protoc"),
        );
    }
    tonic_prost_build::compile_protos("proto/assura.proto")?;
    Ok(())
}
