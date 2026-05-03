# ── Stage 1: Build web surfaces for the community server image ───────────────
FROM node:20-bookworm AS web-builder

WORKDIR /web

COPY frontend_app ./frontend_app
COPY frontend_admin ./frontend_admin

RUN cd frontend_app && npm install && npm run build
RUN cd frontend_admin && npm install && npm run build

# ── Stage 2: Build backend ───────────────────────────────────────────────────
FROM rust:slim-bookworm AS builder

# aws-lc-sys needs cmake, nasm (x86) or clang, plus pkg-config
RUN apt-get update && apt-get install -y --no-install-recommends \
    make pkg-config cmake nasm clang libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Build the backend crate directly so the image does not depend on the full
# workspace layout being present in the Docker build context.
COPY Cargo.lock ./Cargo.lock
COPY backend/Cargo.toml ./backend/

# Seed a dummy binary so cargo can pre-compile all dependencies
RUN mkdir -p backend/src && echo 'fn main(){}' > backend/src/main.rs
RUN cargo build --release --manifest-path backend/Cargo.toml --target-dir /app/target || true

# Now copy the real source and do the final build
COPY backend/src ./backend/src
# Touch main.rs so cargo knows the source changed
RUN touch backend/src/main.rs
RUN cargo build --release --manifest-path backend/Cargo.toml --target-dir /app/target

# ── Stage 3: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/backend /app/backend
COPY --from=web-builder /web/frontend_app/dist /app/frontend_app_dist
COPY --from=web-builder /web/frontend_admin/dist /app/frontend_admin_dist

EXPOSE 8090

# Default to listening on all interfaces inside the container
ENV HOST=0.0.0.0
ENV PORT=8090
ENV SERVER_APP_DIST_DIR=/app/frontend_app_dist
ENV SERVER_ADMIN_DIST_DIR=/app/frontend_admin_dist

ENTRYPOINT ["/app/backend"]
