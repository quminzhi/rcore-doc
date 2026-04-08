# Ch2: 批处理系统

> 对应分支：`origin/ch2`
> 对应 xv6：最简单的用户程序加载 + 特权级切换
> 预计时间：2 天

---

## 2.1 本章目标

实现一个**批处理系统**：依次加载并运行多个用户程序，一个跑完再跑下一个。

核心能力：
- 实现 **S-mode ↔ U-mode 特权级切换**
- 实现 **Trap 处理**（系统调用、异常）
- 加载用户程序到固定地址运行

---

## 2.2 新增文件

```
os/src/
├── batch.rs            # 批处理管理器（加载和运行应用）
├── trap/
│   ├── mod.rs          # Trap 处理入口
│   ├── context.rs      # Trap 上下文（寄存器保存）
│   └── trap.S          # Trap 汇编（保存/恢复寄存器）
├── syscall/
│   ├── mod.rs          # 系统调用分发
│   ├── fs.rs           # sys_write
│   └── process.rs      # sys_exit
└── sync/
    ├── mod.rs
    └── up.rs           # UPSafeCell（单核安全容器）
```

---

## 2.3 核心概念：特权级切换

```
    用户程序 (U-mode)
         │
    ecall (系统调用) 或 异常
         ↓
    ┌──────────────────────┐
    │  __alltraps (trap.S)  │  保存所有寄存器到 TrapContext
    │         ↓             │
    │  trap_handler (Rust)  │  根据 scause 分发处理
    │         ↓             │
    │  __restore (trap.S)   │  恢复寄存器，sret 返回用户态
    └──────────────────────┘
    内核 (S-mode)
```

### 对比 xv6
| xv6 | rCore |
|-----|-------|
| `trampoline.S` (uservec/userret) | `trap.S` (__alltraps/__restore) |
| `usertrap()` / `usertrapret()` | `trap_handler()` |
| `struct trapframe` | `TrapContext` |

---

## 2.4 关键代码解读

### TrapContext — 保存 CPU 状态

```rust
// os/src/trap/context.rs
#[repr(C)]                    // C 内存布局，确保与汇编对应
pub struct TrapContext {
    pub x: [usize; 32],      // 32 个通用寄存器
    pub sstatus: Sstatus,     // S-mode 状态寄存器
    pub sepc: usize,          // 触发 Trap 时的 PC
}

impl TrapContext {
    pub fn set_sp(&mut self, sp: usize) {
        self.x[2] = sp;      // x2 = sp
    }

    // 创建用户程序初始上下文
    pub fn app_init_context(entry: usize, sp: usize) -> Self {
        let mut sstatus = sstatus::read();
        sstatus.set_spp(SPP::User);   // 设置 sret 后回到 U-mode
        let mut cx = Self {
            x: [0; 32],
            sstatus,
            sepc: entry,               // sret 后跳转到用户程序入口
        };
        cx.set_sp(sp);                 // 设置用户栈指针
        cx
    }
}
```

**对比 xv6 的 `trapframe`：**
```c
// xv6 kernel/proc.h
struct trapframe {
    uint64 kernel_satp;    // rCore 不需要（此阶段无虚拟内存）
    uint64 kernel_sp;
    uint64 epc;           // 对应 sepc
    // ... 32 个通用寄存器
};
```

---

### trap_handler — Trap 处理

```rust
// os/src/trap/mod.rs
pub fn trap_handler(cx: &mut TrapContext) -> &mut TrapContext {
    let scause = scause::read();     // 读取 Trap 原因
    let stval = stval::read();       // 读取 Trap 附加信息

    match scause.cause() {
        // 系统调用
        Trap::Exception(Exception::UserEnvCall) => {
            cx.sepc += 4;            // ecall 指令长 4 字节，返回时跳过
            cx.x[10] = syscall(cx.x[17], [cx.x[10], cx.x[11], cx.x[12]]) as usize;
            //              a7=syscall号    a0        a1        a2  参数
            //  返回值写入 a0 (x[10])
        }
        // 页错误 → 杀死程序，运行下一个
        Trap::Exception(Exception::StoreFault)
        | Trap::Exception(Exception::StorePageFault) => {
            println!("[kernel] PageFault in application, kernel killed it.");
            run_next_app();
        }
        // 非法指令 → 杀死程序
        Trap::Exception(Exception::IllegalInstruction) => {
            println!("[kernel] IllegalInstruction, kernel killed it.");
            run_next_app();
        }
        _ => {
            panic!("Unsupported trap {:?}, stval = {:#x}!", scause.cause(), stval);
        }
    }
    cx
}
```

**Rust 语法重点：**
- `match` 是穷举的！必须处理所有情况（`_` 是默认分支）
- `scause.cause()` 返回枚举类型，pattern matching 非常直观
- 对比 C 的 `if-else if` 链，Rust 的 match 更清晰

---

### syscall 分发

```rust
// os/src/syscall/mod.rs
const SYSCALL_WRITE: usize = 64;
const SYSCALL_EXIT: usize = 93;

pub fn syscall(syscall_id: usize, args: [usize; 3]) -> isize {
    match syscall_id {
        SYSCALL_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        SYSCALL_EXIT  => sys_exit(args[0] as i32),
        _ => panic!("Unsupported syscall_id: {}", syscall_id),
    }
}
```

