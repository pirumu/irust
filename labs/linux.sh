#!/usr/bin/env bash
# Run a command inside the Linux lab container, with the repository mounted at /work.
#   ./labs/linux.sh                              -> interactive shell
#   ./labs/linux.sh cargo run -p lab-linux    -> one command
# SYS_PTRACE lets strace/gdb trace processes; NET_ADMIN/NET_RAW let tcpdump and ss see sockets.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
image="rust-foundations-lab:1.95"
# Always build: with the layer cache this is near-instant, and it keeps the image
# in step with labs/Dockerfile.
docker build -q -t "$image" "$repo/labs" >/dev/null

tty_flags=(-i)
[ -t 0 ] && tty_flags+=(-t)

exec docker run --rm "${tty_flags[@]}" \
  --cap-add SYS_PTRACE --cap-add NET_ADMIN --cap-add NET_RAW \
  --security-opt seccomp=unconfined \
  -v "$repo":/work \
  -v rust-foundations-cargo:/usr/local/cargo/registry \
  -e CARGO_TARGET_DIR=/work/labs/target-linux \
  -w /work/labs \
  "$image" "${@:-bash}"
