// ============================================================
// trait 定义：等价于 C 的函数指针表
// ============================================================
trait Device {
    fn read(&mut self, buf: &mut [u8]) -> usize;
    fn write(&mut self, buf: &[u8]) -> usize;
    fn name(&self) -> &str;
}

// ============================================================
// Uart 设备
// ============================================================
struct Uart {
    port: u16,
}

impl Device for Uart {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        println!("[UART 0x{:x}] read {} bytes", self.port, buf.len());
        // 模拟填充数据
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }
        buf.len()
    }

    fn write(&mut self, buf: &[u8]) -> usize {
        println!("[UART 0x{:x}] write {} bytes: {:?}", self.port, buf.len(), &buf[..4]);
        buf.len()
    }

    fn name(&self) -> &str { "UART" }
}

// ============================================================
// Virtio 设备
// ============================================================
struct Virtio {
    mmio_base: usize,
}

impl Device for Virtio {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        println!("[Virtio 0x{:x}] read {} bytes", self.mmio_base, buf.len());
        buf.len()
    }

    fn write(&mut self, buf: &[u8]) -> usize {
        println!("[Virtio 0x{:x}] write {} bytes: {:?}", self.mmio_base, buf.len(), &buf[..4]);
        buf.len()
    }

    fn name(&self) -> &str { "Virtio" }
}

// ============================================================
// 静态分发：编译期确定类型，零开销
// 等价于 C 的宏展开，每种类型生成一份代码
// ============================================================
fn static_io<D: Device>(dev: &mut D) {
    println!("--- static dispatch: {} ---", dev.name());
    let data = [0x41u8, 0x42, 0x43, 0x44]; // "ABCD"
    dev.write(&data);

    let mut buf = [0u8; 8];
    dev.read(&mut buf);
}

// ============================================================
// 动态分发：运行时查 vtable
// 等价于 C 的 struct Device *dev -> dev->write(...)
// ============================================================
fn dynamic_io(dev: &mut dyn Device) {
    println!("--- dynamic dispatch: {} ---", dev.name());
    let data = [0xAAu8, 0xBB, 0xCC, 0xDD];
    dev.write(&data);
}

// ============================================================
// 入口
// ============================================================
pub fn run() {
    // 1. 静态分发
    println!("=== 1. Static Dispatch (zero cost, like C macro) ===\n");
    let mut uart   = Uart   { port: 0x3F8 };
    let mut virtio = Virtio { mmio_base: 0x1000_0000 };
    static_io(&mut uart);
    static_io(&mut virtio);

    // 2. 动态分发
    println!("\n=== 2. Dynamic Dispatch (vtable, like C function pointer) ===\n");
    dynamic_io(&mut uart);
    dynamic_io(&mut virtio);

    // 3. 设备列表（最像 OS driver 管理的写法）
    // Vec<Box<dyn Device>> 等价于 C 的 struct Device *devices[]
    println!("\n=== 3. Device List (like struct Device *devices[]) ===\n");
    let mut devices: Vec<Box<dyn Device>> = vec![
        Box::new(Uart   { port: 0x3F8 }),
        Box::new(Uart   { port: 0x2F8 }),
        Box::new(Virtio { mmio_base: 0x1000_0000 }),
    ];

    for dev in &mut devices {
        let buf = [0x01u8, 0x02, 0x03, 0x04];
        dev.write(&buf);
    }

    // 4. 打印 &dyn Device 胖指针的大小，直观感受
    println!("\n=== 4. Fat Pointer Size ===\n");
    println!("size of &mut Uart        = {} bytes (thin pointer)",
             std::mem::size_of::<&mut Uart>());
    println!("size of &mut dyn Device  = {} bytes (fat pointer: data + vtable)",
             std::mem::size_of::<&mut dyn Device>());
}