RISC-V 系统调用约定（与 xv6 相同）：
- `a7` (x17)：系统调用号
- `a0-a2` (x10-x12)：参数
- `a0` (x10)：返回值

---

### batch.rs — 批处理管理器

```rust
// 核心数据结构
struct AppManager {
    num_app: usize,              // 应用总数
    current_app: usize,          // 当前运行的应用编号
    app_start: [usize; MAX_APP_NUM + 1],  // 每个应用的起始地址
}

// 使用 lazy_static 初始化全局变量
lazy_static! {
    static ref APP_MANAGER: UPSafeCell<AppManager> = unsafe {
        UPSafeCell::new({
            // 从 link_app.S 中读取应用信息
            // ...
        })
    };
}
```

**关键 Rust 概念：`lazy_static!`**
- C 中全局变量在编译时初始化
- Rust 的 `static` 要求编译时常量
- `lazy_static!` 允许运行时初始化全局变量（首次访问时初始化）

**关键 Rust 概念：`UPSafeCell`**
```rust
// os/src/sync/up.rs
pub struct UPSafeCell<T> {
    inner: RefCell<T>,
}

unsafe impl<T> Sync for UPSafeCell<T> {}  // 手动标记为线程安全

impl<T> UPSafeCell<T> {
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        self.inner.borrow_mut()    // 运行时借用检查
    }
}
```

为什么需要 `UPSafeCell`？
- Rust 禁止全局可变变量（`static mut` 不安全）
- `RefCell` 提供"内部可变性"（通过 `&self` 也能修改）
- 单核处理器（UP = Uniprocessor）上手动标记 Sync 是安全的
- 对比 xv6：直接用全局变量 + spinlock

---

### 加载和运行应用

```rust
impl AppManager {
    // 将应用二进制复制到 0x80400000
    fn load_app(&self, app_id: usize) {
        // 清零目标区域
        unsafe {
            core::slice::from_raw_parts_mut(
                APP_BASE_ADDRESS as *mut u8, APP_SIZE_LIMIT
            ).fill(0);
            // 复制应用代码
            let app_src = core::slice::from_raw_parts(
                self.app_start[app_id] as *const u8,
                self.app_start[app_id + 1] - self.app_start[app_id],
            );
            let app_dst = core::slice::from_raw_parts_mut(
                APP_BASE_ADDRESS as *mut u8, app_src.len()
            );
            app_dst.copy_from_slice(app_src);
            asm!("fence.i");       // 刷新指令缓存
        }
    }
}

// 运行下一个应用
pub fn run_next_app() -> ! {
    let mut app_manager = APP_MANAGER.exclusive_access();
    let current_app = app_manager.get_current_app();
    app_manager.load_app(current_app);
    app_manager.move_to_next_app();
    drop(app_manager);       // 手动释放锁！重要！
    unsafe {
        __restore(KERNEL_STACK.push_context(
            TrapContext::app_init_context(APP_BASE_ADDRESS, USER_STACK.get_sp())
        ) as *const _ as usize);
    }
    panic!("Unreachable!");
}
```

**注意 `drop(app_manager)`：**
- `exclusive_access()` 返回 `RefMut` 守卫
- 如果不手动 drop，`__restore` 永远不返回，守卫永远不释放
- 对比 xv6：手动调用 `release(&lock)`

---

## 2.5 内存布局

```
┌─────────────────────┐ 0x80400000 + APP_SIZE_LIMIT
│   User Stack        │
├─────────────────────┤
│   User App Code     │ ← 0x80400000 (APP_BASE_ADDRESS)
├─────────────────────┤
│   Kernel Stack      │
├─────────────────────┤
│   Kernel Code       │ ← 0x80200000
├─────────────────────┤
│   SBI (RustSBI)     │ ← 0x80000000
└─────────────────────┘
```

---

## 2.6 完整执行流程

```
1. rust_main()
   → trap::init()        // 设置 stvec = __alltraps
   → batch::init()       // 读取应用信息
   → batch::run_next_app()
     → load_app(0)       // 加载第一个应用到 0x80400000
     → __restore()       // sret 到用户态

2. 用户程序运行在 U-mode
   → ecall (系统调用)
     → __alltraps        // 保存寄存器
     → trap_handler()    // 处理系统调用
     → __restore         // 恢复寄存器，sret 回用户态

3. 用户程序调用 sys_exit
   → run_next_app()      // 加载下一个应用
   → 循环直到所有应用运行完毕
   → shutdown()
```

---

## 2.7 Rust 知识点总结

| 特性 | 本章用途 |
|------|---------|
| `#[repr(C)]` | TrapContext 与汇编布局对齐 |
| `lazy_static!` | 运行时初始化全局 APP_MANAGER |
| `RefCell` / `UPSafeCell` | 内部可变性，替代 `static mut` |
| `match` 枚举 | Trap 原因分发 |
| `drop()` | 手动释放守卫 |
| `unsafe` 块 | 裸指针操作、内联汇编 |
| `core::slice::from_raw_parts` | 从裸指针创建切片 |

---

## 2.8 思考题

1. 为什么 `cx.sepc += 4`？如果 Trap 是由异常（如缺页）触发的，需要 +4 吗？
2. `UPSafeCell` 在多核环境下安全吗？为什么？
3. 为什么需要 `asm!("fence.i")`？不加会怎样？
4. 批处理系统有什么局限性？（提示：想想一个恶意程序死循环会怎样）
