# Ch7: 进程间通信

> 对应分支：`origin/ch7`
> 对应 xv6：pipe + 信号
> 预计时间：2 天

---

## 7.1 本章目标

实现进程间通信机制：
- **管道 (Pipe)** — 进程间字节流通信
- **信号 (Signal)** — 异步通知机制
- **I/O 重定向** — 让 shell 支持 `|` 和 `>`
- **命令行参数** — exec 传递 argc/argv

---

## 7.2 新增文件

```
os/src/
├── fs/pipe.rs           # 管道实现 ← 核心
├── task/signal.rs       # 信号定义
├── task/action.rs       # 信号处理动作
└── (syscall 新增 sys_pipe, sys_dup, sys_sigaction 等)
```

---

## 7.3 管道 (Pipe) — 核心数据结构

```rust
// os/src/fs/pipe.rs

pub struct Pipe {
    readable: bool,
    writable: bool,
    buffer: Arc<UPSafeCell<PipeRingBuffer>>,
}

pub struct PipeRingBuffer {
    arr: [u8; RING_BUFFER_SIZE],       // 环形缓冲区（32 字节）
    head: usize,                        // 读指针
    tail: usize,                        // 写指针
    status: RingBufferStatus,           // Full / Empty / Normal
    write_end: Option<Weak<Pipe>>,      // 弱引用写端（检测是否关闭）
}

#[derive(Copy, Clone, PartialEq)]
enum RingBufferStatus {
    Full,
    Empty,
    Normal,
}
```

### 管道创建
```rust
pub fn make_pipe() -> (Arc<Pipe>, Arc<Pipe>) {
    let buffer = Arc::new(unsafe { UPSafeCell::new(PipeRingBuffer::new()) });
    let read_end = Arc::new(Pipe::read_end_with_buffer(buffer.clone()));
    let write_end = Arc::new(Pipe::write_end_with_buffer(buffer.clone()));
    buffer.exclusive_access().set_write_end(&write_end);
    (read_end, write_end)
}
```

**共享关系图：**
```
  read_end (Arc<Pipe>) ──→ buffer (Arc<PipeRingBuffer>) ←── write_end (Arc<Pipe>)
       readable=true                  ↑                        writable=true
                              write_end: Weak<Pipe> ──→ write_end
```

- `buffer` 被读端和写端共享（通过 `Arc`）
- `write_end` 字段用 `Weak` 引用写端（检测写端是否已关闭）

---

## 7.4 管道读写 — 实现 File trait

### 读操作
```rust
impl File for Pipe {
    fn read(&self, buf: UserBuffer) -> usize {
        assert!(self.readable());
        let mut buf_iter = buf.into_iter();
        let mut already_read = 0usize;
        loop {
            let mut ring_buffer = self.buffer.exclusive_access();
            let loop_read = ring_buffer.available_read();
            if loop_read == 0 {
                if ring_buffer.all_write_ends_closed() {
                    return already_read;     // 写端关闭，返回已读数据
                }
                drop(ring_buffer);           // 释放锁！
                suspend_current_and_run_next();  // 让出 CPU 等待数据
                continue;
            }
            for _ in 0..loop_read {
                if let Some(byte_ref) = buf_iter.next() {
                    unsafe { *byte_ref = ring_buffer.read_byte(); }
                    already_read += 1;
                    if already_read == want_to_read {
                        return want_to_read;
                    }
                } else {
                    return already_read;
                }
            }
        }
    }
}
```

**对比 xv6 的 `piperead`：**
```c
int piperead(struct pipe *pi, uint64 addr, int n) {
    acquire(&pi->lock);
    while (pi->nread == pi->nwrite && pi->writeopen) {
        sleep(&pi->nread, &pi->lock);  // 等待数据
    }
    // 读取数据...
    release(&pi->lock);
}
```

核心逻辑完全一样：
1. 缓冲区空 → 检查写端是否关闭
2. 写端已关闭 → 返回
3. 写端未关闭 → yield 等待数据
4. 有数据 → 逐字节读取

### 写端关闭检测
```rust
pub fn all_write_ends_closed(&self) -> bool {
    self.write_end.as_ref().unwrap().upgrade().is_none()
    //   Weak<Pipe>               升级为 Arc<Pipe>
    //   如果写端的 Arc 引用计数为 0（已 drop），upgrade 返回 None
}
```

**`Weak` 的精妙之处：**
- 当所有写端的 `Arc<Pipe>` 被 drop 后（比如关闭 fd）
- `Weak::upgrade()` 返回 `None`
- 读端就知道"写端已关闭，不会有更多数据了"

