fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("protoc is required");
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }
    prost_build::Config::new()
        .bytes([".spm.cloud.v1.Envelope.payload"])
        .compile_protos(&["proto/spm/cloud/v1/cloud.proto"], &["proto"])
        .expect("failed to compile Cloud protocol");
    println!("cargo:rerun-if-changed=proto/spm/cloud/v1/cloud.proto");
}
