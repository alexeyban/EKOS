# EKOS production image (RFC 0179): the `ekos` binary, the console API and the built console UI
# in one container, serving on port 8000. State lives in one volume, the EKOS home.
#
#   docker build -t ekos .
#   docker run -p 127.0.0.1:8000:8000 -v ekos-home:/var/lib/ekos -v /srv/repos:/sources:ro ekos
#
# `deploy/docker-compose.yml` is the supported way to run it. `Dockerfile.dev` is a build shell
# for a machine without Rust, and `web/docker-compose.yml` is the console's development setup;
# neither is this.
#
# All three stages use the same Debian release: a binary linked against a newer glibc than the
# runtime has does not start.

# ── 1. The binary ─────────────────────────────────────────────────────────────
# The version is the one `rust-toolchain.toml` pins. That file is copied in, so rustup fetches
# whatever it asks for that the base image lacks: the components it lists, and the pinned
# compiler itself if the two versions ever differ, rather than building with the wrong one.
FROM rust:1.98-slim-bookworm AS binary

# A C compiler for the workspace's C-backed crates (bundled SQLite, zstd). Nothing else: TLS is
# rustls, so there is no OpenSSL to link.
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY rust-toolchain.toml ./
COPY ekos ./ekos

# Lower this on a machine short of memory: each parallel rustc job can take over a gigabyte.
ARG CARGO_BUILD_JOBS=""

# Default features only, as in release.yml: the `distributed` feature must never be in a stock
# build (RFC 0113).
#
# No BuildKit cache mounts, deliberately: a distribution's own Docker package often ships
# without the buildx component (Ubuntu's `docker.io` does), and `RUN --mount` then fails the
# build outright. This file builds with the classic builder and with BuildKit alike. The price
# is a full compile whenever anything under ekos/ changes.
#
# A build arg is also an environment variable inside RUN, and cargo reads CARGO_BUILD_JOBS itself:
# an empty one fails with "could not parse ``", so it is unset when not given.
RUN cd ekos \
    && if [ -z "$CARGO_BUILD_JOBS" ]; then unset CARGO_BUILD_JOBS; fi \
    && cargo build --release --locked -p ekos ${CARGO_BUILD_JOBS:+--jobs "$CARGO_BUILD_JOBS"} \
    && cp target/release/ekos /ekos-bin

# ── 2. The console UI ─────────────────────────────────────────────────────────
FROM node:20-bookworm-slim AS ui

WORKDIR /ui
COPY web/ui/package.json web/ui/package-lock.json ./
RUN npm ci
COPY web/ui ./
RUN npm run build

# ── 3. Runtime ────────────────────────────────────────────────────────────────
FROM python:3.12-slim-bookworm

# `git`: the git observer, the freshness check and `semantics` blame all call it.
RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    # Sources are mounted from the host and belong to another user id, which git refuses to
    # read ("dubious ownership"). Every repository in this container is one the operator mounted.
    && git config --system --add safe.directory '*'

COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

# The console finds the UI at ../../ui/dist relative to its own package (app/main.py).
WORKDIR /opt/ekos/api
COPY web/api/pyproject.toml web/api/uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project --no-build
COPY web/api/app ./app
COPY --from=ui /ui/dist /opt/ekos/ui/dist
COPY --from=binary /ekos-bin /usr/local/bin/ekos
COPY LICENSE /opt/ekos/LICENSE

# Never root. The home is created and owned here so a new named volume mounted over it starts
# with the same owner.
RUN useradd --create-home --uid 1000 ekos \
    && mkdir -p /var/lib/ekos/data /var/lib/ekos/control /sources \
    && chown -R ekos:ekos /var/lib/ekos
USER ekos

ENV PATH="/opt/ekos/api/.venv/bin:$PATH" \
    PYTHONUNBUFFERED=1 \
    # The EKOS home: projects under data/, the console's database and run logs under control/.
    EKOS_HOME=/var/lib/ekos \
    EKOS_BIN=/usr/local/bin/ekos \
    # Projects may only be created for sources mounted here.
    EKOS_CONSOLE_WORKSPACES_ROOT=/sources \
    # RFC 0184: a container has no OS keychain; secrets come from the environment.
    EKOS_CONSOLE_KEYCHAIN=off \
    # The Compose restart policy starts the console again, so it may restart itself after a
    # settings change. Without a restart policy, the restart button stops the container.
    EKOS_CONSOLE_SUPERVISED=true

# Links the package on ghcr.io to the repository.
LABEL org.opencontainers.image.source="https://github.com/alexeyban/EKOS" \
      org.opencontainers.image.description="EKOS: knowledge compiler, console API and console UI" \
      org.opencontainers.image.licenses="MIT"

VOLUME /var/lib/ekos
EXPOSE 8000

# No curl in the slim image; the interpreter is already there.
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD ["python", "-c", "import urllib.request,sys; sys.exit(0 if urllib.request.urlopen('http://127.0.0.1:8000/api/health', timeout=4).status == 200 else 1)"]

CMD ["uvicorn", "app.main:create_app", "--factory", "--host", "0.0.0.0", "--port", "8000"]
