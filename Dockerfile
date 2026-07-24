FROM rust:1.97.1-trixie AS rust-builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && printf 'fn main() {}\n' > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:trixie-slim AS harness-builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       gcc-arm-linux-gnueabi g++-arm-linux-gnueabi make \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
COPY harness ./
# Link checks need the private bionic libraries. They are mounted only at
# runtime, so provide link-only ABI stubs for the harness's direct libraries.
RUN printf 'void getTtsEngine(void) {}\n' > engine-stub.c \
    && arm-linux-gnueabi-gcc -shared -fPIC -Wl,-soname,libTtsToSpeakG3V1_JP.so \
       -o libTtsToSpeakG3V1_JP.so engine-stub.c \
    && printf 'int open(const char*a,int b,...){return 0;} int close(int a){return 0;} int read(int a,void*b,unsigned c){return 0;} int write(int a,const void*b,unsigned c){return 0;} unsigned strlen(const char*a){return 0;} int raise(int a){return 0;}\n' > libc-stub.c \
    && printf 'void placeholder(void) {}\n' > android-stub.c \
    && arm-linux-gnueabi-gcc -shared -fPIC -nostdlib -Wl,-soname,libc.so -o libc.so libc-stub.c \
    && for lib in libdl.so libm.so libstdc++.so; do \
         arm-linux-gnueabi-gcc -shared -fPIC -Wl,-soname,$lib -o $lib android-stub.c; \
       done \
    && make ROOTFS=/build/stub-root clean || true
# Extracted libraries are deliberately unavailable to the image build. Link
# against generated ABI stubs that preserve the runtime SONAMEs instead.
RUN mkdir -p stub-root/system/lib \
    && cp libTtsToSpeakG3V1_JP.so libc.so libdl.so libm.so libstdc++.so stub-root/system/lib/ \
    && make ROOTFS=/build/stub-root all

FROM debian:trixie-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends qemu-user-static curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 tts \
    && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin tts
COPY --from=rust-builder /build/target/release/tts-api /usr/local/bin/tts-api
COPY --from=harness-builder /build/tts-harness /usr/local/bin/tts-harness
USER 10001:10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/tts-api"]
