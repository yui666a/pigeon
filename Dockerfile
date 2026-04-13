# ==================================================
# Pigeon - Development Dockerfile
# ==================================================
# Tauri 2 (Rust + React/TypeScript) 開発環境
# ビルド・テスト用。GUIアプリの実行はホストOSで行う。
# ==================================================

# --- Stage 1: Frontend build ---
FROM node:24-bookworm-slim AS frontend-builder

WORKDIR /app

COPY package.json pnpm-lock.yaml ./
RUN corepack enable && pnpm install --frozen-lockfile

COPY src/ src/
COPY index.html vite.config.ts tsconfig*.json tailwind.config.* postcss.config.* ./
RUN pnpm run build

# --- Stage 2: Rust build ---
FROM rust:1.82-bookworm AS rust-builder

# Tauri 2 のビルドに必要なシステム依存
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev \
    libsqlite3-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app/src-tauri

# 依存クレートのキャッシュ
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo fetch \
    && rm -rf src

# ソースコードのコピーとビルド
COPY src-tauri/ ./
COPY --from=frontend-builder /app/dist ../dist

RUN cargo build --release

# --- Stage 3: Development image ---
FROM rust:1.82-bookworm AS development

RUN apt-get update && apt-get install -y --no-install-recommends \
    # Tauri 2 ビルド依存
    libwebkit2gtk-4.1-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev \
    libsqlite3-dev \
    pkg-config \
    # Node.js
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Node.js 24 のインストール
RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# pnpm + Rust ツール
RUN corepack enable \
    && rustup component add clippy rustfmt \
    && cargo install cargo-watch

WORKDIR /app

# cargo と pnpm の依存を先にインストール（キャッシュ活用）
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY src-tauri/Cargo.toml src-tauri/Cargo.lock* src-tauri/
RUN cd src-tauri && mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo fetch \
    && rm -rf src

COPY . .

CMD ["bash"]
