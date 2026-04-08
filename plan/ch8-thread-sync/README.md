# Ch8: 线程与同步

> 对应分支：`origin/ch8`
> 对应 xv6：`spinlock.c` + `sleeplock.c` + 线程
> 预计时间：2-3 天

---

## 8.1 本章目标

引入**线程**概念，将进程拆分为"资源容器"和"执行流"，并实现同步原语。

核心能力：
- **线程** — 同一进程内的多个执行流
- **互斥锁** (Mutex) — 自旋锁和阻塞锁两种实现
- **信号量** (Semaphore) — 计数同步
- **条件变量** (Condvar) — 等待特定条件

---

## 8.2 架构变化：进程 vs 线程

### ch5-ch7 模型
```
进程 (TaskControlBlock) = 资源 + 执行流（合一）
```

### ch8 新模型
```
进程 (ProcessControlBlock) = 资源容器
  ├── memory_set          地址空间
  ├── fd_table            文件描述符
  ├── mutex_list          互斥锁列表
  ├── semaphore_list      信号量列表
  ├── condvar_list        条件变量列表
  └── tasks: Vec<Option<Arc<TaskControlBlock>>>  线程列表

线程 (TaskControlBlock) = 执行流
  ├── process: Weak<PCB>  所属进程（弱引用）
  ├── kstack              内核栈
  ├── trap_cx_ppn         Trap 上下文
  ├── task_cx             任务上下文
  └── task_status         Ready / Running / Blocked
```

对比 xv6：xv6 没有真正的线程概念，每个 `struct proc` 既是进程也是执行单元。

---

## 8.3 TaskControlBlock — 线程控制块

```rust
// os/src/task/task.rs (ch8 版本)
pub struct TaskControlBlock {
    // 不可变
    pub process: Weak<ProcessControlBlock>,    // 所属进程
    pub kstack: KernelStack,                   // 内核栈
    // 可变
    inner: UPSafeCell<TaskControlBlockInner>,
}

pub struct TaskControlBlockInner {
    pub res: Option<TaskUserRes>,              // 用户态资源（用户栈、TrapContext 页）
    pub trap_cx_ppn: PhysPageNum,
    pub task_cx: TaskContext,
    pub task_status: TaskStatus,
    pub exit_code: Option<i32>,
}

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    Ready,
    Running,
    Blocked,    // ← 新增！用于同步原语
}
```

---

## 8.4 sys_thread_create — 创建线程

```rust
pub fn sys_thread_create(entry: usize, arg: usize) -> isize {
    let task = current_task().unwrap();
    let process = task.process.upgrade().unwrap();

    // 创建新线程
    let new_task = Arc::new(TaskControlBlock::new(
        Arc::clone(&process),
        ustack_base,
        true,    // 分配用户态资源
    ));

    // 加入调度器
    add_task(Arc::clone(&new_task));

    // 设置新线程的 Trap 上下文
    let new_task_trap_cx = new_task_inner.get_trap_cx();
    *new_task_trap_cx = TrapContext::app_init_context(
        entry,                        // 线程入口函数
        new_task_res.ustack_top(),    // 用户栈
        kernel_token(),
        new_task.kstack.get_top(),    // 内核栈
        trap_handler as usize,
    );
    (*new_task_trap_cx).x[10] = arg;  // 传递参数（通过 a0）

    // 加入进程的线程列表
    process_inner.tasks[new_task_tid] = Some(Arc::clone(&new_task));

    new_task_tid as isize
}
```

---

## 8.5 UPSafeCell — 基础同步原语

```rust
// os/src/sync/up.rs
pub struct UPSafeCell<T> {
    inner: RefCell<T>,
}

unsafe impl<T> Sync for UPSafeCell<T> {}

impl<T> UPSafeCell<T> {
    pub unsafe fn new(value: T) -> Self {
        Self { inner: RefCell::new(value) }
    }
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        self.inner.borrow_mut()
    }
}
```

这是 rCore 中最基础的同步容器：
- `RefCell`：运行时借用检查（如果有两个同时的 `borrow_mut`，会 panic）
- `unsafe impl Sync`：手动保证线程安全（在单核上成立）

