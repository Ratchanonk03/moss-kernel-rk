#!/usr/bin/env bash
set -o pipefail

rustc --version
cargo --version
aarch64-none-elf-gcc --version
qemu-system-aarch64 --version

just run 2>&1 | tee boot.log