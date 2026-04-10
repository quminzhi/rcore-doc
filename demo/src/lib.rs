// ============================================================
// lib.rs — library crate 的根模块
//
// 【教学要点 - crate 与模块的关系】
//
// 一个 Cargo package 中：
//   src/main.rs  ->  binary crate（可执行文件）
//   src/lib.rs   ->  library crate（库）
//
// 两者可以共存！binary crate 通过 `use demo::xxx` 使用 library crate。
//
// 模块树 (module tree)：
//
//   demo (lib)                    <- 这个文件 (lib.rs)
//   ├── device                    <- device/mod.rs
//   │   ├── uart                  <- device/uart.rs
//   │   └── virtio                <- device/virtio.rs
//   └── dispatch                  <- dispatch.rs
//
// 【教学要点 - 文件与模块的映射规则】
//
//   mod foo;  会查找：
//     1. foo.rs        （单文件模块）
//     2. foo/mod.rs    （目录模块）
//   两种方式不能同时存在！
//
// ============================================================

// --- 声明模块 ---
pub mod device;
pub mod dispatch;

// --- re-export：让外部用更短的路径 ---
// 没有这行：外部写 demo::device::Device
// 有了这行：外部写 demo::Device 就行
pub use device::Device;
