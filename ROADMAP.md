# Nimbis Roadmap

This document outlines the development roadmap for **Nimbis**, a Redis-compatible database built with Rust and object storage.

**Note:** The project is currently in active development at version **v0.1.0**. We will maintain this version number while iterating through the following phases until the project reaches a significant level of stability and feature completeness.

## 🌟 Vision

Nimbis aims to be a cloud-native, cost-effective alternative to Redis for datasets that exceed memory limits, leveraging modern object storage (S3, GCS, Azure Blob) for persistence without compromising API compatibility.

## 📍 Current Status

- **Version**: `v0.1.0`
- **Core**: RESP protocol implementation, async server architecture.
- **Storage**: Basic object storage backend via `SlateDB`.
- **Commands**: `GET`, `SET`, `DEL`, `PING`, `CONFIG GET`, `CONFIG SET`.
- **Data Types**: String, Hash (partial).

## 🗺️ Development Phases

### Phase 1: Core Commands & Expiry (Priority)

The goal of this phase is to make Nimbis usable for basic caching scenarios.

- **String Operations**:
  - [x] `DEL`
  - [ ] `EXISTS`
  - [ ] `MGET` / `MSET`
  - [ ] `INCR` / `DECR`
  - [ ] `APPEND`
- **Key Expiration (TTL)**:
  - [ ] `EXPIRE` / `EXPIREAT`
  - [ ] `TTL` / `PTTL`
  - [ ] Background expiration mechanism in storage layer

### Phase 2: Advanced Data Structures

Expand utility beyond simple key-value pairs.

- **Lists**:
  - [ ] `LPUSH`, `RPUSH`
  - [ ] `LPOP`, `RPOP`
  - [ ] `LRANGE`
- **Hashes**:
  - [x] `HSET`, `HGET`, `HGETALL`
  - [x] `HMGET`, `HLEN`
  - [ ] `HDEL`
- **Sets**:
  - [ ] `SADD`, `SMEMBERS`, `SISMEMBER`

### Phase 3: Production Readiness

Focus on stability, configurability, and observability.

- **Configuration**:
  - [ ] Support for configuration file (`nimbis.conf`)
  - [ ] Environment variable overrides
  - [ ] SlateDB tuning options
- **Observability**:
  - [ ] Prometheus metrics endpoint
  - [ ] Structured logging improvements
  - [ ] Slow query log
- **Security**:
  - [ ] Basic `AUTH` support
  - [ ] TLS support

### Phase 4: Cloud Native Features

Leverage the unique architecture of Nimbis.

- **Storage Backends**:
  - [ ] S3 / MinIO support verification
  - [ ] GCS / Azure Blob support
- **Advanced Features**:
  - [ ] Data tiering (Hot/Warm/Cold)
  - [ ] Serverless deployment guides
