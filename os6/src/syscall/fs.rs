//! File and filesystem-related syscalls

use crate::mm::PageTable;
use crate::mm::PhysAddr;
use crate::mm::VirtAddr;
use crate::mm::translated_byte_buffer;
use crate::mm::translated_str;
use crate::mm::translated_refmut;
use crate::task::current_user_token;
use crate::task::current_task;
use crate::fs::{open_file, linkat, unlinkat};
use crate::fs::OpenFlags;
use crate::fs::Stat;
use crate::mm::UserBuffer;
use alloc::sync::Arc;
use riscv::addr::Page;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(
            UserBuffer::new(translated_byte_buffer(token, buf, len))
        ) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.read(
            UserBuffer::new(translated_byte_buffer(token, buf, len))
        ) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(
        path.as_str(),
        OpenFlags::from_bits(flags).unwrap()
    ) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

// YOUR JOB: 扩展 easy-fs 和内核以实现以下三个 syscall
// NOTE: 这个应该就是一个类似于时间读取的部分，按照我们获取时间的方案重新获取一次就可以了？
// 通过指针获取对应内存位置，然后通过unsafe包裹向raw pointer(还是啥别的)里面写入对应的数据内容
// 主要存在的疑惑的位置在于我看到stat中包含的 bitflags! 这是不是意味着对于DIR and FILE 而言都存在这个调用
// 因此，是不是就不能使用类似ROOT_INODE来作为方法的承接对象了？应该采用某种方法，能够同时被FILE and DIR作用的方法
// fd_table 中所承接的对象的属性必须要求是满足File or send or sync的其中一个的对象
// 或者直接从三层抽象中实现？
    // 对于 OsInode 实现特殊的方法，允许其调用从 ROOT_INODE 中实现的函数 【 在其他层次中不被允许使用 】
// 
#[allow(dead_code, unused_variables)]
pub fn sys_fstat(_fd: usize, _st: *mut Stat) -> isize {
    
    /* +====================+ GET ADDR +====================+ */
    // BUG REPORT: BC we have using page_table in PageTable::from_token this function will using current_task()
    // when we putting it inside under the create of inner, then will return error because of we borrow it in two position
    // and BC it's extral posistion of this two things, it will case a DEADLOCK which will only case timeout to return
    let virtaddr = VirtAddr::from(_st as usize);
    debug!("VA:{}", usize::from(virtaddr));
    let physaddr = PageTable::from_token(current_user_token())
    .translate_va(virtaddr)
                                                    .unwrap();
    debug!("PA: {}", usize::from(physaddr));
    let pointer = usize::from(physaddr) as *mut Stat;
    
    // FIX 需要判断是否可能出现写不完的情况？
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();

    // bounds checking
    if _fd >= inner.fd_table.len() {
        println!("[WARN]: the len request is out of limit");
        return -1;
    }   
    // DELETE 题目没有明说是否是代表着没有这个必要...
    if inner.fd_table[_fd].is_none() {
        println!("[WARN]: fd_table wasn't existed !!!");
        return -1;
    }
    if let Some(inode) = &inner.fd_table[_fd] {
        println!("get fd_table");
        let (ino, mode, nlink) = inode.fstat();
        drop(inner);
        // println!("REPORT:{}", inode.fstat());
        println!("finish get fstat");

        // debug!("get PA:{:?} successed {}", physaddr, usize::from(physaddr));
        let pointer = usize::from(physaddr) as *mut Stat;
        debug!("get pointer successed");
        // println!("finish translation");
        unsafe {
            (*pointer).dev = 0;
            (*pointer).ino = ino;
            (*pointer).mode = mode;
            (*pointer).nlink = nlink;
        }
    } else {
        println!("[WARN]: couldn't get fd_table");
        return -1
    }
    println!("success: fstat syscall");
    0
}



pub fn sys_linkat(_old_name: *const u8, _new_name: *const u8) -> isize {
    let token = current_user_token();
    let old_path = translated_str(token, _old_name);
    let new_path = translated_str(token, _new_name);

    // base on the given possibliy result
    if old_path == new_path {
        return -1
    }
    // create_a_soft_link(new_path.as_str(), old_path.as_str())
    linkat(old_path.as_str(), new_path.as_str())
    
}

pub fn sys_unlinkat(_name: *const u8) -> isize {
    let token = current_user_token();
    let file_name = translated_str(token, _name);

    unlinkat(file_name.as_str())
}
