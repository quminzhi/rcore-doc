# Ch3: 多道程序与分时多任务

> 对应分支：`origin/ch3`
> 对应 xv6：`scheduler()` + `swtch.S` + 时钟中断
> 预计时间：2-3 天

---

## 3.1 本章目标

从 ch2 的"一次只跑一个"升级为**多个程序同时驻留内存，分时切换**。

核心能力：
- **任务切换** — 保存/恢复任务上下文
- **协作式调度** — 任务主动 yield
- **抢占式调度** — 时钟中断强制切换

---

## 3.2 新增/修改文件

```
os/src/
├── task/
│   ├── mod.rs         # TaskManager 全局任务管理器
│   ├── context.rs     # TaskContext（任务上下文）
│   ├── switch.rs      # __switch 的 Rust 封装
│   ├── switch.S       # 上下文切换汇编
│   └── task.rs        # TaskControlBlock 定义
├── timer.rs           # 时钟中断设置
├── config.rs          # 系统常量配置
├── loader.rs          # 加载多个应用（替代 batch.rs）
└── boards/qemu.rs     # QEMU 平台相关常量
```

---

## 3.3 核心概念：任务上下文 vs Trap 上下文

```
┌─────────────────────────────────────────────────────┐
│                                                       │
│  TrapContext (trap/context.rs)                        │
│  ─ 保存: 用户态 → 内核态 (ecall/中断)                 │
│  ─ 内容: 全部 32 个通用寄存器 + sstatus + sepc        │
│  ─ 对标 xv6: struct trapframe                         │
│                                                       │
│  TaskContext (task/context.rs)                        │
│  ─ 保存: 内核态任务A → 内核态任务B (__switch)          │
│  ─ 内容: ra + sp + s0~s11 (被调用者保存寄存器)        │
│  ─ 对标 xv6: struct context                           │
│                                                       │
└─────────────────────────────────────────────────────┘
```

---

## 3.4 任务上下文

```rust
// os/src/task/context.rs
#[derive(Copy, Clone)]
#[repr(C)]
pub struct TaskContext {
    ra: usize,          // 返回地址（__switch 返回后跳到哪）
    sp: usize,          // 内核栈指针
    s: [usize; 12],     // s0~s11 被调用者保存寄存器
}
```

**对比 xv6 的 `struct context`：**
```c
// xv6 kernel/proc.h
struct context {
    uint64 ra;
    uint64 sp;
    uint64 s0;
    uint64 s1;
    // ... s2~s11
};
```

完全一样！因为 RISC-V 调用约定：调用者保存 a0-a7, t0-t6；被调用者保存 s0-s11, ra, sp。

---

## 3.5 __switch — 上下文切换（最核心！）

```
__switch(current_task_cx_ptr, next_task_cx_ptr)
```

执行流程：
```
任务 A 内核态                        任务 B 内核态
    │                                   │
    ├─ 保存 A 的 ra, sp, s0~s11        │
    │  到 current_task_cx_ptr          │
    │                                   │
    ├─ 从 next_task_cx_ptr             │
    │  恢复 B 的 ra, sp, s0~s11        │
    │                                   │
    └─ ret (跳到 B 的 ra) ─────────────→│
                                        ├─ B 继续执行
```

**对比 xv6 的 `swtch.S`：**
```asm
# xv6 swtch.S — 几乎一模一样！
.globl swtch
swtch:
    sd ra, 0(a0)     # 保存到 old context
    sd sp, 8(a0)
    sd s0, 16(a0)
    # ...
    ld ra, 0(a1)     # 从 new context 恢复
    ld sp, 8(a1)
    ld s0, 16(a1)
    # ...
    ret
```

**为什么只保存被调用者保存寄存器？**
- `__switch` 是一个"函数调用"
- 调用者保存的寄存器已经由编译器在调用前保存了
- 这是 RISC-V ABI 的约定

---

## 3.6 TaskControlBlock — 任务控制块

```rust
// os/src/task/task.rs
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    pub task_status: TaskStatus,
    pub task_cx: TaskContext,
}

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    UnInit,     // 未初始化
    Ready,      // 就绪
    Running,    // 运行中
    Exited,     // 已退出
}
```

对比 xv6 的 `struct proc`，rCore ch3 的 TCB 非常简洁，只有状态和上下文。

---

## 3.7 TaskManager — 全局任务管理器

```rust
// os/src/task/mod.rs
pub struct TaskManager {
    num_app: usize,
    inner: UPSafeCell<TaskManagerInner>,
}

pub struct TaskManagerInner {
    tasks: [TaskControlBlock; MAX_APP_NUM],   // 任务数组
    current_task: usize,                       // 当前运行的任务
}

lazy_static! {
    pub static ref TASK_MANAGER: TaskManager = {
        let num_app = get_num_app();
        let mut tasks = [TaskControlBlock {
            task_cx: TaskContext::zero_init(),
            task_status: TaskStatus::UnInit,
        }; MAX_APP_NUM];

        // 初始化每个任务的上下文
        for (i, task) in tasks.iter_mut().enumerate() {
            task.task_cx = TaskContext::goto_restore(init_app_cx(i));
            task.task_status = TaskStatus::Ready;
        }
        TaskManager { num_app, inner: unsafe { UPSafeCell::new(TaskManagerInner {
            tasks, current_task: 0,
        }) } }
    };
}
```

