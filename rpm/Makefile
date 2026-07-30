# =============================================================================
# RPM build helpers for podflow
# =============================================================================

SPEC      := podflow.spec
VERSION   := 0.1.0
NAME      := podflow
TARBALL   := $(NAME)-$(VERSION).tar.gz
BUILDDIR  := $(shell pwd)/build
RPMDIR    := $(BUILDDIR)/rpms
SRPMDIR   := $(BUILDDIR)/srpms

# RPM 宏定义
RPM_DEFINES := --define "_topdir $(BUILDDIR)" \
               --define "_sourcedir $(BUILDDIR)/SOURCES" \
               --define "_specdir $(BUILDDIR)/SPECS" \
               --define "_builddir $(BUILDDIR)/BUILD" \
               --define "_srcrpmdir $(SRPMDIR)" \
               --define "_rpmdir $(RPMDIR)"

.PHONY: all clean srpm rpm tarball setup-dirs

all: rpm

# ---- 创建构建目录结构 ----
setup-dirs:
	@mkdir -p $(BUILDDIR)/{SOURCES,SPECS,BUILD,BUILDROOT,RPMS,SRPMS}
	@mkdir -p $(RPMDIR)/{x86_64,aarch64,noarch}

# ---- 制作源码 tar 包 ----
tarball: setup-dirs
	@echo "==> Creating source tarball from git working tree..."
	cd ../.. && tar czf $(BUILDDIR)/SOURCES/$(TARBALL) \
		--exclude='.git' \
		--exclude='target' \
		--exclude='rpm/build' \
		--exclude='*.pyc' \
		--exclude='__pycache__' \
		--transform 's,^PodFlow,$(NAME)-$(VERSION),' \
		PodFlow/
	@echo "==> Tarball: $(BUILDDIR)/SOURCES/$(TARBALL)"

# ---- 构建 SRPM ----
srpm: tarball
	@echo "==> Building SRPM..."
	cp $(SPEC) $(BUILDDIR)/SPECS/
	rpmbuild -bs $(RPM_DEFINES) $(BUILDDIR)/SPECS/$(SPEC)
	@echo "==> SRPM: $(SRPMDIR)/$(NAME)-$(VERSION)*.src.rpm"

# ---- 构建 RPM（从源码编译） ----
rpm: tarball
	@echo "==> Building RPM (this will compile Rust project from source)..."
	cp $(SPEC) $(BUILDDIR)/SPECS/
	rpmbuild -ba $(RPM_DEFINES) $(BUILDDIR)/SPECS/$(SPEC)
	@echo "==> RPMs:"
	@find $(RPMDIR) -name '*.rpm' -type f

# ---- 仅构建 RPM（跳过 SRPM，使用预编译的二进制） ----
rpm-fast: setup-dirs
	@echo "==> Building RPM with pre-compiled binaries..."
	@if [ ! -f ../target/release/podflow ]; then \
		echo "ERROR: Pre-compiled binaries not found."; \
		echo "Run 'cargo build --release --features nri-grpc' first."; \
		exit 1; \
	fi
	cp $(SPEC) $(BUILDDIR)/SPECS/
	rpmbuild -bb --nocheck $(RPM_DEFINES) $(BUILDDIR)/SPECS/$(SPEC)
	@echo "==> RPMs:"
	@find $(RPMDIR) -name '*.rpm' -type f

# ---- 清理 ----
clean:
	rm -rf $(BUILDDIR)
	@echo "==> Cleaned build artifacts"

# ---- 安装本地构建的 RPM（测试用） ----
install: rpm
	@echo "==> Installing RPMs locally..."
	sudo rpm -ivh $(RPMDIR)/x86_64/$(NAME)-$(VERSION)*.rpm --replacepkgs || true
	sudo rpm -ivh $(RPMDIR)/x86_64/podflow-collector-$(VERSION)*.rpm --replacepkgs || true

# ---- 卸载 ----
uninstall:
	sudo rpm -e podflow-collector || true
	sudo rpm -e podflow || true

# ---- 显示帮助 ----
help:
	@echo "Usage:"
	@echo "  make tarball    - Create source tarball"
	@echo "  make srpm       - Build source RPM only"
	@echo "  make rpm        - Build RPM from source (full compilation)"
	@echo "  make rpm-fast   - Build RPM using pre-compiled binaries"
	@echo "  make install    - Build and install RPMs locally"
	@echo "  make uninstall  - Remove installed RPMs"
	@echo "  make clean      - Remove build artifacts"