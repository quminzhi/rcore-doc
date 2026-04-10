// ============================================================
// device/virtio.rs — Virtio 设备实现
//
// 【教学要点】
// 同样通过 super::Device 引用父模块的 trait
// ============================================================

use super::Device;

pub struct Virtio {
    mmio_base: usize,
}

impl Virtio {
    pub fn new(mmio_base: usize) -> Self {
        Virtio { mmio_base }
    }
}

impl Device for Virtio {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        println!("[Virtio 0x{:x}] read {} bytes", self.mmio_base, buf.len());
        buf.len()
    }

    fn write(&mut self, buf: &[u8]) -> usize {
        println!("[Virtio 0x{:x}] write {} bytes: {:?}", self.mmio_base, buf.len(), &buf[..buf.len().min(4)]);
        buf.len()
    }

    fn name(&self) -> &str {
        "Virtio"
    }
}