---

## 7.5 sys_pipe 系统调用

```rust
pub fn sys_pipe(pipe: *mut usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    let (pipe_read, pipe_write) = make_pipe();
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(pipe_read);
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(pipe_write);
    // 将 fd 写回用户空间
    *translated_refmut(token, pipe) = read_fd;
    *translated_refmut(token, unsafe { pipe.add(1) }) = write_fd;
    0
}
```

---

## 7.6 sys_dup — 文件描述符复制

```rust
pub fn sys_dup(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() || inner.fd_table[fd].is_none() {
        return -1;
    }
    let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = Some(Arc::clone(inner.fd_table[fd].as_ref().unwrap()));
    //                              Arc::clone → 引用计数 +1，指向同一个 File
    new_fd as isize
}
```

**I/O 重定向的原理：**
```
# shell: app1 | app2

fork app1:
  close(stdout)           // 关闭 fd 1
  dup(pipe_write_fd)      // pipe 写端变成 fd 1
  exec("app1")            // app1 的 stdout 写入管道

fork app2:
  close(stdin)            // 关闭 fd 0
  dup(pipe_read_fd)       // pipe 读端变成 fd 0
  exec("app2")            // app2 的 stdin 从管道读取
```

---

## 7.7 信号 (Signal) — 简述

```rust
// os/src/task/signal.rs
bitflags! {
    pub struct SignalFlags: u32 {
        const SIGDEF    = 1;
        const SIGINT    = 1 << 2;      // Ctrl+C
        const SIGILL    = 1 << 4;      // 非法指令
        const SIGABRT   = 1 << 6;      // abort
        const SIGFPE    = 1 << 8;      // 浮点异常
        const SIGKILL   = 1 << 9;      // 杀死进程
        const SIGSEGV   = 1 << 11;     // 段错误
        const SIGSTOP   = 1 << 17;     // 暂停
        const SIGCONT   = 1 << 18;     // 继续
    }
}
```

信号处理机制：
1. 发送信号：`sys_kill(pid, signal)`
2. 在 Trap 返回用户态前检查待处理信号
3. 执行用户注册的信号处理函数（或默认行为）

---

## 7.8 命令行参数

```rust
// exec 现在支持传递参数
pub fn sys_exec(path: *const u8, args: *const usize) -> isize {
    // 读取参数字符串数组
    let mut args_vec: Vec<String> = Vec::new();
    loop {
        let arg_str_ptr = *translated_ref(token, args.add(i));
        if arg_str_ptr == 0 { break; }
        args_vec.push(translated_str(token, arg_str_ptr as *const u8));
    }
    // 将参数压入用户栈
    // 用户程序通过 a0=argc, a1=argv 获取
}
```

---

## 7.9 ch6 → ch7 变更统计

| 类别 | 新增/修改 |
|------|----------|
| 管道 | `pipe.rs` 全新（173 行） |
| 信号 | `signal.rs`（61 行）+ `action.rs`（31 行） |
| 系统调用 | `sys_pipe`, `sys_dup`, `sys_close`, `sys_kill`, `sys_sigaction` |
| 用户程序 | `pipetest`, `pipe_large_test`, `sig_tests`, `user_shell` 增强 |
| 总计 | +1736 行 / -430 行 |

---

## 7.10 Rust 知识点总结

| 特性 | 本章用途 |
|------|---------|
| `Weak<T>` + `upgrade()` | 检测管道写端是否关闭 |
| `Arc::clone()` | dup 时共享同一个 File |
| `into_iter()` | 遍历 UserBuffer |
| `loop` + `continue` | 管道读写的等待循环 |
| `drop()` | 读写管道前释放环形缓冲区的锁 |
| `bitflags!` | 信号标志位定义 |

---

## 7.11 思考题

1. 管道的 `PipeRingBuffer` 只有 32 字节，如果要写入 1KB 数据会怎样？
   > 提示：写满后 yield，等读端消费后继续写

2. 为什么 `Weak::upgrade()` 能检测写端是否关闭？
   > 提示：当所有 `Arc<Pipe>` (写端) 被 drop 后，`Weak::upgrade()` 返回 `None`

3. `fork` 后子进程继承了父进程的 fd_table，管道两端的引用计数会怎样变化？

4. 如果不 `drop(ring_buffer)` 就调用 `suspend_current_and_run_next()`，会发生什么？
