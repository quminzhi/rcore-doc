# Ch4: 地址空间（虚拟内存）

> 对应分支：`origin/ch4`
> 对应 xv6：`vm.c` + `kalloc.c` + SV39 页表
> 预计时间：3-4 天 ⭐ 重点章节

---

## 4.1 本章目标

为每个应用建立**独立的虚拟地址空间**，实现内存隔离。

核心能力：
- **SV39 三级页表**（与 xv6 完全一致）
- **物理帧分配器**（对标 xv6 的 `kalloc`）
- **内核堆分配器**（让内核能用 `Vec`, `String` 等）
- **地址空间抽象**（MapArea + MemorySet）

---

## 4.2 新增文件

```
os/src/mm/
├── mod.rs              # 模块入口，init() 函数
├── address.rs          # 物理/虚拟地址、页号的类型定义
├── page_table.rs       # SV39 页表实现
├── frame_allocator.rs  # 物理帧分配器
├── heap_allocator.rs   # 内核堆分配器
└── memory_set.rs       # 地址空间（MemorySet = 一组 MapArea）
```

---

## 4.3 地址类型系统 — Rust 的类型安全优势

```rust
// os/src/mm/address.rs
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(pub usize);       // 物理地址

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr(pub usize);       // 虚拟地址

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysPageNum(pub usize);    // 物理页号

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(pub usize);    // 虚拟页号
```

**这就是 Rust 的"新类型模式" (newtype pattern)：**

在 C 中：
```c
// xv6: 全是 uint64，容易混淆
uint64 pa = 0x80000000;
uint64 va = 0x80000000;
uint64 ppn = pa >> 12;
// 不小心把 pa 当 ppn 用？编译器不报错！
```

在 Rust 中：
```rust
let pa = PhysAddr(0x80000000);
let va = VirtAddr(0x80000000);
// let ppn: PhysPageNum = pa;  // 编译错误！类型不匹配
let ppn: PhysPageNum = pa.into();  // 必须显式转换
```

### 类型转换通过 From/Into trait

```rust
impl From<PhysAddr> for PhysPageNum {
    fn from(v: PhysAddr) -> Self {
        assert_eq!(v.page_offset(), 0);  // 确保页对齐
        v.floor()
    }
}

impl From<PhysPageNum> for PhysAddr {
    fn from(v: PhysPageNum) -> Self {
        Self(v.0 << PAGE_SIZE_BITS)
    }
}
```

---

## 4.4 SV39 页表 — 与 xv6 完全一致

### 页表项 (PTE)

```rust
// os/src/mm/page_table.rs
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PageTableEntry {
    pub bits: usize,
}

// 页表项标志位
bitflags! {
    pub struct PTEFlags: u8 {
        const V = 1 << 0;   // Valid
        const R = 1 << 1;   // Readable
        const W = 1 << 2;   // Writable
        const X = 1 << 3;   // Executable
        const U = 1 << 4;   // User accessible
        const G = 1 << 5;   // Global
        const A = 1 << 6;   // Accessed
        const D = 1 << 7;   // Dirty
    }
}
```

**对比 xv6：**
```c
// xv6 kernel/riscv.h
#define PTE_V (1L << 0)
#define PTE_R (1L << 1)
#define PTE_W (1L << 2)
#define PTE_X (1L << 3)
#define PTE_U (1L << 4)
```

rCore 使用 `bitflags!` 宏定义标志位，比 C 的 `#define` 更安全：
```rust
let flags = PTEFlags::R | PTEFlags::W;  // 类型安全的位运算
// flags | 0x100;  // 编译错误！不能和整数混用
```

### SV39 三级页表结构

```
虚拟地址 (39 位):
┌──────────┬──────────┬──────────┬──────────┐
│ VPN[2]   │ VPN[1]   │ VPN[0]   │ Offset   │
│ 9 bits   │ 9 bits   │ 9 bits   │ 12 bits  │
└──────────┴──────────┴──────────┴──────────┘
     ↓           ↓           ↓
  一级页表    二级页表    三级页表 → 物理页号
```

