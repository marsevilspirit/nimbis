# RESP - Redis 序列化协议库

使用 Rust 编写的高性能、零拷贝 RESP 协议解析器和编码器。

## 特性

- ⚡ **零拷贝解析** - 使用 `Bytes` 进行高效的内存管理
- 🔧 **RESP2 & RESP3 支持** - 完整的协议支持
- 🔒 **类型安全** - 充分利用 Rust 的类型系统
- 🚀 **高性能** - 针对吞吐量和最小分配进行优化
- ✨ **优雅的 API** - 符合人体工程学的接口设计

## 使用示例

### 解析 RESP 值

```rust
use resp;

let value = resp::parse(b"+OK\r\n").unwrap();
assert_eq!(value.as_str(), Some("OK"));
```

### 创建和编码 RESP 值

```rust
use resp::{RespValue, RespEncoder};

// 创建 Redis SET 命令（使用 From trait）
let cmd = RespValue::Array(vec![
    "SET".into(),
    "key".into(),
    "value".into(),
]);

// 或使用便捷方法
let cmd = RespValue::array([
    RespValue::bulk_string("SET"),
    RespValue::bulk_string("key"),
    RespValue::bulk_string("value"),
]);

// 编码为字节
let encoded = cmd.encode().unwrap();
// 输出: b"*3\r\n$3\r\nSET\r\n$3\r\nkey\r\n$5\r\nvalue\r\n"
```

### 类型转换

```rust
use resp::RespValue;

// 使用 From trait 创建值 - 无需 bytes 依赖！
let value: RespValue = "hello".into();

// 安全的类型转换
if let Some(s) = value.as_str() {
    println!("String value: {}", s);
}

// 更多 From trait 实现
let from_str: RespValue = "test".into();
let from_int: RespValue = 42i64.into();
let from_bool: RespValue = true.into();

// 或使用便捷方法
let value = RespValue::bulk_string("hello");
let array = RespValue::array([1.into(), 2.into(), 3.into()]);
```

## 支持的类型

### RESP2 类型
- ✅ 简单字符串 (`+OK\r\n`)
- ✅ 错误 (`-ERR message\r\n`)
- ✅ 整数 (`:1000\r\n`)
- ✅ 批量字符串 (`$6\r\nfoobar\r\n`)
- ✅ 数组 (`*2\r\n...`)
- ✅ 空值 (`$-1\r\n`)

### RESP3 类型
- ✅ 布尔值 (`#t\r\n` / `#f\r\n`)
- ✅ 双精度浮点数 (`,3.14\r\n`)
- ✅ 大数 (`(12345...\r\n`)
- ✅ 批量错误 (`!21\r\nERROR...\r\n`)
- ✅ 逐字字符串 (`=15\r\ntxt:...\r\n`)
- ✅ 映射 (`%2\r\n...`)
- ✅ 集合 (`~5\r\n...`)
- ✅ 推送 (`>4\r\n...`)

## 示例

查看 `examples/` 目录了解更多使用模式：

```bash
# 基本使用示例
cargo run --example basic_usage
```

## 运行测试

```bash
# 运行所有测试
just test
```

## 性能基准测试

```bash
just bench
```

基准测试包括：
- 不同 RESP 类型的解析性能
- 编码性能
- 往返（编码 + 解析）性能
- 大型数组和复杂嵌套结构的性能

## 开发

```bash
# 构建库
just build

# 运行所有检查（格式化、clippy、测试）
just all

# 检查代码和格式
just check

# 格式化代码
just fmt
```

## API 文档

生成并查看 API 文档：

```bash
cargo doc --no-deps --open
```

## 性能优化

本库采用了多种优化技术：

1. **零拷贝** - 使用 `Bytes::slice()` 避免不必要的内存拷贝
2. **提前返回** - 遇到不完整数据时快速返回
3. **容量预分配** - 为已知大小的集合预分配内存
4. **最小化分配** - 重用缓冲区并避免临时分配

## 架构

```
resp/
├── src/
│   ├── lib.rs          # 库入口点
│   ├── types.rs        # RESP 值类型定义
│   ├── parser.rs       # 解析器实现
│   ├── encoder.rs      # 编码器实现
│   ├── error.rs        # 错误类型
│   └── utils.rs        # 工具函数
├── tests/              # 集成测试
├── benches/            # 性能基准测试
└── examples/           # 示例代码
```
