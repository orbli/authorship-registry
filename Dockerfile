# One Dockerfile for every app in the workspace: `docker build --build-arg APP=web .`
# Built natively per architecture in CI (amd64 and arm64 runners), then merged into
# one multi-arch tag. Cross-compiling under QEMU would take an hour; native is minutes.
FROM rust:1-bookworm AS build
ARG APP=web
WORKDIR /repo
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/repo/target,sharing=locked \
    cargo build --release --locked -p authorship-registry-${APP} --bin ${APP} \
 && cp /repo/target/release/${APP} /app-bin

FROM debian:bookworm-slim
ARG APP=web
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --no-create-home app
ENV PORT=3000
WORKDIR /app
COPY --from=build /app-bin /app/server
USER app
EXPOSE 3000
CMD ["/app/server"]
