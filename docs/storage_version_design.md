# Storage Version Design

This document describes the logical versioning mechanism implemented in the Nimbis storage layer.

## Core Concept

A **Version** is a **logical timestamp** used for **logical version isolation** and **fast deletion**. It determines data validity through version matching, avoiding the performance overhead of physical deletion for large datasets.

---

## 1. Storage Location

Versions exist in both **Meta Values** and **Data Keys**. Together, they determine data visibility.

### 1.1 Version in Meta Value
Stored in the `string_db` (metadata database), representing the **currently valid** version for a given key.

- **Format**: `[Type] [Version (8B)] [Len] [Expire] ...`
- **Role**: It acts as the "source of truth". Any read operation first retrieves the version from metadata and only processes data matching this version.

### 1.2 Version in Data Key
Stored in the keys of collection databases (e.g., `hash_db`, `set_db`). It represents the version at the time the data record was **created**.

- **Key Format**: `UserKey + Version + Member`
- **Role**: It tags which logical version an individual data item belongs to.

### 1.3 Validity Rule
Data is considered valid if and only if:
```
DataKey.Version == MetaValue.Version
```
If the versions do not match, the data is considered "stale" or a "zombie" and is logically deleted.

---

## 2. Generation Rules

The `VersionGenerator` is responsible for producing globally unique and monotonically increasing version numbers.

1.  **Timestamp-Based**: It primarily uses the current **Unix timestamp (in seconds)**.
2.  **Monotonicity**: If the current timestamp is less than or equal to the last generated version, it increments the last version by 1.
3.  **Concurrency Safety**: Uses atomic operations to ensure safety in multi-threaded environments.

---

## 3. Key Scenarios

### 3.1 O(1) Fast Deletion
When performing a `DEL` operation or when a key expires, Nimbis **does not** need to physically delete every member of a collection one by one.

**Workflow**:
1.  Read and delete the Meta Key (or update the Version in Meta).
2.  **Efficiency**: The operation takes O(1) time, regardless of the collection's size.

**Result**:
The old member data remains physically on disk, but because their version no longer matches the (now non-existent or updated) metadata, all read operations automatically ignore them.

### 3.2 Fast Rebuild & Collision Avoidance
When a key is deleted and immediately recreated with the same name:

**Example**:
1.  `DEL myset`: Deletes the old Meta (Version=100).
2.  `SADD myset m1`: Writes a new Meta (Version=101) and new data `(myset, 101, m1)`.

**Result**:
New and old records are physically isolated by their versions. Even if the old `(myset, 100, m1)` hasn't been cleaned up yet, it will not conflict with the new data.

### 3.3 Async Cleanup (Compaction Filter)
Invalid data is asynchronously cleaned up by the `NimbisCompactionFilter` during the background compaction process.

**Logic**:
1.  The Compaction Filter scans each data item.
2.  It compares `DataKey.Version` with the corresponding `MetaValue.Version`.
3.  If they do not match, the disk space is reclaimed.

---

## 4. Key Advantages

1.  **High Performance**: Deletion of complex structures no longer slows down as data volume grows.
2.  **Low Write Amplification**: Deletion requires minimal disk writes, avoiding massive "tombstone" markers.
3.  **Resource Optimization**: Cleanup is handled in the background, ensuring low latency for frontend requests.
