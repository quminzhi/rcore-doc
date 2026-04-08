# Ch6: 文件系统

> 对应分支：`origin/ch6`
> 对应 xv6：`fs.c` + `bio.c` + inode 层
> 预计时间：3-4 天 ⭐ 重点章节

---

## 6.1 本章目标

实现一个简单的文件系统 `easy-fs`，支持文件的创建、读写和应用加载。

核心能力：
- **块设备抽象** — `BlockDevice` trait
- **块缓存层** — 缓存磁盘块，减少 I/O
- **磁盘布局** — SuperBlock + Bitmap + Inode + Data
- **VFS 层** — 通过 inode 操作文件
- **内核集成** — 文件描述符表，`open`/`read`/`write`/`close`

---

## 6.2 项目结构

```
easy-fs/                 # 独立的文件系统 crate（可在宿主机测试！）
├── src/
│   ├── lib.rs
│   ├── block_dev.rs     # BlockDevice trait 定义
│   ├── block_cache.rs   # 块缓存管理器
│   ├── layout.rs        # 磁盘数据结构（SuperBlock, Inode, DirEntry）
│   ├── bitmap.rs        # 位图管理（inode/data block 分配）
│   ├── efs.rs           # EasyFileSystem 顶层接口
│   └── vfs.rs           # Inode 虚拟文件系统接口

easy-fs-fuse/            # 宿主机工具：将用户程序打包成 fs 镜像

os/src/fs/
├── mod.rs               # File trait 定义
├── inode.rs             # OSInode — 内核中的文件抽象
├── pipe.rs              # 管道（ch7 引入，ch6 可能已有）
└── stdio.rs             # 标准输入输出
```

---

## 6.3 BlockDevice trait — 块设备抽象

```rust
// easy-fs/src/block_dev.rs
pub trait BlockDevice: Send + Sync + Any {
    fn read_block(&self, block_id: usize, buf: &mut [u8]);
    fn write_block(&self, block_id: usize, buf: &[u8]);
}
```

**对比 xv6：**
```c
// xv6 kernel/bio.c
struct buf* bread(uint dev, uint blockno);  // 读块
void bwrite(struct buf *b);                  // 写块
void brelse(struct buf *b);                  // 释放
```

xv6 直接操作 `struct buf`，rCore 通过 trait 抽象：
- QEMU 上：`VirtIOBlock` 实现 `BlockDevice`
- 测试时：`File` 也可以实现 `BlockDevice`

---

## 6.4 磁盘布局

```
┌──────────┬──────────┬──────────┬──────────┬──────────┐
│  Super   │  Inode   │  Inode   │  Data    │  Data    │
│  Block   │  Bitmap  │  Area    │  Bitmap  │  Area    │
│  (1 块)  │          │          │          │          │
└──────────┴──────────┴──────────┴──────────┴──────────┘
```

### SuperBlock
```rust
#[repr(C)]
pub struct SuperBlock {
    magic: u32,              // 魔数，标识文件系统类型
    pub total_blocks: u32,
    pub inode_bitmap_blocks: u32,
    pub inode_area_blocks: u32,
    pub data_bitmap_blocks: u32,
    pub data_area_blocks: u32,
}
```

### DiskInode — 磁盘上的 inode
```rust
#[repr(C)]
pub struct DiskInode {
    pub size: u32,
    pub direct: [u32; INODE_DIRECT_COUNT],     // 直接块（28个）
    pub indirect1: u32,                         // 一级间接块
    pub indirect2: u32,                         // 二级间接块
    type_: DiskInodeType,                       // File 或 Directory
}
```

**对比 xv6 的 `struct dinode`：**
```c
struct dinode {
    short type;
    short major;
    short minor;
    short nlink;
    uint size;
    uint addrs[NDIRECT+1];  // 12 个直接块 + 1 个间接块
};
```

rCore 的 easy-fs 有两级间接块，容量更大。

### 目录项
```rust
#[repr(C)]
pub struct DirEntry {
    name: [u8; NAME_LENGTH_LIMIT + 1],   // 文件名（27+1 字节）
    inode_number: u32,                     // inode 编号
}
```

---

## 6.5 块缓存 — BlockCache

```rust
// easy-fs/src/block_cache.rs
pub struct BlockCache {
    cache: [u8; BLOCK_SZ],                // 块数据缓存
    block_id: usize,
    block_device: Arc<dyn BlockDevice>,
    modified: bool,                        // 脏标记
}

impl BlockCache {
    // 获取缓存中某个偏移处的引用
    pub fn get_ref<T>(&self, offset: usize) -> &T {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        let addr = &self.cache[offset] as *const _ as usize;
        unsafe { &*(addr as *const T) }
    }

    // 获取可变引用，并自动标记为脏
    pub fn get_mut<T>(&mut self, offset: usize) -> &mut T {
        let type_size = core::mem::size_of::<T>();
        assert!(offset + type_size <= BLOCK_SZ);
        self.modified = true;               // 自动标脏！
        let addr = &self.cache[offset] as *const _ as usize;
        unsafe { &mut *(addr as *mut T) }
    }
}

impl Drop for BlockCache {
    fn drop(&mut self) {
        if self.modified {
            self.sync();                     // 脏块写回磁盘
        }
    }
}
```

