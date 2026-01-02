# SlateDB Redis Data Types Implementation Design

This document outlines the design for implementing Redis data types on top of SlateDB's key-value store.

## Overview

SlateDB is a key-value store. To support Redis data types (String, Hash, List, Set, ZSet), we use a multi-engine architecture where each data type is stored in its own dedicated SlateDB instance. This provides isolation and simplifies key management by removing the need for type prefixes in keys.

## Storage Architecture

We maintain separate SlateDB instances for:
- **String (and Meta)**: Stores String values and Metadata for other types (Hash, Set, etc.)
- **Hash**: Hash fields storage
- **Set**: Set members storage
- **List**: List nodes storage
- **ZSet**: ZSet nodes storage

Each supported data type has a **Type Code** stored in the `String DB` key-value pair to identify the type and allow collision detection.

## Encoding Scheme

### 1. Root Key (in String DB)

All keys start in the `String DB`, which acts as the source of truth for the key's type.

**Key Format:**
```
user_key
```

**Value Format:**
```
[Type Code (u8)] [Payload (Bytes)]
```
*   **Type Code**: `s` (String), `h` (Hash), etc.

### 2. String Type

**Stored in String DB.**

**Value Format:**
```
['s'] [raw_value_bytes]
```

**Example:**
*   Redis Command: `SET mykey "hello"`
*   String DB Key: `mykey`
*   String DB Value: `['s', 'h', 'e', 'l', 'l', 'o']`

### 3. Hash Type

**Meta Stored in String DB, Fields in Hash DB.**

**Meta (String DB):**
*   **Key**: `user_key`
*   **Value**: `['h']` + `[count (u64 BE)]` // HashMetaValue (Currently len is u64)

**Fields (Hash DB):**
*   **Key**: `user_key + length(field) (u32 BigEndian) + field`
*   **Value**: `raw_value_bytes`

**Example:**
*   Redis Command: `HSET myhash field1 value1`
*   **String DB**: Key=`myhash`, Value=`['h']` + `1`
*   **Hash DB**: Key=`myhash` + `...field1...`, Value=`value1`

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
