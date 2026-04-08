# Ch5: 进程管理

> 对应分支：`origin/ch5`
> 对应 xv6：`proc.c` + `fork()` / `exec()` / `waitpid()`
> 预计时间：2-3 天 ⭐ 重点章节

---

## 5.1 本章目标

实现**进程**抽象，支持 `fork`, `exec`, `waitpid`, `getpid` 系统调用。

从 ch3 的固定任务数组 → ch5 的动态进程创建和回收。

---

## 5.2 架构变化

ch3 的单一 `TaskManager` 被拆分为三个组件：

```
┌──────────────────────────────────┐
│  TaskManager (管理就绪队列)       │  → 存放所有 Ready 的任务
│  VecDeque<Arc<TaskControlBlock>> │
├──────────────────────────────────┤
│  Processor (管理当前 CPU)         │  → 维护当前运行的任务
│  current: Option<Arc<TCB>>       │
├──────────────────────────────────┤
│  PidAllocator (进程 ID 分配)     │  → 分配/回收 PID
│  RecycleAllocator                │
└──────────────────────────────────┘
```

---

## 5.3 TaskControlBlock — 进程控制块

```rust
// os/src/task/task.rs (ch5 版本)
pub struct TaskControlBlock {
    // 不可变部分
    pub pid: PidHandle,                        // 进程 PID（RAII 管理）
    pub kernel_stack: KernelStack,             // 内核栈（RAII 管理）
    // 可变部分
    inner: UPSafeCell<TaskControlBlockInner>,
}

pub struct TaskControlBlockInner {
    pub trap_cx_ppn: PhysPageNum,              // Trap 上下文所在物理页
    pub base_size: usize,                      // 应用数据大小
    pub task_cx: TaskContext,                   // 任务上下文
    pub task_status: TaskStatus,
    pub memory_set: MemorySet,                 // 地址空间 ← 新增！
    pub parent: Option<Weak<TaskControlBlock>>, // 父进程 ← 新增！
    pub children: Vec<Arc<TaskControlBlock>>,   // 子进程列表 ← 新增！
    pub exit_code: i32,                        // 退出码 ← 新增！
}
```

### 关键 Rust 概念：`Arc` 和 `Weak`

```rust
// Arc = Atomic Reference Counting（原子引用计数）
pub children: Vec<Arc<TaskControlBlock>>,  // 父进程持有子进程的强引用
pub parent: Option<Weak<TaskControlBlock>>, // 子进程持有父进程的弱引用
```

**为什么父进程用 `Weak`？**
```
父进程 ──Arc──→ 子进程
子进程 ──Arc──→ 父进程   ← 循环引用！永远不会被释放！

父进程 ──Arc──→ 子进程
子进程 ──Weak─→ 父进程   ← 弱引用不增加引用计数，打破循环
```

对比 xv6：
```c
// xv6 不用引用计数，通过 wait() 手动回收
struct proc *parent;  // 就是一个裸指针
```

---

## 5.4 PidHandle — RAII 管理 PID

```rust
pub struct PidHandle(pub usize);

pub fn pid_alloc() -> PidHandle {
    PidHandle(PID_ALLOCATOR.exclusive_access().alloc())
}

impl Drop for PidHandle {
    fn drop(&mut self) {
        PID_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}
```

PID 和物理帧一样，用 RAII 管理：
- 创建 → 自动分配
- 销毁 → 自动回收
- **永远不会泄漏 PID！**

---

## 5.5 fork — 复制进程

```rust
// os/src/syscall/process.rs
pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // 修改子进程的 Trap 上下文，让 fork 返回 0
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    trap_cx.x[10] = 0;           // a0 = 0（子进程的返回值）
    // 将子进程加入就绪队列
    add_task(new_task);
    new_pid as isize              // 父进程返回子进程 PID
}
```

**fork 的实现（简化）：**
```rust
impl TaskControlBlock {
    pub fn fork(self: &Arc<Self>) -> Arc<TaskControlBlock> {
        // 1. 复制地址空间
        let memory_set = MemorySet::from_existed_user(
            &parent.inner.memory_set
        );
        // 2. 分配新的 PID 和内核栈
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        // 3. 创建新 TCB
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe { UPSafeCell::new(TaskControlBlockInner {
                // ... 复制父进程的状态
                parent: Some(Arc::downgrade(self)),  // 弱引用指向父进程
                children: Vec::new(),
                // ...
            })},
        });
        // 4. 添加到父进程的子进程列表
        parent.inner.children.push(Arc::clone(&task_control_block));
        task_control_block
    }
}
```

对比 xv6 的 `fork()`：
```c
// xv6 kernel/proc.c
int fork(void) {
    np = allocproc();
    uvmcopy(p->pagetable, np->pagetable, p->sz);
    np->parent = p;
    // ...
}
```

---

## 5.6 exec — 加载新程序

```rust
pub fn sys_exec(path: *const u8) -> isize {
    // 从用户空间读取路径字符串
    let path = translated_str(current_user_token(), path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);    // 替换当前进程的地址空间
        0
    } else {
        -1
    }
}
```

