#!/usr/bin/env just --justfile

create-image:
    ./scripts/create-image.sh

run:
    #!/usr/bin/env sh
    # Create moss.img if it doesn't exist
    if [ ! -f moss.img ]; then
    just create-image
    fi
    cargo run --release -- --init /bin/ash

run-rr:
    #!/usr/bin/env sh
    # Create moss.img if it doesn't exist
    if [ ! -f moss.img ]; then
    just create-image
    fi
    cargo run --release --features sched-rr  -- --init /bin/ash

benchmark:
    #!/usr/bin/env sh
    # Create moss.img if it doesn't exist
    if [ ! -f moss.img ]; then
    just create-image
    fi
    cargo run --release --no-default-features -- --init /bin/usertest --smp 1 2>&1 | tee benchmark.log

benchmark-rr:
    #!/usr/bin/env sh
    # Create moss.img if it doesn't exist
    if [ ! -f moss.img ]; then
    just create-image
    fi
    cargo run --release --no-default-features --features sched-rr  -- --init /bin/usertest --smp 1 2>&1 | tee benchmark-rr.log

test-unit:
    #!/usr/bin/env sh
    host_target="$(rustc --version --verbose | awk -F': ' '/^host:/ {print $2; exit}')"
    cargo test --package libkernel --target "$host_target"

test-kunit:
    cargo test --release

test-userspace:
    cargo run -r -- --init /bin/usertest

verify:
    ./scripts/verify_toolchain.sh
