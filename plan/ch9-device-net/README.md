# Ch9: 设备驱动与网络

> 对应分支：`origin/ch9`
> 对应 xv6：`uart.c` + `virtio_disk.c`
> 预计时间：2-3 天

---

## 9.1 本章目标

完成 OS 的最后一块拼图：**设备驱动**和**网络栈**。

核心能力：
- **VirtIO 设备驱动** — 块设备、网络、GPU、输入设备
- **PLIC 中断控制器** — 外部中断管理
- **UART 串口驱动** — 替代 SBI 的字符 I/O
- **TCP/UDP 网络栈** — 简单的网络通信

---

## 9.2 新增文件

```
os/src/
├── drivers/
│   ├── mod.rs              # 驱动模块入口
│   ├── block/
│   │   ├── mod.rs          # BlockDevice trait
│   │   └── virtio_blk.rs   # VirtIO 块设备驱动（非阻塞 I/O）
│   ├── bus/
│   │   ├── mod.rs
│   │   └── virtio.rs       # VirtIO 总线 HAL 层
│   ├── chardev/
│   │   ├── mod.rs          # CharDevice trait
│   │   └── ns16550a.rs     # UART 16550A 串口驱动
│   ├── gpu/
│   │   └── mod.rs          # GPU 显示驱动
│   ├── input/
│   │   └── mod.rs          # 键盘/鼠标输入驱动
│   ├── net/
│   │   └── mod.rs          # VirtIO 网络设备
│   └── plic.rs             # RISC-V PLIC 中断控制器
├── net/
│   ├── mod.rs              # 网络栈入口 + 中断处理
│   ├── socket.rs           # Socket 管理
│   ├── port_table.rs       # 端口表
│   ├── tcp.rs              # TCP 协议
│   └── udp.rs              # UDP 协议
└── syscall/
    ├── net.rs              # 网络系统调用
    ├── gui.rs              # GUI 系统调用
    └── input.rs            # 输入设备系统调用
```

---

## 9.3 VirtIO — 虚拟化 I/O 框架

VirtIO 是 QEMU 使用的虚拟设备标准。xv6 也用 VirtIO 块设备。

### VirtIO 工作原理
```
┌──────────────────────────────────────┐
│  Guest OS (rCore)                     │
│  ┌──────────────┐                     │
│  │  VirtIO 驱动  │                    │
│  └──────┬───────┘                     │
│         │ 共享内存 (VirtQueue)          │
│  ┌──────┴───────┐                     │
│  │  VirtQueue    │  描述符表 + 可用环 + 已用环 │
│  └──────┬───────┘                     │
├─────────┼────────────────────────────┤
│  ┌──────┴───────┐                     │
│  │  QEMU 后端    │  实际 I/O 操作       │
│  └──────────────┘                     │
│  Host OS                               │
└──────────────────────────────────────┘
```

### VirtIO Block — 块设备驱动

```rust
// os/src/drivers/block/virtio_blk.rs
pub struct VirtIOBlock {
    virtio_blk: UPIntrFreeCell<VirtIOBlk<'static, VirtioHal>>,
    condvars: BTreeMap<u16, Condvar>,    // 每个请求一个条件变量
}

impl BlockDevice for VirtIOBlock {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        let nb = *DEV_NON_BLOCKING_ACCESS.exclusive_access();
        if nb {
            // 非阻塞模式：提交请求 → 等待条件变量 → 中断唤醒
            let mut resp = BlkResp::default();
            let task_cx_ptr = self.virtio_blk.exclusive_session(|blk| {
                let token = unsafe { blk.read_block_nb(block_id, buf, &mut resp).unwrap() };
                self.condvars.get(&token).unwrap().wait_no_sched()
            });
            schedule(task_cx_ptr);     // 切换到其他任务
            assert_eq!(resp.status(), RespStatus::Ok);
        } else {
            // 阻塞模式：直接等待完成
            self.virtio_blk.exclusive_access()
                .read_block(block_id, buf)
                .expect("Error when reading VirtIOBlk");
        }
    }

    fn handle_irq(&self) {
        // 中断到来 → 唤醒等待的任务
        self.virtio_blk.exclusive_session(|blk| {
            while let Ok(token) = blk.pop_used() {
                self.condvars.get(&token).unwrap().signal();
            }
        });
    }
}
```

