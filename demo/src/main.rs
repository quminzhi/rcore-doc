// ============================================================
// main.rs — binary crate 入口
//
// 【教学要点 - binary crate 与 library crate 的关系】
//
// main.rs 和 lib.rs 是两个独立的 crate：
//   - main.rs = binary crate（编译为可执行文件）
//   - lib.rs  = library crate（编译为库）
//
// binary crate 通过 `use demo::xxx` 引用 library crate
// （demo 是 Cargo.toml 中 [package] name 的值）
//
// ============================================================

// --- 引入 library crate 中的内容 ---
// 方式一：通过 re-export 的短路径
use demo::Device;
// 方式二：完整路径
use demo::device::{Uart, Virtio};
// 引入 dispatch 模块的函数
use demo::dispatch;

fn main() {
    println!("========================================");
    println!(" Module & Crate Demo");
    println!("========================================\n");

    // pub(crate) 函数：因为 main.rs 是另一个 crate，所以无法调用！
    // 取消注释下面这行会报错：
    // demo::device::list_all_devices();  // ERROR: 函数是 pub(crate)，仅 lib crate 内部可见

    // 使用 library crate 导出的类型
    let mut uart = Uart::new(0x3F8);
    let mut virtio = Virtio::new(0x1000_0000);

    println!("=== 静态分发 ===\n");
    dispatch::static_io(&mut uart);
    dispatch::static_io(&mut virtio);

    println!("\n=== 动态分发 ===\n");
    dispatch::dynamic_io(&mut uart);
    dispatch::dynamic_io(&mut virtio);

    println!("\n=== 设备列表 ===\n");
    dispatch::device_list_demo();

    println!("\n=== 胖指针 ===\n");
    dispatch::fat_pointer_demo();

    // ========================================
    // Part 3: 模块路径总结
    // ========================================
    println!("\n========================================");
    println!(" Part 3: 模块路径总结");
    println!("========================================\n");
    println!("完整路径:  demo::device::uart::Uart   (但 uart 是私有子模块，外部不可达)");
    println!("re-export: demo::device::Uart          (通过 mod.rs 中的 pub use)");
    println!("再 re-export: demo::Device             (通过 lib.rs 中的 pub use)");
    println!("\n路径解析优先级:");
    println!("  crate::  -> 当前 crate 根 (lib.rs 或 main.rs)");
    println!("  super::  -> 父模块");
    println!("  self::   -> 当前模块");

    // 证明 Device trait 确实通过 re-export 可用
    println!("\n通过 re-export 调用 Device trait 方法:");
    print_device_name(&uart);
    print_device_name(&virtio);
}

/// 使用 re-export 的 Device trait
fn print_device_name(dev: &dyn Device) {
    println!("  设备名: {}", dev.name());
}
