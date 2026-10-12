# Linux lab image: the Rust toolchain the docs are pinned to, plus the tools
# the labs use to look inside the kernel boundary (strace, /proc, ss, tcpdump).
FROM rust:1.95-bookworm

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      strace gdb procps psmisc iproute2 iputils-ping dnsutils \
      netcat-openbsd tcpdump curl less man-db manpages manpages-dev \
 && rm -rf /var/lib/apt/lists/*

# The labs are linted inside the container too; the base image ships without clippy.
RUN rustup component add clippy

WORKDIR /work/labs
