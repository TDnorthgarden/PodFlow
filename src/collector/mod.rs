pub mod block_io;
pub mod network;
pub mod nri_mapping;
pub mod nri_mapping_v2;
pub mod nri_version;
pub mod nri_persist;
pub mod nri_socket;
pub mod nri_batch;
pub mod nri_grpc;
pub mod nri_v3;

// Containerd NRI 官方协议实现 (仅在启用 nri-grpc feature 时可用)
#[cfg(feature = "nri-grpc")]
pub mod nri_containerd;

pub mod syscall_latency;
pub mod fs_stall;
pub mod oom_events;
pub mod cgroup_contention;
pub mod bpftrace_adapter;
pub mod permission;
pub mod collector_client;

// 引入 protobuf 生成的代码 (仅在启用 nri-grpc feature 时可用)
#[cfg(feature = "nri-grpc")]
pub mod proto {
    tonic::include_proto!("nuts.collector");
    pub mod nri {
        tonic::include_proto!("nri.plugin.v1");
    }
}

