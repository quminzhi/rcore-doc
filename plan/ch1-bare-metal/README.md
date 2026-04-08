# Ch1: 裸机启动与环境搭建

> 对应分支：`origin/ch1`
> 对应 xv6：`entry.S` + `start()` + `printf()`
> 预计时间：1-2 天

---

## 1.1 本章目标

在 QEMU 模拟的 RISC-V 64 机器上，用 Rust 打印出 `Hello, world!` 然后关机。

听起来简单？但这意味着你需要：
- 脱离操作系统（`#![no_std]`）
- 脱离标准入口（`#![no_main]`）
- 自己设置栈指针
- 自己实现 `print!` 宏
- 自己实现 panic handler

---

## 1.2 源文件结构

```
os/src/
├── entry.asm          # 汇编入口，设置栈指针
├── main.rs            # Rust 主函数
├── sbi.rs             # SBI 调用封装
├── console.rs         # print!/println! 宏实现
├── lang_items.rs      # panic handler
├── logging.rs         # 日志系统
└── linker-qemu.ld     # 链接脚本
```

---

## 1.3 启动流程

```
QEMU 加载 SBI (rustsbi)
       ↓
SBI 初始化硬件，跳转到 0x80200000
       ↓
entry.asm: _start
  → 设置栈指针 sp = boot_stack_top
  → call rust_main
       ↓
rust_main()
  → clear_bss()
  → println!("Hello, world!")
  → shutdown()
```

### 对比 xv6
| xv6 | rCore |
|-----|-------|
| `entry.S` → `start()` → `main()` | `entry.asm` → `rust_main()` |
| M-mode 初始化由自己做 | M-mode 初始化由 SBI 做 |
| `printf()` 直接写 UART | `println!()` 通过 SBI ecall |

---

## 1.4 关键代码解读

### entry.asm — 汇编入口

```asm
    .section .text.entry
    .globl _start
_start:
    la sp, boot_stack_top    # 设置栈指针
    call rust_main           # 跳转到 Rust 代码

    .section .bss.stack
    .globl boot_stack_lower_bound
boot_stack_lower_bound:
    .space 4096 * 16         # 预留 64KB 栈空间
    .globl boot_stack_top
boot_stack_top:
```

**对比 xv6 的 entry.S：**
```asm
# xv6 entry.S
_entry:
    la sp, stack0
    li a0, 1024*4
    csrr a1, mhartid
    addi a1, a1, 1
    mul a0, a0, a1
    add sp, sp, a0
    call start
```

区别：rCore 不需要处理多核（单核启动），SBI 已经从 M-mode 切到了 S-mode。

---

### main.rs — Rust 入口

```rust
#![no_std]                    // 不使用标准库
#![no_main]                   // 不使用标准 main 入口

use core::arch::global_asm;

#[macro_use]
mod console;
mod lang_items;
mod sbi;

global_asm!(include_str!("entry.asm"));  // 包含汇编文件

/// 清零 BSS 段
pub fn clear_bss() {
    unsafe extern "C" {
        safe fn sbss();       // 链接脚本中定义的 BSS 起始符号
        safe fn ebss();       // BSS 结束符号
    }
    (sbss as usize..ebss as usize)
        .for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
}

#[unsafe(no_mangle)]          // 不改变函数名，让汇编能调用
pub fn rust_main() -> ! {     // -> ! 表示永不返回
    clear_bss();
    println!("[kernel] Hello, world!");
    sbi::shutdown(false)
}
```

### Rust 语法讲解

| 语法 | 含义 | C 对应 |
|------|------|--------|
| `#![no_std]` | 不链接标准库 | 编译时不链接 libc |
| `#![no_main]` | 没有标准 main | 自定义入口 |
| `#[unsafe(no_mangle)]` | 保持函数名不变 | 默认就不会 mangle |
| `-> !` | 返回类型为 "永不返回" | `__attribute__((noreturn))` |
| `global_asm!()` | 内联全局汇编 | `__asm__()` |
| `unsafe extern "C"` | 声明外部 C 函数 | `extern` |

---

### sbi.rs — SBI 调用

```rust
pub fn console_putchar(c: usize) {
    #[allow(deprecated)]
    sbi_rt::legacy::console_putchar(c);    // ecall 到 M-mode 输出字符
}

pub fn shutdown(failure: bool) -> ! {
    use sbi_rt::{NoReason, Shutdown, SystemFailure, system_reset};
    if !failure {
        system_reset(Shutdown, NoReason);
    } else {
        system_reset(Shutdown, SystemFailure);
    }
    unreachable!()
}
```

**对比 xv6：**
- xv6 直接操作 UART 寄存器（`uartputc`）
- rCore 通过 SBI（Supervisor Binary Interface）调用，SBI 运行在 M-mode，类似 BIOS

---

### console.rs — 实现 print! 宏

```rust
use crate::sbi::console_putchar;
use core::fmt::{self, Write};

struct Stdout;

impl Write for Stdout {                  // 实现 core::fmt::Write trait
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            console_putchar(c as usize);  // 逐字符通过 SBI 输出
        }
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?));
    }
}
```

**这段代码做了什么：**
1. 定义空结构体 `Stdout`
2. 为它实现 `core::fmt::Write` trait（只需实现 `write_str` 方法）
3. `format_args!` 是 Rust 内置宏，将格式字符串编译为 `Arguments` 类型
4. `write_fmt` 调用 `write_str` 逐字符输出

---

### lang_items.rs — Panic 处理

```rust
use core::panic::PanicInfo;

#[panic_handler]                          // 必须提供！no_std 环境必备
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        error!("[kernel] Panicked at {}:{} {}",
            location.file(), location.line(), info.message());
    } else {
        error!("[kernel] Panicked: {}", info.message());
    }
    shutdown(true)
}
```

在 `no_std` 环境中，Rust 需要你告诉它：panic 了怎么办？
- 标准程序：打印栈回溯，调用 `abort()`
- OS 内核：打印错误信息，关机

---

## 1.5 链接脚本简述

链接脚本 (`linker-qemu.ld`) 控制程序在内存中的布局：

```
0x80200000  ← 内核入口地址（SBI 跳转到这里）
  .text     ← 代码段
  .rodata   ← 只读数据
  .data     ← 可读写数据
  .bss      ← 未初始化数据（需要清零）
  stack     ← 内核栈
```

---

## 1.6 编译与运行

```bash
# 切换到 ch1 分支
git checkout origin/ch1

# 编译
cd os && make build

# 运行
make run
# 期望输出: [kernel] Hello, world!
```

---

## 1.7 Rust 知识点总结

本章涉及的 Rust 特性：

| 特性 | 用途 |
|------|------|
| `#![no_std]` / `#![no_main]` | 裸机编程基础 |
| `global_asm!()` | 嵌入汇编 |
| `macro_rules!` | 实现 print!/println! |
| `impl Write for Stdout` | trait 实现 |
| `#[panic_handler]` | 必须提供的 lang item |
| `unsafe` | 操作裸指针、调用外部函数 |
| `-> !` (never type) | 永不返回的函数 |

---

## 1.8 思考题

1. 为什么需要 `clear_bss()`？如果不清零会怎样？
   > 提示：BSS 段存放未初始化的全局变量，C 规范要求它们为 0

2. `entry.asm` 中栈为什么从高地址向低地址增长？
   > 提示：RISC-V 的 ABI 约定

3. 为什么 rCore 用 SBI 输出字符，而不是直接操作 UART？
   > 提示：分层设计，S-mode 不直接操作硬件
