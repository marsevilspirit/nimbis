# SlateDB Redis Data Types Implementation Design

This document outlines the design for implementing Redis data types on top of SlateDB's key-value store.

## Overview

SlateDB is a key-value store. To support Redis data types (String, Hash, List, Set, ZSet), we use a multi-engine architecture where each data type is stored in its own dedicated SlateDB instance. This provides isolation and simplifies key management by removing the need for type prefixes in keys.

## Storage Architecture

We maintain separate SlateDB instances for:
- **Meta**: Metadata storage
- **String**: String data type storage
- **Hash**: Hash data type storage
- **Set**: Set data type storage
- **List**: List data type storage
- **ZSet**: ZSet data type storage

Each instance manages its own key space, so keys do not need type prefixes to avoid collisions between types.

## Encoding Scheme

### 1. Meta Key

The Meta Key is used to store metadata for a specific user key and data type (e.g., expiration, encoding info, item count).

**Key Format:**
```
user_key
```

**Value Format:**
Variable based on data `type` (stored in the metadata value itself or implied by the DB instance if strict separation is used).

### 2. String Type

**Key Format:**
```
user_key
```

*   **User Key**: The original key provided by the client (bytes).

**Value Format:**
```
raw_value_bytes
```

*   **Value**: The raw binary data of the string value.

**Example:**
*   Redis Command: `SET mykey "hello"`
*   SlateDB String DB Key: `[109, 121, 107, 101, 121]` (bytes for 'mykey')
*   SlateDB String DB Value: `[104, 101, 108, 108, 111]` (bytes for "hello")

---


### 3. Hash Type

Implemented using `Hash DB` for data and `Meta DB` for metadata.

**Meta Key (in Meta DB):**
```
user_key
```
*   **Value Format**: `type_code` (u8, 'h') + `count` (u64 BigEndian)

**Field Key (in Hash DB):**
```
user_key + length(field) (u32 BigEndian) + field
```
*   **Value Format**: `raw_value_bytes`

**Example:**
*   Redis Command: `HSET myhash field1 value1`
*   **Meta DB**: Key=`myhash`, Value=`'h'` + `1`
*   **Hash DB**: Key=`myhash` + `0x00 0x00 0x00 0x06` + `field1`, Value=`value1`

---

## Future Implementations (Tentative)

The following designs are placeholders and subject to change.

### Set (in Set DB)
*   **Meta Key**: `user_key`
*   **Member Key**: `user_key` + `member` -> `(empty)`

### List (in List DB)
*   **Meta Key**: `user_key` -> Metadata
*   **Node Key**: `user_key` + `seq_id` -> `value`

### ZSet (in ZSet DB)
*   **Meta Key**: `user_key`
*   **Member Key**: `user_key` + `member` -> `score`
*   **Score Key**: `user_key` + `score` + `member` -> `(empty)`
