# Ch0: Rust 语言最小必备知识

> 目标：不求学完 Rust，只掌握 rCore 项目中用到的核心特性。
> 预计时间：3-5 天

---

## 0.1 为什么选 Rust 写 OS？

| 特性 | C (xv6) | Rust (rCore) |
|------|---------|-------------|
| 内存安全 | 手动 malloc/free，容易 use-after-free | 所有权系统，编译期保证 |
| 并发安全 | 手动加锁，容易忘记 release | Send/Sync trait，编译器检查 |
| 空指针 | NULL 到处飞 | Option<T> 强制处理 |
| 裸机编程 | 天然支持 | `#![no_std]` 去掉标准库即可 |

---

## 0.2 所有权 (Ownership) — 最核心概念

### C 的问题
```c
// xv6 中常见模式
char *p = kalloc();
kfree(p);
// p 现在是悬空指针(dangling pointer)，但编译器不报错！
*p = 'x';  // 未定义行为
```

### Rust 的解决方案
```rust
fn main() {
    let s1 = String::from("hello");
    let s2 = s1;          // s1 的所有权"移动"到 s2
    // println!("{}", s1); // 编译错误！s1 已经无效
    println!("{}", s2);    // OK
}
```

### 三条规则
1. **每个值有且只有一个所有者 (owner)**
2. **当所有者离开作用域，值被自动释放 (Drop)**
3. **赋值 = 移动 (move)，而不是拷贝**

### 在 rCore 中的应用
```rust
// os/src/mm/frame_allocator.rs
pub struct FrameTracker {   // 物理帧的"所有者"
    pub ppn: PhysPageNum,
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        frame_dealloc(self.ppn);  // 离开作用域时自动归还物理帧！
    }
}
// 对比 xv6：需要手动 kfree()，忘了就内存泄漏
```

---

## 0.3 借用 (Borrowing) — 不转移所有权的访问

```rust
fn print_len(s: &String) {   // & 表示借用（只读引用）
    println!("len = {}", s.len());
}   // s 离开作用域，但它只是引用，不会释放原始数据

fn append(s: &mut String) {  // &mut 表示可变借用
    s.push_str(" world");
}

fn main() {
    let mut s = String::from("hello");
    print_len(&s);        // 不可变借用
    append(&mut s);       // 可变借用
    println!("{}", s);    // "hello world"
}
```

### 借用规则
- **任意时刻：要么有多个 `&T`（只读），要么有一个 `&mut T`（读写）**
- 类比 C：`const T*`（多个只读指针） vs `T*`（独占写指针）

---

## 0.4 struct + impl — 替代 C 的 struct + 函数

### C (xv6)
```c
struct proc {
    int pid;
    enum procstate state;
    pagetable_t pagetable;
};

void proc_init(struct proc *p) {
    p->pid = allocpid();
}
```

### Rust (rCore)
```rust
pub struct TaskControlBlock {
    pub pid: PidHandle,
    pub task_status: TaskStatus,
}

impl TaskControlBlock {
    pub fn new() -> Self {           // 关联函数（类似构造函数）
        Self {
            pid: pid_alloc(),
            task_status: TaskStatus::Ready,
        }
    }

    pub fn get_pid(&self) -> usize { // 方法（&self = 只读借用自身）
        self.pid.0
    }
}
```

---

## 0.5 trait — 替代 C 的函数指针表

### C (xv6) — 函数指针
```c
struct file_operations {
    int (*read)(struct file*, char*, int);
    int (*write)(struct file*, char*, int);
};
```

### Rust (rCore) — trait
```rust
// os/src/fs/mod.rs
pub trait File: Send + Sync {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(&self, buf: UserBuffer) -> usize;
    fn write(&self, buf: UserBuffer) -> usize;
}

// Stdin, Stdout, Pipe 都实现了 File trait
impl File for Stdin {
    fn readable(&self) -> bool { true }
    fn writable(&self) -> bool { false }
    fn read(&self, buf: UserBuffer) -> usize { /* ... */ }
    fn write(&self, _buf: UserBuffer) -> usize { panic!("Cannot write to stdin!"); }
}
```

---

## 0.6 枚举 enum + match — 比 C 的 switch 强大得多

### C
```c
enum procstate { UNUSED, USED, SLEEPING, RUNNABLE, RUNNING, ZOMBIE };

switch(p->state) {
    case RUNNABLE: /* ... */ break;
    case SLEEPING: /* ... */ break;
    default: break;
}
```

### Rust
```rust
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    Ready,
    Running,
    Blocked,
}

// match 必须穷举所有变体，编译器强制检查！
match task.task_status {
    TaskStatus::Ready   => { /* ... */ }
    TaskStatus::Running => { /* ... */ }
    TaskStatus::Blocked => { /* ... */ }
}
```

### 枚举还能携带数据（C 的 union 替代品）
```rust
enum Option<T> {         // Rust 标准库自带
    Some(T),             // 有值
    None,                // 无值（替代 NULL）
}

// 使用时必须处理 None，不可能出现空指针！
match pid_alloc() {
    Some(pid) => println!("got pid: {}", pid),
    None      => println!("allocation failed"),
}
```

