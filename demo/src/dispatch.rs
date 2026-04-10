// ============================================================
// dispatch 模块 — 分发策略演示
//
// 【教学要点】
// 这个模块使用 library crate 中定义的 device 模块。
// 注意 use 路径的写法：
//   use crate::device::Device;   从 crate 根开始的绝对路径
//   use super::device::Device;   从父模块开始的相对路径
// 两种写法等价，推荐用 crate:: 开头（更清晰）
// ============================================================

use crate::device::{Device, Uart, Virtio};

/// 静态分发：编译期单态化，零开销
/// 【对比 C】类似宏展开，每种类型生成一份代码副本
pub fn static_io<D: Device>(dev: &mut D) {
    println!("--- static dispatch: {} ---", dev.name());
    let data = [0x41u8, 0x42, 0x43, 0x44];
    dev.write(&data);

    let mut buf = [0u8; 8];
    dev.read(&mut buf);
}

/// 动态分发：运行时查 vtable
/// 【对比 C】等价于 dev->ops->write(dev, buf, len)
pub fn dynamic_io(dev: &mut dyn Device) {
    println!("--- dynamic dispatch: {} ---", dev.name());
    let data = [0xAAu8, 0xBB, 0xCC, 0xDD];
    dev.write(&data);
}

/// 设备列表演示
pub fn device_list_demo() {
    println!("--- device list (Vec<Box<dyn Device>>) ---");
    let mut devices: Vec<Box<dyn Device>> = vec![
        Box::new(Uart::new(0x3F8)),
        Box::new(Uart::new(0x2F8)),
        Box::new(Virtio::new(0x1000_0000)),
    ];

    for dev in &mut devices {
        let buf = [0x01u8, 0x02, 0x03, 0x04];
        dev.write(&buf);
    }
}

/// 胖指针大小演示
pub fn fat_pointer_demo() {
    println!("size of &mut Uart        = {} bytes (thin pointer)",
             std::mem::size_of::<&mut Uart>());
    println!("size of &mut dyn Device  = {} bytes (fat pointer: data + vtable)",
             std::mem::size_of::<&mut dyn Device>());
}
