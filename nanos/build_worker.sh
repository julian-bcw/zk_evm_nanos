#RUSTFLAGS="-g -C link-arg=-static -C target-feature=-avx512f,-avx512dq,-avx512cd,-avx512bw,-avx512vl,-avx512ifma" cargo build --bin worker --target x86_64-unknown-linux-musl
CC=/usr/bin/gcc-10 RUSTFLAGS="-g -C target-feature=+crt-static,-avx512f,-avx512dq,-avx512cd,-avx512bw,-avx512vl,-avx512ifma" cargo build --bin worker --target x86_64-unknown-linux-gnu --release