与 xv6 完全一样的三级查表过程。

### 页表实现

```rust
pub struct PageTable {
    root_ppn: PhysPageNum,          // 根页表物理页号
    frames: Vec<FrameTracker>,      // 页表占用的物理帧（RAII 管理！）
}

impl PageTable {
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],    // frame 的所有权移入 Vec
        }
    }
    // PageTable 被 drop 时，frames 中所有 FrameTracker 也被 drop
    // → 自动调用 frame_dealloc 归还物理帧！
}
```

**RAII 的威力：**
- xv6: 需要手动遍历页表，逐个 `kfree()` 页表页
- rCore: `PageTable` drop 时自动释放所有页表页！

---

## 4.5 物理帧分配器

```rust
// os/src/mm/frame_allocator.rs
pub struct StackFrameAllocator {
    current: usize,          // 下一个可分配的物理页号
    end: usize,              // 可分配范围的结束
    recycled: Vec<usize>,    // 已回收的页号栈
}

impl FrameAllocator for StackFrameAllocator {
    fn alloc(&mut self) -> Option<PhysPageNum> {
        if let Some(ppn) = self.recycled.pop() {
            Some(ppn.into())                  // 优先复用回收的帧
        } else if self.current == self.end {
            None                               // 没有可用帧
        } else {
            self.current += 1;
            Some((self.current - 1).into())   // 分配新帧
        }
    }
    fn dealloc(&mut self, ppn: PhysPageNum) {
        // 检查是否重复释放
        if ppn.0 >= self.current || self.recycled.iter().any(|v| *v == ppn.0) {
            panic!("Frame ppn={:#x} has not been allocated!", ppn.0);
        }
        self.recycled.push(ppn.0);
    }
}
```

**对比 xv6 的 `kalloc.c`：** xv6 用链表管理空闲页，rCore 用栈（Vec）。

### FrameTracker — RAII 的关键

```rust
pub struct FrameTracker {
    pub ppn: PhysPageNum,
}

impl FrameTracker {
    pub fn new(ppn: PhysPageNum) -> Self {
        // 分配时清零页面
        let bytes_array = ppn.get_bytes_array();
        for i in bytes_array { *i = 0; }
        Self { ppn }
    }
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        frame_dealloc(self.ppn);   // 所有者离开作用域时自动释放！
    }
}
```

这是 Rust 最精华的模式之一：
```rust
{
    let frame = frame_alloc().unwrap();   // 分配一个物理帧
    // 使用 frame...
}   // frame 离开作用域，自动调用 Drop::drop → frame_dealloc
    // 永远不会忘记释放！
```

---

## 4.6 内核堆分配器

```rust
// os/src/mm/heap_allocator.rs
#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

pub fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR.lock().init(
            HEAP_SPACE.as_ptr() as usize,
            KERNEL_HEAP_SIZE,
        );
    }
}
```

有了堆分配器，内核就能使用 `alloc` crate 提供的：
- `Vec<T>` — 动态数组
- `String` — 动态字符串
- `Box<T>` — 堆上分配
- `Arc<T>` — 引用计数指针

---

## 4.7 MemorySet — 地址空间抽象

```rust
// os/src/mm/memory_set.rs
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>,          // 这个地址空间的所有映射区域
}

pub struct MapArea {
    vpn_range: VPNRange,          // 虚拟页号范围
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,  // VPN → 物理帧
    map_type: MapType,            // 映射类型
    map_perm: MapPermission,      // 权限
}

pub enum MapType {
    Identical,    // 恒等映射（虚拟地址 = 物理地址，内核用）
    Framed,       // 分帧映射（每个虚拟页分配新物理帧，用户用）
}
```

### 内核地址空间布局

