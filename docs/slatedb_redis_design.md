# SlateDB Redis Data Types Implementation Design

This document outlines the design for implementing Redis data types on top of SlateDB's key-value store.

## Overview

SlateDB is a key-value store. To support Redis data types (String, Hash, List, Set, ZSet), we use a multi-engine architecture where each data type is stored in its own dedicated SlateDB instance. This provides isolation and simplifies key management by removing the need for type prefixes in keys.

## Storage Architecture

We maintain separate SlateDB instances for:
- **String (and Meta)**: Stores String values and Metadata for other types (Hash, List, Set, ZSet)
- **Hash**: Hash fields storage
- **List**: List elements storage
- **Set**: Set members storage
- **ZSet**: Sorted Set members and score indices storage

Each supported data type has a **Type Code** stored in the `String DB` key-value pair to identify the type and allow collision detection.

## Encoding Scheme

### 1. Root Key (in String DB)

All keys start in the `String DB`, which acts as the source of truth for the key's type.

**Key Format:**
```
[len(user_key) (u16 BE)] [user_key]
```
*   The key is length-prefixed with a 16-bit big-endian length to prevent prefix collisions.

**Value Format:**
```
[Type Code (u8)] [Payload (Bytes)]
```
*   **Type Code**: `s` (String), `h` (Hash), etc.

### 2. String Type

**Stored in String DB.**

**Value Format:**
```
['s'] [expire_time (u64 BE)] [raw_value_bytes]
```
*   **expire_time**: 8 bytes, milliseconds since epoch. `0` means no expiration.

**Example:**
*   Redis Command: `SET mykey "hello"`
*   String DB Key: `\x00\x05mykey` (5-byte length prefix + "mykey")
*   String DB Value: `['s', 0, 0, 0, 0, 0, 0, 0, 0, 'h', 'e', 'l', 'l', 'o']` (Example with no TTL)


### 3. Hash Type

**Meta Stored in String DB, Fields in Hash DB.**

**Meta (String DB):**
*   **Key**: `[len(user_key) (u16 BE)]` + `user_key`
*   **Value**: `['h']` + `[count (u64 BE)]` + `[expire_time (u64 BE)]`
*   **Payload**: 1 byte type code + 8 bytes field count + 8 bytes expiration timestamp.

**Fields (Hash DB):**
*   **Key**: `[len(user_key) (u16 BE)]` + `user_key` + `[len(field) (u32 BE)]` + `field`
*   **Value**: `raw_value_bytes`

**Example:**
*   Redis Command: `HSET myhash field1 value1`
*   **String DB**: Key=`\x00\x06myhash`, Value=`['h']` + `1`
*   **Hash DB**: Key=`\x00\x06myhash\x00\x00\x00\x06field1`, Value=`value1`

**Note on Expiration:** Both `StringValue` and `HashMetaValue` implement the `Expirable` trait (defined in `crates/storage/src/expirable.rs`), which provides a unified interface for managing TTL/expiration logic. This ensures consistent expiration behavior across different data types and eliminates code duplication.

### 4. List Type

**Meta Stored in String DB, Elements in List DB.**

**Meta (String DB):**
*   **Key**: `[len(user_key) (u16 BE)]` + `user_key`
*   **Value**: `['l']` + `[len (u64)]` + `[head (u64)]` + `[tail (u64)]` + `[expire_time (u64)]`
*   **Logic**: Implemented as a deque. `head` and `tail` start at `2^63` (middle of u64 range).
    *   `LPUSH`: Decrement `head`, store at new `head`.
    *   `RPUSH`: Store at `tail`, increment `tail`.
    *   Elements are in range `[head, tail)`.

**Elements (List DB):**
*   **Key**: `[len(user_key) (u16 BE)]` + `user_key` + `[seq (u64 BE)]`
*   **Value**: `raw_element_bytes`

**Example:**
*   Redis Command: `RPUSH mylist A`
*   **String DB**: Key=`\x00\x06mylist`, Meta=`len=1, head=mid, tail=mid+1`
*   **List DB**: Key=`\x00\x06mylist` + `mid`, Value=`A`

### 5. Set Type

**Meta Stored in String DB, Members in Set DB.**

**Meta (String DB):**
*   **Key**: `[len(user_key) (u16 BE)]` + `user_key`
*   **Value**: `['S']` + `[member_count (u64 BE)]` + `[expire_time (u64 BE)]`
*   **Payload**: 1 byte type code + 8 bytes member count + 8 bytes expiration timestamp.

**Members (Set DB):**
*   **Key**: `[len(user_key) (u16 BE)]` + `user_key` + `[len(member) (u32 BE)]` + `member`
*   **Value**: Empty (existence indicates membership)

