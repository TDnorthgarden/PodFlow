use std::io::Result;

fn main() -> Result<()> {
    // 编译 collector protobuf 文件
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/collector.proto"], &["proto"])?;

    // 编译 NRI protobuf 文件（containerd 官方协议）
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&["proto/nri.proto"], &["proto"])?;

    // 告诉 cargo 当 proto 文件变化时重新运行
    println!("cargo:rerun-if-changed=proto/collector.proto");
    println!("cargo:rerun-if-changed=proto/nri.proto");

    Ok(())
}