**对比 xv6 的 `virtio_disk.c`：**
```c
void virtio_disk_rw(struct buf *b, int write) {
    // 填充描述符
    // 通知设备
    while (b->disk == 1) {
        sleep(b, &disk.vdisk_lock);  // 等待中断
    }
}
```

rCore 使用 Condvar 替代 xv6 的 sleep/wakeup，并支持非阻塞模式。

---

## 9.4 PLIC — 中断控制器

```rust
// os/src/drivers/plic.rs
pub struct PLIC {
    base_addr: usize,
}

impl PLIC {
    // 设置中断源优先级
    pub fn set_priority(&mut self, intr_source_id: usize, priority: u32) {
        unsafe {
            let ptr = self.priority_ptr(intr_source_id);
            ptr.write_volatile(priority);
        }
    }

    // 使能中断源
    pub fn enable(&mut self, hart_id: usize, target_priority: IntrTargetPriority,
                  intr_source_id: usize) {
        let (reg_ptr, reg_shift) = self.enable_ptr(hart_id, target_priority, intr_source_id);
        unsafe {
            reg_ptr.write_volatile(reg_ptr.read_volatile() | (1 << reg_shift));
        }
    }

    // 声明中断（获取中断号）
    pub fn claim(&self, hart_id: usize, target_priority: IntrTargetPriority) -> u32 {
        unsafe {
            self.claim_comp_ptr_of_hart_with_priority(hart_id, target_priority)
                .read_volatile()
        }
    }

    // 完成中断处理
    pub fn complete(&self, hart_id: usize, target_priority: IntrTargetPriority,
                    completion: u32) {
        unsafe {
            self.claim_comp_ptr_of_hart_with_priority(hart_id, target_priority)
                .write_volatile(completion);
        }
    }
}
```

### 中断处理流程
```
外部设备产生中断
    ↓
PLIC 路由到 CPU
    ↓
trap_handler: Trap::Interrupt(Interrupt::SupervisorExternal)
    ↓
plic.claim() → 获取中断号
    ↓
根据中断号分发：
  1 → UART 串口中断
  8 → VirtIO 块设备中断
  10 → VirtIO 网络中断
    ↓
plic.complete() → 通知 PLIC 中断处理完毕
```

---

## 9.5 UART 串口驱动 — NS16550A

```rust
// os/src/drivers/chardev/ns16550a.rs
pub trait CharDevice {
    fn init(&self);
    fn read(&self) -> u8;
    fn write(&self, ch: u8);
    fn handle_irq(&self);
}
```

ch9 用 NS16550A UART 驱动替代了之前的 SBI ecall 字符输出：
- 直接操作 UART 寄存器（MMIO 地址 0x10000000）
- 支持中断驱动的输入（不再需要轮询）

---

## 9.6 网络栈

### Socket 结构
```rust
// os/src/net/socket.rs
pub struct Socket {
    pub raddr: IPv4,              // 远程 IP 地址
    pub lport: u16,               // 本地端口
    pub rport: u16,               // 远程端口
    pub buffers: VecDeque<Vec<u8>>, // 接收缓冲区
    pub seq: u32,                 // TCP 序列号
    pub ack: u32,                 // TCP 确认号
}
```

### 网络中断处理
```rust
// os/src/net/mod.rs
pub fn net_interrupt_handler() {
    let mut recv_buf = vec![0u8; 1024];
    let len = NET_DEVICE.receive(&mut recv_buf);
    let packet = LOSE_NET_STACK.0.exclusive_access()
        .analysis(&recv_buf[..len]);

    match packet {
        Packet::ARP(arp_packet) => {
            // ARP 请求 → 回复 MAC 地址
            let reply = arp_packet.reply_packet(ip, mac);
            NET_DEVICE.transmit(&reply.build_data());
        }
        Packet::UDP(udp_packet) => {
            // UDP 包 → 推入对应 socket 的缓冲区
            if let Some(socket_index) = get_socket(target, lport, rport) {
                push_data(socket_index, udp_packet.data.to_vec());
            }
        }
        Packet::TCP(tcp_packet) => {
            // TCP 包 → 处理连接和数据
            // ...
        }
    }
}
```