### 初始化技巧

```rust
impl TaskContext {
    pub fn goto_restore(kstack_ptr: usize) -> Self {
        unsafe extern "C" {
            unsafe fn __restore();
        }
        Self {
            ra: __restore as usize,    // __switch 返回后跳到 __restore
            sp: kstack_ptr,            // 内核栈顶（存放了 TrapContext）
            s: [0; 12],
        }
    }
}
```

任务第一次被调度时：
1. `__switch` 恢复 TaskContext
2. `ret` 跳到 `ra` = `__restore`
3. `__restore` 从内核栈恢复 TrapContext
4. `sret` 跳到用户程序入口

---

## 3.8 调度流程

### 协作式：yield 主动让出

```rust
pub fn suspend_current_and_run_next() {
    mark_current_suspended();    // 当前任务状态 → Ready
    run_next_task();             // 找到下一个 Ready 任务，__switch
}
```

### 抢占式：时钟中断

```rust
// os/src/trap/mod.rs — 处理时钟中断
Trap::Interrupt(Interrupt::SupervisorTimer) => {
    set_next_trigger();                       // 设置下一次定时器
    suspend_current_and_run_next();           // 强制切换！
}
```

```rust
// os/src/timer.rs
const TICKS_PER_SEC: usize = 100;     // 每秒 100 次中断

pub fn set_next_trigger() {
    set_timer(get_time() + CLOCK_FREQ / TICKS_PER_SEC);  // 10ms 后触发
}
```

**对比 xv6：**
```c
// xv6 kernel/trap.c
void kerneltrap() {
    if (which_dev == 2) {  // timer interrupt
        yield();           // 强制让出
    }
}
```

---

## 3.9 find_next_task — 轮转调度

```rust
fn find_next_task(&self) -> Option<usize> {
    let inner = self.inner.exclusive_access();
    let current = inner.current_task;
    (current + 1..current + self.num_app + 1)    // 从当前任务的下一个开始
        .map(|id| id % self.num_app)              // 取模实现循环
        .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)
        // 找到第一个 Ready 的任务
}
```

**Rust 迭代器链式调用讲解：**
```
假设 num_app=4, current=1

(2..5)                              → [2, 3, 4]
.map(|id| id % 4)                   → [2, 3, 0]
.find(|id| tasks[*id] == Ready)     → 返回第一个 Ready 的 id
```

对比 C 写法：
```c
int find_next_task() {
    for (int i = current + 1; i < current + num_app + 1; i++) {
        int id = i % num_app;
        if (tasks[id].status == READY) return id;
    }
    return -1;
}
```

---

## 3.10 完整切换流程图

```
任务 A (U-mode)                    任务 B (U-mode)
    │                                   │
    │ 时钟中断                           │
    ↓                                   │
__alltraps (保存用户寄存器)             │
    ↓                                   │
trap_handler                            │
    → set_next_trigger()                │
    → suspend_current_and_run_next()    │
        → mark_current_suspended()      │
        → run_next_task()               │
            → __switch(A_cx, B_cx)      │
                                        ↓
                        (恢复 B 的内核上下文)
                                        ↓
                                    __restore (恢复 B 的用户寄存器)
                                        ↓
                                    sret 回到 B 用户态
```

---

## 3.11 内存布局（多道程序）

```
┌────────────────────┐ 高地址
│  App 3             │ 0x80400000 + 3 * 0x20000
├────────────────────┤
│  App 2             │ 0x80400000 + 2 * 0x20000
├────────────────────┤
│  App 1             │ 0x80400000 + 1 * 0x20000
├────────────────────┤
│  App 0             │ 0x80400000
├────────────────────┤
│  Kernel            │ 0x80200000
├────────────────────┤
│  SBI               │ 0x80000000
└────────────────────┘
```

每个应用占固定大小 `APP_SIZE_LIMIT = 0x20000` (128KB)。

---

## 3.12 Rust 知识点总结

| 特性 | 本章用途 |
|------|---------|
| `#[derive(Copy, Clone)]` | TaskControlBlock 需要可复制 |
| `iter_mut().enumerate()` | 遍历并修改任务数组 |
| `.map().find()` | 函数式风格查找下一个任务 |
| `Option<usize>` | find_next_task 可能找不到 |
| `if let Some(next)` | 模式匹配处理 Option |
| `drop()` | 手动释放 UPSafeCell 守卫 |
| `as *mut TaskContext` | 获取裸指针传给汇编 |

---

## 3.13 思考题

1. `__switch` 为什么不保存 `a0-a7` 和 `t0-t6`？
2. 第一个任务被 `__switch` 到时，它的 `ra` 是 `__restore`，为什么？
3. 如果一个任务 panic 了，会发生什么？其他任务还能运行吗？
4. 当前的调度算法是什么？公平吗？（提示：Round-Robin）
5. `drop(inner)` 在 `__switch` 之前被调用，为什么这很重要？
