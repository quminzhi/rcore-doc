// ============================================================
// device/uart.rs — Uart 设备实现
//
// 【教学要点】
// 这个文件是 device 模块的子模块。
// 路径关系：device/uart.rs -> 模块路径 device::uart
//
// 注意：这里用 super::Device 访问父模块的 trait
//   super = device (即 device/mod.rs 中定义的内容)
// ============================================================

use super::Device;

pub struct Uart {
    port: u16,
}

impl Uart {
    pub fn new(port: u16) -> Self {
        Uart { port }
    }

    /// 私有方法：仅 Uart 内部使用
    /// 外部代码无法调用 uart.port_address()
    fn port_address(&self) -> String {
        format!("0x{:x}", self.port)
    }
}

impl Device for Uart {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        println!("[UART {}] read {} bytes", self.port_address(), buf.len());
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }
        buf.len()
    }

    fn write(&mut self, buf: &[u8]) -> usize {
        println!("[UART {}] write {} bytes: {:?}", self.port_address(), buf.len(), &buf[..buf.len().min(4)]);
        buf.len()
    }

    fn name(&self) -> &str {
        "UART"
    }
}
