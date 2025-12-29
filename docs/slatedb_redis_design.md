# SlateDB Redis Data Types Implementation Design

This document outlines the design for implementing Redis data types on top of SlateDB's key-value store.

## Overview

SlateDB is a key-value store. To support Redis data types (String, Hash, List, Set, ZSet), we need to map high-level Redis structures to SlateDB's flat binary key-space.

## Encoding Scheme

We use a prefix-based encoding scheme to distinguish between different data types and metadata.

### 1. Meta Key

The Meta Key is used to store metadata for a specific user key and data type (e.g., expiration, encoding info, item count).

**Key Format:**
```
'm' + user_key + type
```

*   **Prefix**: `m` (ASCII char) identifies the key as a Metadata key.
*   **User Key**: The original key provided by the client.
*   **Type**: Single byte identifier for the data type (e.g., 's' for String, 'h' for Hash).

**Value Format:**
Variable based on `type`.

*   **String ('s')**: NO Meta Key. Simple strings do not use a separate metadata key.
*   **Hash ('h')**: (Format definition tbd, e.g., field_count, encoding_version)
*   **List ('l')**: (Format definition tbd, e.g., head_index, tail_index, length)

### 2. String Type

Strings are the simplest Redis data type. They map almost directly to the underlying KV store, with a type prefix.

**Key Format:**
```
's' + user_key
```

*   **Prefix**: `s` (ASCII char) identifies the key as a String type.
*   **User Key**: The original key provided by the client (bytes).

**Value Format:**
```
raw_value_bytes
```

*   **Value**: The raw binary data of the string value. No additional metadata or encoding wrappers are added to the value itself for simple strings.

**Example:**
*   Redis Command: `SET mykey "hello"`
*   SlateDB Key: `[115, 109, 121, 107, 101, 121]` (bytes for 's' + 'mykey')
*   SlateDB Value: `[104, 101, 108, 108, 111]` (bytes for "hello")

---


## Future Implementations (Tentative)

The following designs are placeholders and subject to change.

### Hash
*   **Meta Key**: `h` + `user_key` -> Metadata (count, encoding, etc.)
*   **Field Key**: `H` + `user_key` + `length(field)` + `field` -> `value`

### Set
*   **Meta Key**: `t` + `user_key`
*   **Member Key**: `T` + `user_key` + `member` -> `(empty)`

### List
*   **Meta Key**: `l` + `user_key` -> Metadata (head, tail, count)
*   **Node Key**: `L` + `user_key` + `seq_id` -> `value`

### ZSet
*   **Meta Key**: `z` + `user_key`
*   **Member Key**: `Z` + `user_key` + `member` -> `score`
*   **Score Key**: `S` + `user_key` + `score` + `member` -> `(empty)`