**exec 的实现：**
```rust
pub fn exec(&self, elf_data: &[u8]) {
    // 从 ELF 文件创建新的地址空间
    let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
    // 替换旧的地址空间
    let mut inner = self.inner_exclusive_access();
    inner.memory_set = memory_set;
    // 设置新的 Trap 上下文
    let trap_cx = inner.get_trap_cx();
    *trap_cx = TrapContext::app_init_context(entry_point, user_sp, ...);
}
```

---

## 5.7 waitpid — 等待子进程

```rust
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();

    // 找到指定的子进程（或任意子进程）
    // pid == -1: 等待任意子进程
    // pid > 0: 等待指定子进程

    // 检查是否有子进程已经退出（状态 = Zombie）
    if let Some((idx, _)) = inner.children.iter().enumerate()
        .find(|(_, p)| {
            p.inner_exclusive_access().is_zombie()
            && (pid == -1 || p.getpid() == pid as usize)
        })
    {
        let child = inner.children.remove(idx);
        assert_eq!(Arc::strong_count(&child), 1);  // 确保是最后一个引用
        let found_pid = child.getpid();
        let exit_code = child.inner_exclusive_access().exit_code;
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2  // 子进程还没退出，需要再次调用
    }
}
```

**`Arc::strong_count(&child) == 1` 的含义：**
- 只有父进程持有这个子进程的引用
- `remove` 后 Arc 的引用计数变为 0
- 子进程的所有资源（内存、PID、内核栈）自动释放！

---

## 5.8 Processor — CPU 管理

```rust
// os/src/task/processor.rs
pub struct Processor {
    current: Option<Arc<TaskControlBlock>>,   // 当前运行的任务
    idle_task_cx: TaskContext,                 // idle 任务上下文
}
```

调度循环：
```
idle 循环 ←──── __switch ──── 任务执行
    │                            │
    ├─ 从 TaskManager 取任务     │
    ├─ __switch 到任务           │
    │                            │
    │         任务结束/让出       │
    │                            │
    ←──── __switch 回到 idle ────┘
```

```rust
pub fn run_tasks() {
    loop {
        if let Some(task) = fetch_task() {      // 从就绪队列取任务
            let idle_task_cx_ptr = /* ... */;
            let next_task_cx_ptr = /* ... */;
            task.inner.task_status = TaskStatus::Running;
            processor.current = Some(task);
            unsafe {
                __switch(idle_task_cx_ptr, next_task_cx_ptr);
            }
            // 任务让出后回到这里，继续循环
        }
    }
}
```

---

## 5.9 进程生命周期

```
pid_alloc() ──→ new() ──→ Ready ──→ Running ──→ Zombie ──→ 被 waitpid 回收
                           ↑          │                         │
                           │    yield/中断                   Arc::drop
                           └──────────┘                         │
                                                         自动释放所有资源
```

对比 xv6：
```
allocproc() → USED → RUNNABLE → RUNNING → ZOMBIE → freeproc()
```

---

## 5.10 initproc — 第一个用户进程

```rust
// 内核启动时创建 initproc
lazy_static! {
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new(
        TaskControlBlock::new(get_app_data_by_name("initproc").unwrap())
    );
}

pub fn add_initproc() {
    add_task(INITPROC.clone());
}
```

initproc 的用户态代码：
```rust
// user/src/bin/initproc.rs
fn main() -> i32 {
    if fork() == 0 {
        exec("user_shell\0");   // 子进程运行 shell
    } else {
        loop {
            let mut exit_code: i32 = 0;
            let pid = wait(&mut exit_code);  // 父进程回收僵尸进程
            if pid == -1 { yield_(); continue; }
            println!("[initproc] Released a zombie process, pid={}, exit_code={}", pid, exit_code);
        }
    }
    0
}
```

---

## 5.11 Rust 知识点总结

| 特性 | 本章用途 |
|------|---------|
| `Arc<T>` | 多个地方引用同一个 TCB（就绪队列 + 父子关系） |
| `Weak<T>` | 子进程引用父进程（避免循环引用） |
| `Arc::downgrade()` | 创建弱引用 |
| `Weak::upgrade()` | 尝试获取强引用（可能失败，返回 Option） |
| `Arc::strong_count()` | 查看引用计数 |
| `Vec::remove()` | 按索引移除元素 |
| `iter().enumerate().find()` | 查找符合条件的子进程 |
| RAII | PidHandle drop → 回收 PID；KernelStack drop → 回收栈空间 |

---

## 5.12 思考题

1. 为什么 `fork` 中子进程的 `parent` 用 `Weak` 而不是 `Arc`？
2. `waitpid` 中 `assert_eq!(Arc::strong_count(&child), 1)` 是在验证什么？
3. 如果一个进程的父进程先退出了，子进程会变成孤儿进程，rCore 怎么处理？
   > 提示：看 `exit_current_and_run_next()` 中如何把子进程挂到 initproc 下
4. `exec` 后，旧的地址空间何时被释放？（提示：`inner.memory_set = memory_set` 会触发什么？）