---

## 9.7 UPIntrFreeCell — 中断安全的同步容器

ch9 引入了比 `UPSafeCell` 更安全的容器：

```rust
pub struct UPIntrFreeCell<T> {
    inner: UPSafeCell<T>,
}

impl<T> UPIntrFreeCell<T> {
    pub fn exclusive_access(&self) -> UPIntrRefMut<'_, T> {
        // 关闭中断
        // 获取独占访问
        // 返回守卫
    }

    pub fn exclusive_session<F, V>(&self, f: F) -> V
    where
        F: FnOnce(&mut T) -> V,
    {
        // 关中断 → 执行闭包 → 开中断
    }
}
```

**为什么需要关中断？**
- 驱动代码可能在中断上下文和普通上下文中都被调用
- 如果不关中断，可能出现：
  1. 任务 A 获取 VirtIOBlock 的锁
  2. 中断到来，handle_irq 也要获取同一个锁
  3. 死锁！

对比 xv6：
```c
void acquire(struct spinlock *lk) {
    push_off();  // 关中断
    // ...
}
```

---

## 9.8 ch8 → ch9 变更统计

| 类别 | 新增/修改 |
|------|----------|
| 驱动框架 | `drivers/` 大幅扩展（chardev, gpu, input, net, bus, plic） |
| 网络栈 | `net/` 全新模块（~600 行） |
| 同步原语 | `UPIntrFreeCell` 替代部分 `UPSafeCell` |
| 系统调用 | gui, input, net 相关 syscall |
| 用户程序 | tcp_simplehttp, udp, gui_*, inputdev_event 等 |
| 总计 | +3145 行 / -502 行 |

---

## 9.9 完整 OS 架构图

```
┌──────────────────────────────────────────────────┐
│  用户态应用                                        │
│  (shell, cat, httpserver, gui_snake, ...)         │
├──────────────────────────────────────────────────┤
│  系统调用层 (syscall/)                             │
│  fs | process | sync | thread | net | gui | input │
├──────────────────────────────────────────────────┤
│  内核核心功能                                      │
│  ┌────────┬──────┬──────┬──────┬───────┐         │
│  │  task/  │ mm/  │ fs/  │ sync/│ net/  │         │
│  │ 进程线程│虚存  │文件  │同步  │网络栈 │          │
│  └────────┴──────┴──────┴──────┴───────┘         │
├──────────────────────────────────────────────────┤
│  驱动层 (drivers/)                                │
│  VirtIO Block | UART | GPU | Input | Net | PLIC  │
├──────────────────────────────────────────────────┤
│  硬件抽象 (trap/ + sbi)                           │
│  Trap 处理 | 时钟中断 | 外部中断 | SBI 调用       │
├──────────────────────────────────────────────────┤
│  RISC-V 硬件 (QEMU virt)                         │
└──────────────────────────────────────────────────┘
```

---

## 9.10 Rust 知识点总结

| 特性 | 本章用途 |
|------|---------|
| `UPIntrFreeCell` | 中断安全的同步容器 |
| `BTreeMap<u16, Condvar>` | VirtIO 请求 → 条件变量映射 |
| `FnOnce` 闭包 | `exclusive_session` 中的回调 |
| `dyn BlockDevice` | 块设备多态（VirtIO/测试环境） |
| `dyn CharDevice` | 字符设备多态 |
| `write_volatile`/`read_volatile` | MMIO 寄存器操作 |
| `lazy_static!` | 全局设备实例（`GPU_DEVICE`, `NET_DEVICE` 等） |

---

## 9.11 思考题

1. VirtIO 的非阻塞 I/O 和阻塞 I/O 各有什么优缺点？

2. 为什么 `UPIntrFreeCell` 要在获取锁时关闭中断？

3. PLIC 的 `claim` 和 `complete` 是做什么的？为什么需要 complete？
   > 提示：告诉 PLIC 中断已处理完毕，可以发送下一个中断

4. 如果网络包到来时，对应的 socket 不存在，数据会怎样处理？

5. rCore 的网络栈是在内核态处理的，Linux 也是吗？有没有用户态网络栈？
