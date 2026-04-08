# rCore-Tutorial-v3 学习计划

> 为有 C/xv6 经验、无 Rust 基础的学习者量身定制
> 预计总时间：4-5 周（每天 3-4 小时）

---

## 学习路线图

```
Week 1          Week 2          Week 3          Week 4          Week 5
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│ Ch0 Rust │   │ Ch3 多道 │   │ Ch5 进程 │   │ Ch7 IPC  │   │ Ch9 设备 │
│ 基础语法 │──→│ 程序与   │──→│ 管理     │──→│ 管道信号 │──→│ 驱动网络 │
│          │   │ 分时调度 │   │ fork/exec│   │          │   │          │
│ Ch1 裸机 │   │          │   │ Ch6 文件 │   │ Ch8 线程 │   │          │
│ 启动     │   │          │   │ 系统     │   │ 与同步   │   │          │
│          │   │          │   │          │   │          │   │          │
│ Ch2 批处 │   │          │   │          │   │          │   │          │
│ 理系统   │   │          │   │          │   │          │   │          │
└──────────┘   └──────────┘   └──────────┘   └──────────┘   └──────────┘
```

---

## 章节目录

| 章节 | 目录 | 主题 | xv6 对应 | 难度 |
|------|------|------|----------|------|
| [Ch0](ch0-rust-basics/README.md) | `ch0-rust-basics/` | Rust 最小必备知识 | — | ★★☆ |
| [Ch1](ch1-bare-metal/README.md) | `ch1-bare-metal/` | 裸机启动 Hello World | `entry.S` + `start()` | ★☆☆ |
| [Ch2](ch2-batch/README.md) | `ch2-batch/` | 批处理系统与特权级切换 | `trampoline.S` | ★★☆ |
| [Ch3](ch3-multiprog/README.md) | `ch3-multiprog/` | 多道程序与分时调度 | `scheduler()` + `swtch.S` | ★★☆ |
| [Ch4](ch4-address-space/README.md) | `ch4-address-space/` | 虚拟内存与页表 | `vm.c` + `kalloc.c` | ★★★ |
| [Ch5](ch5-process/README.md) | `ch5-process/` | 进程管理 fork/exec/wait | `proc.c` | ★★★ |
| [Ch6](ch6-filesystem/README.md) | `ch6-filesystem/` | 文件系统 easy-fs | `fs.c` + `bio.c` | ★★★ |
| [Ch7](ch7-ipc/README.md) | `ch7-ipc/` | 进程间通信（管道/信号） | pipe | ★★☆ |
| [Ch8](ch8-thread-sync/README.md) | `ch8-thread-sync/` | 线程与同步原语 | `spinlock.c` | ★★☆ |
| [Ch9](ch9-device-net/README.md) | `ch9-device-net/` | 设备驱动与网络 | `uart.c` + `virtio_disk.c` | ★★☆ |

---

## 每章学习三步法

```
1. 📖 阅读本 plan 中对应章节的 README.md
   └─ 了解本章核心概念 + Rust 语法 + 与 xv6 的对比

2. 🔍 切到对应分支，阅读源码
   └─ git checkout origin/chN
   └─ 重点看 README 中标注的"关键代码"

3. 🔧 动手实验
   └─ 编译运行：cd os && make run
   └─ 尝试修改代码观察效果
   └─ 做 ch*-lab 分支的练习题
```

---

## 核心 Rust → C 对照速查

| Rust | C (xv6) | 说明 |
|------|---------|------|
| `#![no_std]` | 不链接 libc | 裸机编程 |
| 所有权 + `Drop` | `malloc`/`free` | 自动内存管理 |
| `UPSafeCell<T>` | `spinlock` + 全局变量 | 内部可变性 |
| `Arc<T>` | 手动引用计数 | 共享所有权 |
| `Weak<T>` | 裸指针 | 弱引用，打破循环 |
| `trait File` | `struct file` + 函数指针 | 多态接口 |
| `dyn Trait` | vtable 函数指针表 | 动态分发 |
| `match enum` | `switch` | 穷举匹配 |
| `Option<T>` | `NULL` | 空值安全 |
| `lazy_static!` | 全局初始化 | 运行时初始化静态变量 |
| `bitflags!` | `#define` 位掩码 | 类型安全的标志位 |

---

## 外部资源

- [rCore 官方中文教程](https://rcore-os.github.io/rCore-Tutorial-Book-v3/)
- [Rust 语言圣经（中文）](https://course.rs/)
- [Rust By Example（中文）](https://rustwiki.org/zh-CN/rust-by-example/)
- [RISC-V 手册](https://github.com/riscv/riscv-isa-manual)