**Example:**
*   Redis Command: `SADD myset member1`
*   **String DB**: Key=`\x00\x05myset`, Value=`['S']` + `1` + `expire_time`
*   **Set DB**: Key=`\x00\x05myset\x00\x00\x00\x08member1`, Value=`(empty)`

**Operations:**
*   `SADD`: Check/update metadata in String DB, add member key to Set DB
*   `SREM`: Update metadata count, delete member key from Set DB
*   `SMEMBERS`: Scan all keys in Set DB with prefix `user_key`
*   `SCARD`: Read count from metadata in String DB (O(1))

### 6. Sorted Set (ZSet) Type

**Meta Stored in String DB, Dual Index in ZSet DB.**

**Meta (String DB):**
*   **Key**: `[len(user_key) (u16 BE)]` + `user_key`
*   **Value**: `['z']` + `[member_count (u64 BE)]` + `[expire_time (u64 BE)]`
*   **Payload**: 1 byte type code + 8 bytes member count + 8 bytes expiration timestamp.

**Dual Index Structure (ZSet DB):**

1. **Member → Score Index:**
   *   **Key**: `[len(user_key) (u16 BE)]` + `user_key` + `'M'` + `[len(member) (u32 BE)]` + `member`
   *   **Value**: `score (f64)` stored as 8 bytes
   *   **Purpose**: Fast O(1) lookup of member score (for `ZSCORE`)

2. **Score → Member Index:**
   *   **Key**: `[len(user_key) (u16 BE)]` + `user_key` + `'S'` + `[encoded_score (u64 BE)]` + `member`
   *   **Value**: Empty
   *   **Purpose**: Ordered iteration by score (for `ZRANGE`)

**Score Encoding:**
*   Scores use **bit-flip encoding** to ensure correct byte-level sorting:
    - **Positive numbers**: Set sign bit: `bits | 0x8000_0000_0000_0000`
    - **Negative numbers**: Flip all bits: `!bits`
*   This maps the entire f64 range to ascending byte order:
    - Negative infinity → `0x0000...`
    - Negative numbers → `0x0000...` to `0x7FFF...`
    - Positive numbers → `0x8000...` to `0xFFFF...`
    - Positive infinity → `0xFFFF...`
*   The encoded u64 value is then stored in big-endian format

**Example:**
*   Redis Command: `ZADD myzset 1.5 member1`
*   **String DB**: Key=`\x00\x06myzset`, Value=`['z']` + `1` + `expire_time`
*   **ZSet DB (Member Index)**:
    - Key: `\x00\x06myzsetM\x00\x00\x00\x07member1`
    - Value: `1.5` (8 bytes)
*   **ZSet DB (Score Index)**:
    - Key: `\x00\x06myzsetS\x<encoded_score>\x00\x00\x00\x07member1`
    - Value: `(empty)`

**Operations:**
*   `ZADD`: Uses `WriteBatch` for atomic updates:
    1. Update/insert member count in metadata
    2. Insert/update member→score index
    3. Insert/update score→member index
*   `ZRANGE`: Scan score→member index with prefix `user_key`, ordered by score
*   `ZSCORE`: Direct lookup in member→score index
*   `ZREM`: Uses `WriteBatch` to atomically delete both indices and update metadata
*   `ZCARD`: Read count from metadata in String DB (O(1))

---

## Implementation Notes

### Atomicity

- **ZSet operations** (`ZADD`, `ZREM`) use SlateDB's `WriteBatch` to ensure atomic updates across metadata and both indices
- **Set operations** use `WriteBatch` for atomic metadata and member updates
- **Hash operations** use `WriteBatch` for atomic field updates

### Expiration
 
 All data types that store metadata in String DB implement the `Expirable` trait (defined in `crates/storage/src/expirable.rs`) and the `MetaValue` trait (in `crates/storage/src/string/meta.rs`), providing unified TTL management:
 - `StringValue` (String type)
 - `HashMetaValue` (Hash type)
 - `ListMetaValue` (List type)
 - `SetMetaValue` (Set type)
 - `ZSetMetaValue` (Sorted Set type)
 
 The `AnyValue` enum abstracts over all these types, allowing unified read operations and lazy expiration checks.
 
 ### Type Safety
 
 Nimbis leverages a centralized `get_meta<T>` helper to ensure type safety:
 - Operations use `get_meta` with the expected metadata type.
 - `AnyValue` is used for generic operations (`EXPIRE`, `EXISTS`, `TTL`, `GET`).
 - The helper performs strict type-code validation before decoding.
 - `WRONGTYPE` error is returned automatically if a type mismatch is detected.
 - Lazy expiration is handled uniformly within `get_meta`.
