#!/bin/sh
#
# Runs the CI checks in a clean Ubuntu 24.04 container with only the packages the GitHub workflow
# installs, so a missing dependency or a difference from the runner shows up here first.
#
#   scripts/ci-docker.sh                 scripts/ci.sh (everything)
#   scripts/ci-docker.sh --quick         scripts/ci.sh --quick
#   NOTEY_DOCKER_CMD='xvfb-run -a cargo test --test headless_gtk -- session_scroll' scripts/ci-docker.sh
#                                        any other command, from the repository root
#
# The checkout is mounted read-only; cargo's cache and the build output live in named Docker volumes
# (notey-ci-cargo, notey-ci-rustup, notey-ci-target) so later runs are fast. `docker volume rm
# notey-ci-cargo notey-ci-rustup notey-ci-target` starts from scratch. It does not copy the runner's CPU speed or its display.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
image="${NOTEY_DOCKER_IMAGE:-ubuntu:24.04}"
cmd="${NOTEY_DOCKER_CMD:-sh scripts/ci.sh $*}"

# Runs as an ordinary user like the GitHub runner. As root, file permissions mean nothing, so the
# read-only-file tests would fail for a reason that does not exist on a real machine.
exec docker run --rm \
    -v "$root":/src:ro \
    -v notey-ci-cargo:/home/runner/.cargo \
    -v notey-ci-rustup:/home/runner/.rustup \
    -v notey-ci-target:/target \
    -e CARGO_TARGET_DIR=/target \
    -e DEBIAN_FRONTEND=noninteractive \
    -e HOST_UID="$(id -u)" \
    -e HOST_GID="$(id -g)" \
    -e NOTEY_CMD="$cmd" \
    -w /src \
    "$image" bash -ec '
        apt-get update -qq
        apt-get install -y -qq --no-install-recommends \
            build-essential pkg-config curl ca-certificates xvfb \
            libgtk-4-dev libadwaita-1-dev libgtksourceview-5-dev > /dev/null
        getent group "$HOST_GID" > /dev/null || groupadd -g "$HOST_GID" runner
        id -u runner > /dev/null 2>&1 || useradd -m -d /home/runner -u "$HOST_UID" -g "$HOST_GID" runner 2> /dev/null \
            || useradd -m -d /home/runner -g "$HOST_GID" runner
        mkdir -p /home/runner/.cargo /home/runner/.rustup /target
        chown -R runner:"$HOST_GID" /home/runner /target
        exec runuser -u runner -- env HOME=/home/runner CARGO_TARGET_DIR=/target NOTEY_CI_LOG=/target/ci-test.log NOTEY_CMD="$NOTEY_CMD" bash -ec "
            export PATH=/home/runner/.cargo/bin:\$PATH
            command -v rustup > /dev/null || curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none > /dev/null
            channel=\$(sed -n \"s/^channel *= *\\\"\\(.*\\\)\\\"/\\1/p\" rust-toolchain.toml)
            rustup toolchain install \$channel --profile minimal --component clippy
            rustc --version
            id -un
            sh -c \"\$NOTEY_CMD\"
        "
    '
