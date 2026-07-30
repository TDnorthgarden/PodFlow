# =============================================================================
# podflow RPM Spec
# =============================================================================
# 容器智能故障分析平台 — 基于 eBPF/NRI 的内核级观测与诊断
#
# 构建依赖:
#   - Rust 1.70+ (cargo, rustc)
#   - protobuf-compiler (protoc)
#   - gcc, make, openssl-devel (for tonic/reqwest)
#
# 产出 RPM:
#   podflow           — 主服务 + CLI + adapters + bpftrace 脚本
#   podflow-collector — 特权采集守护进程 (eBPF collector)
# =============================================================================

%define _version 0.1.0
%define _release 1%{?dist}
%define _github_org  TDnorthgarden
%define _github_repo PodFlow

# ---- 禁用 debug_package（Rust binary 无独立 debuginfo） ----
%global debug_package %{nil}

Name:           podflow
Version:        %{_version}
Release:        %{_release}
Summary:        Container intelligent fault diagnosis platform (eBPF/NRI based)
License:        MIT
URL:            https://github.com/%{_github_org}/%{_github_repo}
Source0:        %{name}-%{version}.tar.gz

# ---- 平台约束 ----
ExclusiveArch:  x86_64 aarch64

# ---- 构建依赖 ----
BuildRequires:  rust >= 1.70
BuildRequires:  cargo
BuildRequires:  protobuf-compiler
BuildRequires:  gcc
BuildRequires:  make
BuildRequires:  openssl-devel

Requires:       systemd

%description
PodFlow is an intelligent fault diagnosis platform for container
environments. It collects kernel-level observability data via eBPF/bpftrace,
generates diagnostic conclusions through a rule engine with AI enhancement,
and supports alert push notifications.

Core features:
  - 7 evidence types: Block I/O, Network, Syscall Latency, OOM, FS Stall,
    NRI Events, Cgroup Metrics
  - 5 diagnostic rule types: Threshold, Trend, Correlation, Statistical,
    Composite
  - NRI (Node Resource Interface) integration for containerd
  - Privilege separation architecture

# =============================================================================
# 子包: podflow-collector
# =============================================================================
%package -n podflow-collector
Summary:        Privileged eBPF collector daemon for podflow
Requires:       systemd
Requires:       bpftrace

%description -n podflow-collector
Privileged eBPF collector daemon that runs as root with Linux capabilities
(CAP_BPF, CAP_SYS_ADMIN, CAP_SYS_PTRACE, CAP_NET_ADMIN, CAP_IPC_LOCK) to
attach eBPF probes and collect kernel-level observability data.

# =============================================================================
# 准备阶段
# =============================================================================
%prep
%setup -q -n %{name}-%{version}

if [ ! -f proto/collector.proto ]; then
    echo "ERROR: proto/collector.proto not found"
    exit 1
fi

%build
cargo build --release --features nri-grpc

%install
# ---- 创建目录结构 ----
install -d %{buildroot}%{_bindir}
install -d %{buildroot}%{_sysconfdir}/%{name}
install -d %{buildroot}%{_sysconfdir}/nri/conf.d
install -d %{buildroot}%{_sharedstatedir}/%{name}
install -d %{buildroot}%{_localstatedir}/log/%{name}
install -d %{buildroot}%{_rundir}/%{name}
install -d %{buildroot}%{_datadir}/%{name}/bpftrace/templates
install -d %{buildroot}%{_datadir}/%{name}/bpftrace/adapters
install -d %{buildroot}%{_datadir}/%{name}/bpftrace/block_io
install -d %{buildroot}%{_datadir}/%{name}/bpftrace/network
install -d %{buildroot}%{_unitdir}

# ---- 安装二进制 ----
install -m 0755 target/release/podflow                  %{buildroot}%{_bindir}/podflow
install -m 0755 target/release/podflow-cli              %{buildroot}%{_bindir}/podflow-cli
install -m 0755 target/release/podflow-adapters         %{buildroot}%{_bindir}/podflow-adapters
install -m 0755 target/release/podflow-collector        %{buildroot}%{_bindir}/podflow-collector

# ---- 安装配置文件 ----
install -m 0644 config.yaml %{buildroot}%{_sysconfdir}/%{name}/config.yaml

# ---- 安装 NRI 插件配置 ----
install -m 0644 deploy/nri/podflow-nri.toml             %{buildroot}%{_sysconfdir}/nri/conf.d/podflow-nri.toml
install -m 0644 deploy/nri/99-podflow-nri.conf          %{buildroot}%{_sysconfdir}/nri/conf.d/99-podflow-nri.conf

# ---- 安装 bpftrace 脚本 ----
cp -r scripts/bpftrace/templates/*.bt  %{buildroot}%{_datadir}/%{name}/bpftrace/templates/
cp -r scripts/bpftrace/adapters/*.yaml %{buildroot}%{_datadir}/%{name}/bpftrace/adapters/
cp -r scripts/bpftrace/block_io/*.bt   %{buildroot}%{_datadir}/%{name}/bpftrace/block_io/
cp -r scripts/bpftrace/network/*.bt    %{buildroot}%{_datadir}/%{name}/bpftrace/network/

# ---- 安装 systemd 单元 ----
install -m 0644 systemd/podflow.service                 %{buildroot}%{_unitdir}/podflow.service
install -m 0644 systemd/podflow-collector.service       %{buildroot}%{_unitdir}/podflow-collector.service

# =============================================================================
# 文件列表 — 主包
# =============================================================================
%files
%doc README.md
%license LICENSE
%{_bindir}/podflow
%{_bindir}/podflow-cli
%{_bindir}/podflow-adapters
%dir %{_sysconfdir}/%{name}
%config(noreplace) %{_sysconfdir}/%{name}/config.yaml
%dir %{_sysconfdir}/nri/conf.d
%config(noreplace) %{_sysconfdir}/nri/conf.d/podflow-nri.toml
%config(noreplace) %{_sysconfdir}/nri/conf.d/99-podflow-nri.conf
%dir %{_sharedstatedir}/%{name}
%dir %{_localstatedir}/log/%{name}
%dir %{_rundir}/%{name}
%{_datadir}/%{name}/bpftrace/
%{_unitdir}/podflow.service

# =============================================================================
# 文件列表 — collector 子包
# =============================================================================
%files -n podflow-collector
%{_bindir}/podflow-collector
%{_unitdir}/podflow-collector.service

# =============================================================================
# 安装前脚本
# =============================================================================
%pre
getent group podflow >/dev/null || groupadd -r podflow
getent passwd podflow >/dev/null || useradd -r -g podflow -d %{_sharedstatedir}/%{name} -s /sbin/nologin -c "PodFlow" podflow
exit 0

# =============================================================================
# 安装后脚本
# =============================================================================
%post
%systemd_post podflow.service

%post -n podflow-collector
%systemd_post podflow-collector.service

# =============================================================================
# 卸载前脚本
# =============================================================================
%preun
%systemd_preun podflow.service

%preun -n podflow-collector
%systemd_preun podflow-collector.service

# =============================================================================
# 卸载后脚本
# =============================================================================
%postun
%systemd_postun_with_restart podflow.service

%postun -n podflow-collector
%systemd_postun_with_restart podflow-collector.service

# =============================================================================
# Changelog
# =============================================================================
%changelog
* Wed Jun 17 2026 TDnorthgarden <tdnorthgarden@gmail.com> - 0.1.0-1
- Initial RPM packaging
- Renamed from nuts-observer to podflow
- Split into podflow (main service + CLI) and podflow-collector (privileged collector)
- Include bpftrace scripts, NRI plugin configs, systemd units