---

## 0.7 unsafe — "我知道我在做什么"

Rust 编译器无法验证某些操作的安全性时，需要用 `unsafe` 标记：

```rust
// os/src/mm/address.rs — 直接操作物理内存
impl PhysPageNum {
    pub fn get_mut<T>(&self) -> &mut T {
        let pa: PhysAddr = (*self).into();
        unsafe {
            (pa.0 as *mut T).as_mut().unwrap()   // 裸指针操作
        }
    }
}
```

### unsafe 允许做的事：
1. 解引用裸指针 (`*const T`, `*mut T`)
2. 调用 unsafe 函数
3. 访问可变静态变量
4. 实现 unsafe trait

> 类比：C 的所有代码都是 "unsafe" 的，Rust 只在必要时用 unsafe

---

## 0.8 宏 macro_rules! — 比 C 的 #define 强大

```rust
// os/src/console.rs — 实现 println! 宏
#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?));
    }
}
```

| C 宏 | Rust 宏 |
|------|--------|
| 文本替换 | 语法树匹配 |
| 无类型检查 | 有类型检查 |
| 容易出 bug | 编译器报错精确 |

---

## 0.9 no_std — 裸机编程必备

```rust
#![no_std]    // 不使用标准库（std），只用核心库（core）
#![no_main]   // 不用标准的 main 入口
```

| 标准库 (std) | 核心库 (core) | 说明 |
|-------------|--------------|------|
| `println!` | 不可用 | 需要自己实现 |
| `Vec`, `String` | 不可用 | 需要 `alloc` crate + 自定义分配器 |
| `HashMap` | 不可用 | 需要 `alloc` |
| 基本类型、trait | 可用 | `Option`, `Result`, `Copy`, `Clone` 等 |

---

## 0.10 常用智能指针

| 类型 | C 对应 | 用途 | rCore 使用场景 |
|------|-------|------|---------------|
| `Box<T>` | `malloc` + 单一所有者 | 堆上分配 | 较少直接使用 |
| `Arc<T>` | 引用计数指针 | 多所有者共享 | 进程/线程控制块 |
| `Weak<T>` | 弱引用 | 避免循环引用 | 子线程引用父进程 |
| `RefCell<T>` | 运行时借用检查 | 内部可变性 | `UPSafeCell` 的核心 |

```rust
// 在 rCore 中最常见的模式
use alloc::sync::Arc;

let process = Arc::new(ProcessControlBlock::new());
let cloned = Arc::clone(&process);  // 引用计数 +1
// 两个变量指向同一个 PCB，离开作用域时自动 -1
```

---

## 0.11 迭代器和闭包 — Rust 的函数式风格

```rust
// C 风格循环
for i in 0..self.num_app {
    println!("app_{}", i);
}

// Rust 迭代器 + 闭包（rCore 中大量使用）
(current + 1..current + self.num_app + 1)
    .map(|id| id % self.num_app)                          // 闭包：|参数| 表达式
    .find(|id| inner.tasks[*id].task_status == TaskStatus::Ready)  // 找到第一个 Ready 的
```

---

## 0.12 推荐学习路径

1. **第 1 天**：所有权、借用、生命周期 → [Rust 语言圣经 Ch4](https://course.rs/basic/ownership/index.html)
2. **第 2 天**：struct、enum、match、impl → [Rust 语言圣经 Ch5-6](https://course.rs/basic/compound-type/struct.html)
3. **第 3 天**：trait、泛型 → [Rust 语言圣经 Ch10](https://course.rs/basic/trait/trait.html)
4. **第 4 天**：智能指针 (Box, Arc, RefCell) → [Rust 语言圣经 Ch15](https://course.rs/advance/smart-pointer/box.html)
5. **第 5 天**：unsafe、宏 → 直接看 rCore ch1 代码学习

> 碰到不懂的语法，随时问我，我会用 C/xv6 类比解释！

---

## 0.13 Rust 速查对照表

| C / xv6 | Rust / rCore | 说明 |
|---------|-------------|------|
| `int x = 5;` | `let x: i32 = 5;` | 变量默认不可变 |
| `int x = 5; x = 6;` | `let mut x = 5; x = 6;` | 需要 `mut` 才可变 |
| `void foo(int *p)` | `fn foo(p: &mut i32)` | 可变引用 |
| `struct proc *p = kalloc(sizeof(*p))` | `let p = Box::new(Proc::new())` | 堆分配 |
| `free(p)` | 自动（Drop trait） | RAII |
| `NULL` | `None` (Option 类型) | 编译期检查 |
| `(void*)ptr` | `ptr as *mut T` | 类型转换 |
| `#define MAX 100` | `const MAX: usize = 100;` | 常量 |
| `typedef` | `type Alias = Original;` | 类型别名 |
| `printf("%d", x)` | `println!("{}", x)` | 格式化输出 |