```
┌─────────────────────┐ 高地址
│   Trampoline        │ ← 最高虚拟页（跳板，用于 Trap 切换）
├─────────────────────┤
│   Kernel Stacks     │ ← 每个应用一个内核栈（guard page 隔离）
├─────────────────────┤
│   Physical Memory   │ ← 恒等映射（VA = PA）
│   (.text .data .bss)│
├─────────────────────┤
│   MMIO              │ ← 设备寄存器映射
└─────────────────────┘ 0x80000000
```

### 用户地址空间布局

```
┌─────────────────────┐ 高地址
│   Trampoline        │ ← 与内核相同的虚拟地址（关键！）
├─────────────────────┤
│   TrapContext        │ ← 次高页，存放用户的 Trap 上下文
├─────────────────────┤
│   User Stack         │
├─────────────────────┤
│   (Guard Page)       │ ← 不映射，用于检测栈溢出
├─────────────────────┤
│   User Heap          │
├─────────────────────┤
│   .data / .bss       │
├─────────────────────┤
│   .text (代码段)     │ ← 低地址
└─────────────────────┘
```

---

## 4.8 Trampoline 跳板页 — 与 xv6 思路相同

**问题**：切换页表（`satp`）后，PC 指向的虚拟地址可能无效

**解决**：在所有地址空间中，把 Trap 处理代码映射到**相同的虚拟地址**

```rust
// 映射跳板页（内核和用户地址空间都要做）
fn map_trampoline(&mut self) {
    self.page_table.map(
        VirtAddr::from(TRAMPOLINE).into(),       // 虚拟地址：最高页
        PhysAddr::from(strampoline as usize).into(), // 物理地址：trap.S
        PTEFlags::R | PTEFlags::X,
    );
}
```

---

## 4.9 关键 Rust 知识点

### From/Into trait — 类型安全转换
```rust
// 虚拟地址 → 虚拟页号
impl From<VirtAddr> for VirtPageNum {
    fn from(v: VirtAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.floor()
    }
}

// 使用
let vpn: VirtPageNum = va.into();
let vpn = VirtPageNum::from(va);  // 等价
```

### BTreeMap — 有序映射
```rust
data_frames: BTreeMap<VirtPageNum, FrameTracker>
// VPN → 物理帧的映射
// 当 MapArea 被 drop 时，BTreeMap 中所有 FrameTracker 也被 drop
// → 自动释放所有物理帧！
```

### derive 宏
```rust
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(pub usize);
```
- `Copy, Clone`：可以按值复制（地址是个数字，复制很便宜）
- `Ord, PartialOrd`：可以比较大小
- `Eq, PartialEq`：可以判断相等

---

## 4.10 对比总结：xv6 vs rCore 内存管理

| 功能 | xv6 (C) | rCore (Rust) |
|------|---------|-------------|
| 页号类型 | `uint64` (易混淆) | `PhysPageNum` / `VirtPageNum` (类型安全) |
| 帧分配 | `kalloc()` / `kfree()` | `frame_alloc()` + RAII (`FrameTracker`) |
| 帧释放 | 手动 `kfree()`，忘了=泄漏 | 自动 `Drop`，不可能忘记 |
| 页表 | `walk()` / `mappages()` | `PageTable::find_pte_create()` / `map()` |
| 页表释放 | `freewalk()` 手动递归 | `PageTable` drop 时自动 |
| 地址空间 | `pagetable_t` + 散落函数 | `MemorySet` 结构化管理 |

---

## 4.11 思考题

1. 为什么 `MapType::Identical`（恒等映射）对内核很重要？
   > 提示：内核在开启页表前后，代码地址不能变

2. `FrameTracker` 的 `Drop` trait 实现是如何防止内存泄漏的？

3. Guard Page 是怎么检测栈溢出的？（提示：不映射 → 访问时缺页异常）

4. 为什么 Trampoline 要映射到所有地址空间的相同虚拟地址？

5. `BTreeMap<VirtPageNum, FrameTracker>` 被 drop 时会发生什么？
