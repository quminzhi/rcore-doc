// ============================================================
// device 模块入口 (device/mod.rs)
//
// 【教学要点】
// 当模块内容较多时，用目录形式组织：
//   device/
//     mod.rs      <- 等价于之前的 device.rs
//     uart.rs
//     virtio.rs
//
// mod.rs 的职责：
//   1. 声明子模块 (mod uart; mod virtio;)
//   2. 定义该模块公共接口 (trait Device)
//   3. 通过 pub use 控制对外暴露哪些类型
// ============================================================

// --- 声明子模块 ---
// 不加 pub，外部无法直接访问 device::uart::Uart
// 只能通过本模块的 re-export 访问
mod uart;
mod virtio;

// --- re-export：简化外部使用路径 ---
// 没有这行：外部需要写 device::uart::Uart（但 uart 是私有的，编译不过）
// 有了这行：外部直接写 device::Uart
pub use uart::Uart;
pub use virtio::Virtio;

// ============================================================
// trait 定义：设备公共接口
//
// 【对比 C】等价于函数指针表：
//   struct DeviceOps {
//       ssize_t (*read)(void *dev, char *buf, size_t len);
//       ssize_t (*write)(void *dev, const char *buf, size_t len);
//       const char* (*name)(void *dev);
//   };
// ============================================================
pub trait Device {
    fn read(&mut self, buf: &mut [u8]) -> usize;
    fn write(&mut self, buf: &[u8]) -> usize;
    fn name(&self) -> &str;
}

// ============================================================
// pub(crate) 函数：仅 crate 内部可见
//
// 【教学要点 - 可见性层级】
//   private      — 仅当前模块（默认）
//   pub(crate)   — 整个 crate 内部
//   pub(super)   — 父模块
//   pub          — 完全公开
// ============================================================
pub(crate) fn list_all_devices() {
    println!("[device] 已注册设备: UART, Virtio");
}
