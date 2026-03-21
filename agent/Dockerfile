# === Builder Stage ===
FROM rust:1.75-slim as builder

WORKDIR /app

# Копируем манифесты зависимостей (для кэширования слоёв)
COPY Cargo.toml Cargo.lock ./

# Копируем исходники
COPY src/ ./src/

# Собираем релиз-версию
RUN cargo build --release --bin monsterfi-executor

# === Runtime Stage ===
FROM debian:bookworm-slim

# Устанавливаем CA-сертификаты (нужны для HTTPS запросов)
RUN apt-get update && apt-get install -y ca-certificates openssl && rm -rf /var/lib/apt/lists/*

# Копируем бинарник из builder
COPY --from=builder /app/target/release/monsterfi-executor /usr/local/bin/

WORKDIR /app

# Копируем .env (опционально, лучше передавать через --env-file)
COPY .env .env

# Переменные окружения по умолчанию
ENV RUST_LOG=info

# Точка входа
CMD ["monsterfi-executor", "--strategy", "macd_rsi", "--leverage", "20", "--testnet"]
