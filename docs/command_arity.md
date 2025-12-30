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

| Command  | Arity | Type             | Meaning              | Example                                                    |
| :------- | :---- | :--------------- | :------------------- | :--------------------------------------------------------- |
| **GET**  | `2`   | Positive (Exact) | Exactly 2 arguments  | `GET <key>` <br> (1 cmd + 1 arg)                           |
| **SET**  | `3`   | Positive (Exact) | Exactly 3 arguments  | `SET <key> <value>` <br> (1 cmd + 2 args)                  |
| **PING** | `-1`  | Negative (Min)   | At least 1 argument  | `PING` (1 arg) <br> `PING <msg>` (2 args)                  |
| **MGET** | `-2`  | Negative (Min)   | At least 2 arguments | `MGET <key1>` (2 args) <br> `MGET <k1> <k2> ...` (>2 args) |

## Implementation Details

In `CmdMeta::validate_arity`:
- The input `arg_count` should be `args.len() + 1` (i.e., the length of the `args` array plus the command name).
