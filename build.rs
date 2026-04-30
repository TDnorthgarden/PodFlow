use std::io::Result;

fn main() -> Result<()> {
    // 编译 protobuf 文件到标准 OUT_DIR
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/collector.proto"], &["proto"])?;

    // 告诉 cargo 当 proto 文件变化时重新运行
    println!("cargo:rerun-if-changed=proto/collector.proto");

    Ok(())
}