**RAII 再次发挥作用：**
- xv6: 手动调用 `bwrite()` 和 `brelse()`
- rCore: `BlockCache` drop 时自动写回脏块

### 全局块缓存管理器
```rust
pub struct BlockCacheManager {
    queue: VecDeque<(usize, Arc<Mutex<BlockCache>>)>,  // (block_id, cache)
}
// 最多缓存 16 个块，LRU 淘汰
```

---

## 6.6 Inode (vfs.rs) — 文件系统操作接口

```rust
pub struct Inode {
    block_id: usize,          // inode 所在的块号
    block_offset: usize,      // 块内偏移
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// 列出目录下所有文件
    pub fn ls(&self) -> Vec<String> { /* ... */ }

    /// 在目录中查找文件
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> { /* ... */ }

    /// 创建文件
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> { /* ... */ }

    /// 读取文件数据
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize { /* ... */ }

    /// 写入文件数据
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize { /* ... */ }
}
```

---

## 6.7 File trait — 内核文件抽象

```rust
// os/src/fs/mod.rs
pub trait File: Send + Sync {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(&self, buf: UserBuffer) -> usize;
    fn write(&self, buf: UserBuffer) -> usize;
}
```

所有可以读写的东西都实现 `File` trait：
- `OSInode` — 磁盘文件
- `Stdin` / `Stdout` — 标准输入输出
- `Pipe` — 管道（ch7）

### OSInode — 内核文件描述符

```rust
// os/src/fs/inode.rs
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}

pub struct OSInodeInner {
    offset: usize,                    // 读写偏移
    inode: Arc<Inode>,                // 底层 easy-fs inode
}
```

### 文件描述符表

```rust
// 在 TaskControlBlockInner 中
pub fd_table: Vec<Option<Arc<dyn File>>>,
//                   ↑       ↑
//            下标=fd   trait object（可以是任何实现了 File 的类型）
```

**`Arc<dyn File>` 是什么？**
- `dyn File`：动态分发的 trait object（类似 C 的函数指针表/vtable）
- `Arc<dyn File>`：引用计数的 trait object
- fd_table[0] 可能是 Stdin，fd_table[3] 可能是 Pipe，统一为 `dyn File`

对比 xv6：
```c
// xv6 kernel/file.h
struct file {
    enum { FD_NONE, FD_PIPE, FD_INODE } type;  // 手动类型标签
    struct pipe *pipe;
    struct inode *ip;
    // ...
};
```

Rust 的 trait object 自动处理了 xv6 中手动的类型分发！

---

## 6.8 系统调用

### open
```rust
pub fn sys_open(path: *const u8, flags: u32) -> isize {
    let path = translated_str(current_user_token(), path);
    let flags = OpenFlags::from_bits(flags).unwrap();
    if let Some(inode) = open_file(path.as_str(), flags) {
        let task = current_task().unwrap();
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();     // 找到空闲的 fd
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}
```

### read / write
```rust
pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();       // Arc 引用计数 +1
        drop(inner);                    // 释放锁！
        file.read(UserBuffer::new(/* ... */)) as isize
    } else {
        -1
    }
}
```

---

## 6.9 对比总结

| 概念 | xv6 | rCore |
|------|-----|-------|
| 块设备 | `bread`/`bwrite`/`brelse` | `BlockDevice` trait |
| 块缓存 | `struct buf` + LRU | `BlockCache` + RAII 写回 |
| inode | `struct inode` + `ilock`/`iunlock` | `Inode` + `Mutex` |
| 文件 | `struct file` + type 标签 | `dyn File` trait object |
| fd 表 | `struct file *ofile[NOFILE]` | `Vec<Option<Arc<dyn File>>>` |
| 目录 | `struct dirent` | `DirEntry` |

---

## 6.10 Rust 知识点总结

| 特性 | 本章用途 |
|------|---------|
| `trait` | `BlockDevice`, `File` — 抽象接口 |
| `dyn Trait` | 动态分发，fd 表统一不同文件类型 |
| `Arc<dyn File>` | 引用计数 + 动态分发 |
| `Arc<Mutex<T>>` | 多个地方共享且可变的数据 |
| RAII (`Drop`) | BlockCache 自动写回脏块 |
| 泛型方法 `get_ref::<T>` | 块缓存中读取任意类型 |
| `core::mem::size_of::<T>()` | 获取类型大小 |

---

## 6.11 思考题

1. 为什么 `easy-fs` 是一个独立的 crate？这有什么好处？
   > 提示：可以在宿主机上单独测试，不需要 QEMU

2. `BlockCache::get_mut` 为什么要自动标记 `modified = true`？

3. `Arc<dyn File>` 和 C 中 `struct file` + 函数指针有什么区别？

4. 为什么 `sys_read` 中要 `drop(inner)` 再调用 `file.read()`？
   > 提示：如果不 drop，读文件时持有 TCB 的锁会导致什么问题？

5. easy-fs 没有实现 `delete` 操作，如果要添加需要做什么？
