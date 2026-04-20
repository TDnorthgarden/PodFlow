pub mod block_io;
pub mod network;
pub mod nri_mapping;
pub mod syscall_latency;
pub mod fs_stall;
pub mod oom_events;
pub mod cgroup_contention;
pub mod bpftrace_adapter;
pub mod permission;
pub mod collector_client;

// 引入 protobuf 生成的代码
pub mod proto {
    tonic::include_proto!("nuts.collector");
}