---

## 8.6 Mutex — 互斥锁

### 两种实现

#### 1. MutexSpin — 自旋锁

```rust
pub struct MutexSpin {
    locked: UPSafeCell<bool>,
}

impl Mutex for MutexSpin {
    fn lock(&self) {
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();  // 让出 CPU，但不阻塞
                continue;
            } else {
                *locked = true;
                return;
            }
        }
    }

    fn unlock(&self) {
        let mut locked = self.locked.exclusive_access();
        *locked = false;
    }
}
```

**对比 xv6 的 `spinlock`：**
```c
void acquire(struct spinlock *lk) {
    while (__sync_lock_test_and_set(&lk->locked, 1) != 0)
        ;  // 忙等待
}
```

rCore 的"自旋"其实是 yield 式的（`suspend_current_and_run_next`），不是真正的忙等。

#### 2. MutexBlocking — 阻塞锁

```rust
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,  // 等待队列
}

impl Mutex for MutexBlocking {
    fn lock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            // 锁被占用 → 加入等待队列 + 阻塞
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();    // 状态改为 Blocked
        } else {
            mutex_inner.locked = true;
        }
    }

    fn unlock(&self) {
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            wakeup_task(waking_task);        // 唤醒等待的线程
        } else {
            mutex_inner.locked = false;
        }
    }
}
```

**对比 xv6 的 `sleeplock`：**
```c
void acquiresleep(struct sleeplock *lk) {
    acquire(&lk->lk);
    while (lk->locked) {
        sleep(lk, &lk->lk);     // 睡眠等待
    }
    lk->locked = 1;
    release(&lk->lk);
}
```

---

## 8.7 Semaphore — 信号量

```rust
pub struct Semaphore {
    pub inner: UPSafeCell<SemaphoreInner>,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    pub fn new(res_count: usize) -> Self {
        Self {
            inner: unsafe { UPSafeCell::new(SemaphoreInner {
                count: res_count as isize,
                wait_queue: VecDeque::new(),
            })},
        }
    }

    pub fn up(&self) {                           // V 操作
        let mut inner = self.inner.exclusive_access();
        inner.count += 1;
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                wakeup_task(task);
            }
        }
    }

    pub fn down(&self) {                         // P 操作
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
        }
    }
}
```

---

## 8.8 Condvar — 条件变量

```rust
pub struct Condvar {
    pub inner: UPSafeCell<CondvarInner>,
}

pub struct CondvarInner {
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Condvar {
    pub fn signal(&self) {
        let mut inner = self.inner.exclusive_access();
        if let Some(task) = inner.wait_queue.pop_front() {
            wakeup_task(task);
        }
    }

    pub fn wait(&self, mutex: Arc<dyn Mutex>) {
        mutex.unlock();                          // 先释放锁
        let mut inner = self.inner.exclusive_access();
        inner.wait_queue.push_back(current_task().unwrap());
        drop(inner);
        block_current_and_run_next();            // 阻塞
        mutex.lock();                            // 被唤醒后重新获取锁
    }
}
```

**经典的条件变量使用模式：**
```rust
mutex.lock();
while !condition {
    condvar.wait(mutex.clone());  // 原子地释放锁 + 阻塞
}
// condition 为 true，继续执行
mutex.unlock();
```

---

## 8.9 block_current_and_run_next — 阻塞机制

```rust
pub fn block_current_and_run_next() {
    let task = take_current_task().unwrap();
    let mut task_inner = task.inner_exclusive_access();
    task_inner.task_status = TaskStatus::Blocked;   // 标记为 Blocked
    let task_cx_ptr = &mut task_inner.task_cx as *mut TaskContext;
    drop(task_inner);
    // 注意：不把 task 放回就绪队列！
    // 它在某个等待队列中（mutex/semaphore/condvar 的 wait_queue）
    schedule(task_cx_ptr);
}

pub fn wakeup_task(task: Arc<TaskControlBlock>) {
    let mut task_inner = task.inner_exclusive_access();
    task_inner.task_status = TaskStatus::Ready;
    drop(task_inner);
    add_task(task);    // 重新加入就绪队列
}
```

