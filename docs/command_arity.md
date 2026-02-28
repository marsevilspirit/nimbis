# Command Arity Rules

Nimbis follows standard Redis command arity rules. The `arity` field in command metadata (`CmdMeta`) defines the argument count requirements for each command.

## Core Rules

1.  **Count Includes Command Name**: The `arity` count always includes the command name itself.
    *   Example: `GET key` has an arity of 2 (1 command name + 1 key).

2.  **Positive Arity ($n > 0$)**: **Exact Match**
    *   The command must have exactly $n$ arguments.
    *   Returns an error if the argument count is not exactly $n$.

3.  **Negative Arity ($n < 0$)**: **Minimum Match**
    *   The command must have at least $-n$ arguments.
    *   Returns an error if the argument count is less than $-n$ (absolute value).
    *   Allows argument counts greater than $-n$.

## Examples

| Command       | Arity | Type             | Meaning              | Example                                                           |
| :------------ | :---- | :--------------- | :------------------- | :---------------------------------------------------------------- |
| **GET**       | `2`   | Positive (Exact) | Exactly 2 arguments  | `GET <key>` <br> (1 cmd + 1 arg)                                  |
| **SET**       | `3`   | Positive (Exact) | Exactly 3 arguments  | `SET <key> <value>` <br> (1 cmd + 2 args)                         |
| **PING**      | `-1`  | Negative (Min)   | At least 1 argument  | `PING` (1 arg) <br> `PING <msg>` (2 args)                         |
| **EXISTS**    | `-2`  | Negative (Min)   | At least 2 arguments | `EXISTS <key1>` (2 args) <br> `EXISTS <k1> <k2> ...` (>2 args)    |
| **EXPIRE**    | `3`   | Positive (Exact) | Exactly 3 arguments  | `EXPIRE <key> <seconds>` (3 args)                                 |
| **TTL**       | `2`   | Positive (Exact) | Exactly 2 arguments  | `TTL <key>` (2 args)                                              |
| **INCR**      | `2`   | Positive (Exact) | Exactly 2 arguments  | `INCR <key>` (1 cmd + 1 arg)                                  |
| **DECR**      | `2`   | Positive (Exact) | Exactly 2 arguments  | `DECR <key>` (1 cmd + 1 arg)                                  |
| **MGET**      | `-2`  | Negative (Min)   | At least 2 arguments | `MGET <key1>` (2 args) <br> `MGET <k1> <k2> ...` (>2 args)        |
| **LPUSH**     | `-3`  | Negative (Min)   | At least 3 arguments | `LPUSH <key> <el>` (3 args) <br> `LPUSH <k> <e1> <e2>` (4 args)   |
| **LPOP**      | `-2`  | Negative (Min)   | At least 2 arguments | `LPOP <key>` (2 args) <br> `LPOP <key> <count>` (3 args)          |
| **LRANGE**    | `4`   | Positive (Exact) | Exactly 4 arguments  | `LRANGE <key> <start> <stop>` (4 args)                            |
| **SADD**      | `-3`  | Negative (Min)   | At least 3 arguments | `SADD <key> <member>` (3 args) <br> `SADD <k> <m1> <m2>` (4 args) |
| **SREM**      | `-3`  | Negative (Min)   | At least 3 arguments | `SREM <key> <member>` (3 args)                                    |
| **SMEMBERS**  | `2`   | Positive (Exact) | Exactly 2 arguments  | `SMEMBERS <key>` (2 args)                                         |
| **SISMEMBER** | `3`   | Positive (Exact) | Exactly 3 arguments  | `SISMEMBER <key> <member>` (3 args)                               |
| **SCARD**     | `2`   | Positive (Exact) | Exactly 2 arguments  | `SCARD <key>` (2 args)                                            |
| **HDEL**      | `-3`  | Negative (Min)   | At least 3 arguments | `HDEL <key> <field>` (3 args) <br> `HDEL <k> <f1> <f2>` (>3 args) |
| **FLUSHDB**   | `1`   | Positive (Exact) | Exactly 1 argument   | `FLUSHDB` (1 arg)                                                 |

## Implementation Details

In `CmdMeta::validate_arity` (in `crates/nimbis/src/cmd/mod.rs`):
- The input `arg_count` should be `args.len() + 1` (i.e., the length of the `args` array plus the command name).
