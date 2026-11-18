# syntax=docker/dockerfile:1
#
# The owl links liblogosdelivery, which comes from libchat's Nix flake. The
# build stage runs inside libchat's own dev shell at the rev pinned in
# Cargo.lock, so the toolchain and native library match what libchat expects.
# The runtime stage gets the binary plus only the Nix store paths it references.

FROM nixos/nix:latest AS build
RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf
WORKDIR /src

# Warm the dev shell (toolchain + liblogosdelivery) in its own layer: it only
# changes when the libchat rev in Cargo.lock does.
COPY Cargo.lock ./
# (The nix image has no sed or awk.)
RUN rev=$(grep -A2 'name = "logos-chat"' Cargo.lock | grep '^source = ' | cut -d'#' -f2 | tr -d '"') \
 && test -n "$rev" && echo "$rev" > /libchat-rev \
 && nix develop --accept-flake-config "github:logos-messaging/libchat/$rev" -c true

COPY . .
# .git is not in the build context; pass flock's commit in.
ARG OWL_REV=unknown
ENV OWL_REV=$OWL_REV
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/src/target \
    nix develop --accept-flake-config "github:logos-messaging/libchat/$(cat /libchat-rev)" \
      -c cargo build --release --locked -p owl \
 && cp target/release/owl /owl

# The binary names its interpreter and liblogosdelivery by absolute store
# path; copy exactly those closures.
RUN mkdir -p /out/nix/store \
 && grep -ao '/nix/store/[a-z0-9]\{32\}-[^/[:space:]]*' /owl | sort -u \
    | xargs nix-store --query --requisites \
    | xargs -I{} cp -a {} /out/nix/store/

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/nix /nix
COPY --from=build /owl /usr/local/bin/owl
# The owl keeps its instance ID (./instance) and logos-delivery its message
# store (./data) here. A volume, so a recreated container keeps both: compose
# hands the old container's anonymous volume to its replacement, so the owl
# keeps its identity. Never share one volume between owls.
WORKDIR /var/lib/owl
VOLUME /var/lib/owl
ENTRYPOINT ["owl"]
