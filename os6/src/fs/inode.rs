use easy_fs::{
    EasyFileSystem,
    Inode,
};
use crate::drivers::BLOCK_DEVICE;
use crate::sync::UPSafeCell;
use alloc::sync::Arc;
use lazy_static::*;
use bitflags::*;
use alloc::vec::Vec;
use super::{File, StatMode, Stat};
use crate::mm::UserBuffer;

/// A wrapper around a filesystem inode
/// to implement File trait atop
pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}

/// The OS inode inner in 'UPSafeCell'
pub struct OSInodeInner {
    offset: usize,
    inode: Arc<Inode>,
}

impl OSInode {
    /// Construct an OS inode from a inode
    pub fn new(
        readable: bool,
        writable: bool,
        inode: Arc<Inode>,
    ) -> Self {
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner {
                offset: 0,
                inode,
            })},
        }
    }
    /// Read all data inside a inode into vector
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.exclusive_access();
        let mut buffer = [0u8; 512];
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
    // // 尝试过只实现这个元素的情况下，没有办法从`syscall::fs`中进行访问
    // // 似乎只有学习类似`read`或者`write`的操作，生成一个全新的trait类型，并在 `Stdin` and `Stdout`中实现，才能使之被正常访问。
    // #[allow(dead_code)]
    // fn fstat(&self) -> (u64, StatMode, u32) {
    //     let inner = self.inner.exclusive_access();     // OSInode
    //     let inode = &inner.inode;                               // Inode
    //     let (ino, dir_add, nlink) = ROOT_INODE.fstat(inode);
    //     let mode = match dir_add {
    //         // 注意：由于在这个地方理论上不应该传入2 因为disk_node 不存在这第三种状态，因此顺手用了一个新的东西来承接这个东西,但是逻辑上应该是添加一个新的类型
    //         0 => StatMode::DIR,
    //         1 => StatMode::FILE,
    //         _ => StatMode::NULL,
    //     };
    //     (ino, mode, nlink)
    // }
}

lazy_static! {
    /// The root of all inodes, or '/' in short
    pub static ref ROOT_INODE: Arc<Inode> = {
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        Arc::new(EasyFileSystem::root_inode(&efs))
    };
}

/// List all files in the filesystems
pub fn list_apps() {
    println!("/**** APPS ****");
    for app in ROOT_INODE.ls() {
        println!("{}", app);
    }
    println!("**************/");
}

bitflags! {
    /// Flags for opening files
    pub struct OpenFlags: u32 {
        const RDONLY = 0;
        const WRONLY = 1 << 0;
        const RDWR = 1 << 1;
        const CREATE = 1 << 9;
        const TRUNC = 1 << 10;
    }
}

impl OpenFlags {
    /// Get the current read write permission on an inode
    /// does not check validity for simplicity
    /// returns (readable, writable)
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

/// Open a file by path
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = ROOT_INODE.find(name) {
            // clear size
            inode.clear();
            Some(Arc::new(OSInode::new(
                readable,
                writable,
                inode,
            )))
        } else {
            // create file
            ROOT_INODE.create(name)
                .map(|inode| {
                    Arc::new(OSInode::new(
                        readable,
                        writable,
                        inode,
                    ))
                })
        }
    } else {
        ROOT_INODE.find(name)
            .map(|inode| {
                if flags.contains(OpenFlags::TRUNC) {
                    inode.clear();
                }
                Arc::new(OSInode::new(
                    readable,
                    writable,
                    inode
                ))
            })
    }
}

impl File for OSInode {
    // 尝试过只实现这个元素的情况下，没有办法从`syscall::fs`中进行访问
    // 似乎只有学习类似`read`或者`write`的操作，生成一个全新的trait类型，并在 `Stdin` and `Stdout`中实现，才能使之被正常访问。
    #[allow(dead_code)]
    fn fstat(&self) -> (u64, StatMode, u32) {

    // fn fstat(&self) -> u64 {
        // println!("A");
        let inner = self.inner.exclusive_access();     // OSInode
        // println!("B");
        let inode = &inner.inode;                               // Inode
        // println!("C");
        let (ino, dir_add, nlink) = ROOT_INODE.fstat(inode);
        debug!("return nlink = {} in OSInode", nlink);
        // let s = ROOT_INODE.fstat(inode); 
        // println!("D");
        let mode = match dir_add {
            // 注意：由于在这个地方理论上不应该传入2 因为disk_node 不存在这第三种状态，因此顺手用了一个新的东西来承接这个东西
            0 => StatMode::DIR,
            1 => StatMode::FILE,
            _ => StatMode::NULL,
        };
        debug!("ino:{}\tmode:{}\tnlink:{}", ino, dir_add, nlink);
        (ino, mode, nlink)
        // 123124
    }
    fn readable(&self) -> bool { self.readable }
    fn writable(&self) -> bool { self.writable }
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_read_size = 0usize;
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, *slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size;
            total_read_size += read_size;
        }
        total_read_size
    }
    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.exclusive_access();
        let mut total_write_size = 0usize;
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, *slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }
}

pub fn linkat(old_name: &str, new_name: &str) -> isize{
    ROOT_INODE.create_a_hard_link(old_name, new_name)
}

pub fn unlinkat(name: &str) -> isize {
    ROOT_INODE.delete_a_hard_link(name)
    // -1
}