对比 `suspend_current_and_run_next`：
| 操作 | suspend | block |
|------|---------|-------|
| 状态 | Ready | Blocked |
| 就绪队列 | 放回 | 不放回 |
| 唤醒 | 调度器自动选中 | 需要显式 `wakeup_task` |

---

## 8.10 系统调用总结

```rust
// os/src/syscall/sync.rs
pub fn sys_sleep(ms: usize) -> isize;                    // 睡眠
pub fn sys_mutex_create(blocking: bool) -> isize;         // 创建锁
pub fn sys_mutex_lock(mutex_id: usize) -> isize;          // 加锁
pub fn sys_mutex_unlock(mutex_id: usize) -> isize;        // 解锁
pub fn sys_semaphore_create(res_count: usize) -> isize;   // 创建信号量
pub fn sys_semaphore_up(sem_id: usize) -> isize;           // V 操作
pub fn sys_semaphore_down(sem_id: usize) -> isize;         // P 操作
pub fn sys_condvar_create() -> isize;                       // 创建条件变量
pub fn sys_condvar_signal(condvar_id: usize) -> isize;     // signal
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize; // wait

// os/src/syscall/thread.rs
pub fn sys_thread_create(entry: usize, arg: usize) -> isize;
pub fn sys_gettid() -> isize;
pub fn sys_waittid(tid: usize) -> i32;
```

---

## 8.11 Mutex trait — 多态的力量

```rust
pub trait Mutex: Sync + Send {
    fn lock(&self);
    fn unlock(&self);
}

// 创建时根据参数选择实现
pub fn sys_mutex_create(blocking: bool) -> isize {
    let mutex: Option<Arc<dyn Mutex>> = if !blocking {
        Some(Arc::new(MutexSpin::new()))
    } else {
        Some(Arc::new(MutexBlocking::new()))
    };
    // 存入进程的 mutex_list
}
```

`Arc<dyn Mutex>` — 同一个接口，不同实现：
- `MutexSpin`：让出式自旋
- `MutexBlocking`：阻塞式等待
- 用户通过 `sys_mutex_create(blocking)` 选择

---

## 8.12 对比总结

| 概念 | xv6 | rCore ch8 |
|------|-----|-----------|
| 线程 | 无独立概念 | `TaskControlBlock` |
| 进程 | `struct proc` | `ProcessControlBlock` |
| 自旋锁 | `struct spinlock` | `MutexSpin` |
| 睡眠锁 | `struct sleeplock` | `MutexBlocking` |
| 信号量 | 无 | `Semaphore` |
| 条件变量 | `sleep`/`wakeup` | `Condvar` |
| 锁接口 | `acquire`/`release` | `Mutex` trait |
| 等待机制 | `sleep(chan, lock)` | `block_current_and_run_next()` |

---

## 8.13 Rust 知识点总结

| 特性 | 本章用途 |
|------|---------|
| `trait Mutex: Send + Sync` | 锁的抽象接口 |
| `Arc<dyn Mutex>` | 运行时多态，统一自旋锁和阻塞锁 |
| `Weak<ProcessControlBlock>` | 线程引用进程（避免循环引用） |
| `VecDeque<Arc<TCB>>` | 等待队列 |
| `block_current_and_run_next` | 阻塞当前线程 |
| `wakeup_task` | 唤醒被阻塞的线程 |

---

## 8.14 思考题

1. `MutexSpin` 和 `MutexBlocking` 的性能差异是什么？什么场景用哪个？

2. 为什么 `condvar.wait()` 要先 `mutex.unlock()` 再阻塞？如果反过来呢？
   > 提示：如果先阻塞再解锁，其他线程无法获取锁来 signal

3. 一个进程有 3 个线程，如果主线程 exit 了，其他线程会怎样？

4. `Semaphore::down()` 中 `count` 为什么用 `isize`（有符号）而不是 `usize`（无符号）？
   > 提示：count < 0 表示有线程在等待

5. 如果两个线程互相 `mutex_lock` 对方持有的锁，会发生什么？rCore 能检测死锁吗？
