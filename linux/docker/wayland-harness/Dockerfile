FROM rust:1.93-slim-bookworm

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        dbus \
        libwayland-dev \
        libx11-dev \
        libxfixes-dev \
        pkg-config \
        sway \
        wl-clipboard \
    && useradd -m -s /bin/sh ccvv \
    && ln -sf /usr/local/cargo/bin/cargo /usr/local/bin/cargo \
    && ln -sf /usr/local/cargo/bin/rustc /usr/local/bin/rustc \
    && ln -sf /usr/local/cargo/bin/rustup /usr/local/bin/rustup \
    && ln -sf /usr/local/cargo/bin/rustdoc /usr/local/bin/rustdoc \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

COPY linux/docker/wayland-harness/entrypoint.sh /usr/local/bin/wayland-harness-entrypoint
COPY linux/docker/wayland-harness/sway.conf /etc/ccvv/sway.conf

RUN chmod +x /usr/local/bin/wayland-harness-entrypoint

ENTRYPOINT ["/usr/local/bin/wayland-harness-entrypoint"]
