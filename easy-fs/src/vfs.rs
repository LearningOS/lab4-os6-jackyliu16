use core::mem::size_of;

use crate::BLOCK_SZ;

use super::{
    BlockDevice,
    DiskInode,
    DiskInodeType,
    DirEntry,
    EasyFileSystem,
    DIRENT_SZ,
    get_block_cache,
    block_cache_sync_all,
};
use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use log::debug;
use spin::{Mutex, MutexGuard};

/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}
// the question is how could we delete the A and all sort linked it has create?
// for example we need to impl the drop function, which contain all inode( the soft linked of main )
// or trying to using a struct to contain all Inode as a iinode ( which contain Inode and it's sort linked )
// or maybe add a things inside disk_node which is a caculator of soft linked
impl Inode {
    fn get_inode_id(&self) -> u64 {
        let fs = self.fs.lock();
        fs.get_inode_id(self.block_id as u32, self.block_offset)
    }
    // 这个地方没有办法将泛型传入进来，因此只是简单的使用了一个u8来代替对应的内容，准备在转出之后在具体变成泛型的内容
    // 这个地方由于disk_node理论上不应该传出None的结果，因此只是做了一个简单的占位
    fn get_inode_mode(&self) -> u8 {
        self.read_disk_inode(|disk_node| {
            if disk_node.is_dir() {
                0
            } else if disk_node.is_file() {
                1
            } else {
                2
            }
        })
    }

    // pub fn find_inode(&self,name:&str)->Option<Arc<Inode>>{
    //     let fs = self.fs.lock();//尝试获得文件系统的互斥锁
    //     self.read_disk_inode(|disk_inode|{
    //         self.find_inode_id(name,disk_inode).map(|inode_id|{
    //             let(block_id,block_offset) = fs.get_disk_inode_pos(inode_id);
    //             // println!("the inode id: {}, The block_id :{}, block_offset :{}",inode_id,block_id,block_offset);
    //             Arc::new(
    //                 Inode::new(
    //                     block_id,
    //                     block_offset,
    //                     self.fs.clone(),
    //                     self.block_device.clone(),
    //                 )
    //             )
    //         })
    //     })
    // }

    pub fn fstat(&self, inode: &Arc<Inode>) -> (u64, u8, u32) {
        debug!("inside fstat");
        let inode_id = inode.get_inode_id();
        debug!("a");
        let mode = inode.get_inode_mode();
        // debug!("b");
        let nlink = inode.get_disk_hard_linked();
        debug!("in inode.fstat: nlink = {}", nlink);
        (inode_id, mode, nlink as u32)
    }

    /* +==========+ JUST FOR HARD_LINKED +==========+ */
    #[allow(dead_code)]
    pub fn get_disk_hard_linked(&self) -> usize {
        self.read_disk_inode(|disk_node| {
            disk_node.hard_linked
        })
    }
    pub fn add_disk_hard_linked(&self)  {
        self.modify_disk_inode(|disk_node| {
            disk_node.hard_linked += 1;
        })
    }
    pub fn sub_disk_hard_linked(&self) {
        // FIX 是否需要检查可能存在的删除最后一个硬链接点的问题
        self.modify_disk_inode(|disk_node| {
            disk_node.hard_linked -= 1;
        })
    }

    // BUG report: we mutable using fs and it's it immuable存在死锁可能，需要注意顺序
    // NOTE: 按照原版额的想法是尝试复制一个inode的实现，生成一个硬链接，但是后面发现似乎可以直接添加一个DirEntry?而不重新alloc inode
    // 但是可能出现bug，bug的原因主要在删除上面【但是好像没有牵涉到了这个部分？】
    // 注意：基本所有内容都与之相同。
    // BUG REPORT: The passed parameter is given in reverse
    pub fn create_a_hard_link(&self, old_name: &str, new_name: &str) -> isize {
        debug!("===== CREATE A HARD LINK =====");
        
        // first check if old_name's file existed
        if self.modify_disk_inode(|root_inode| {
            assert!(root_inode.is_dir());
            self.find_inode_id(old_name, root_inode)
            // let old_inode_id = self.find_inode_id(old_name, root_inode);
            // old_inode_id.clone()        // i was trying to using life cycle to fix it but it seem not work
        }).is_none() {
            debug!("old name not existed");
            return -1
        }
        debug!("old name existed");

        let old_inode = self.find(old_name).unwrap();
        let (inode_block_id,inode_block_offset) = (old_inode.block_id,old_inode.block_offset);

        self.modify_disk_inode(|root_inode| {
            // BC we need to using fs in this function so, we have to get value before we acquire the fs lock
            let old_inode_id = self.find_inode_id(old_name, root_inode).unwrap();
            debug!("get old inode id successed");
            // acquire mutex lock
            let mut fs = self.fs.lock();
            debug!("get fs lock");
            // increasing the size of the function
            let file_account = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_account + 1) * DIRENT_SZ;
            self.increase_size(new_size as u32, root_inode, &mut fs);
            debug!("finish increase size");
            let dirent = DirEntry::new(new_name, old_inode_id);
            root_inode.write_at(
                file_account * DIRENT_SZ, 
                dirent.as_bytes(), 
                &self.block_device
            );
            debug!("finish write information into disk_inode");
        });
        let new_inode = Inode::new(
            inode_block_id as u32,
            inode_block_offset,
            self.fs.clone(),
            self.block_device.clone()
        );
        new_inode.add_disk_hard_linked();

        // BUG REPORT: 之前在没有对于当前inode进行操作的时候，有尝试过通过inode中保存block_id
        // 按照self.read_at的方法进行修改，但是似乎也没有办法将修改保存到disk_inode中去，
        // 原因在于如果我在当前的inode中进行操作本函数由ROOT_INODE调用，直接调用
        // self.add_disk_hard_linked实际上不会对于该inode产生任何影响
        // 因此，我们就创造了一个与我们所生成的inode完全相同的clone物品，对于他使用了对应的操作，
        // 进而使结果可以被用于inode，而非ROOT_INODE
        0
    }

    #[allow(unused_variables)]
    pub fn delete_a_hard_link(&self, file_name: &str) -> isize {

        // first check if file existed
        debug!("===== DELETE A HARD LINK =====");
        if self.modify_disk_inode(|root_inode|{
            assert!(root_inode.is_dir());
            self.find_inode_id(file_name, root_inode)
        }).is_none() {
            debug!("file not existed");
            return -1
        }

        // get nlink of a inode
        let delete_inode = self.find(file_name).unwrap();
        // let mut delete_inode 
        debug!("get delete inode successed");
        let nlink = delete_inode.get_disk_hard_linked();
        debug!("get nlink as :{} ", nlink);

        delete_inode.sub_disk_hard_linked();

        debug!("get nlink as 2:{}", nlink);


        // FIX 我真的不知道发生了什么，但是神奇的就是这个地方只要为 1 就会成功了
        if nlink == 1 {
            // into delete file process
            debug!("into delete file process");

            self.modify_disk_inode(|root_inode| {
                    let file_count = (root_inode.size as usize) / DIRENT_SZ;
                    
                    for i in 0..file_count {
                        let mut dirent = DirEntry::empty();
                        assert_eq!(
                            root_inode.read_at(
                                i * DIRENT_SZ,
                                dirent.as_bytes_mut(),
                                &self.block_device
                            ),
                            DIRENT_SZ,
                        );
                        if dirent.name() == file_name {

                            if dirent.inode_number() == delete_inode.block_id as u32{
                                debug!("===");
                            }

                            root_inode.write_at(
                                i * DIRENT_SZ, 
                                DirEntry::empty().as_bytes(),
                                &self.block_device
                            );
                        }
                    }
                }
            );
            debug!("seem finish delete file");
            delete_inode.clear();
            debug!("clear delete_inode");
        } else {
            debug!("into delete hard linked process");
            
            // FIX 有两种方法删除对应的inode，一种是简单的将原先的inode的值抹去，另外一种是删除并将其后面的元素向前挪动
            // 但是只剩下两天时间，可能没有那么多时间来让我完成后一种方式的调试，因此此处只是简单的的采用一个将原先的值抹去的操作
            self.modify_disk_inode(|root_inode| {
                // let mut fs = self.fs.lock();
                // debug!("acquire fs lock");
                let file_count = (root_inode.size as usize) / DIRENT_SZ;
                
                for i in 0..file_count {
                    let mut dirent = DirEntry::empty();
                    assert_eq!(
                        root_inode.read_at(
                            i * DIRENT_SZ,
                            dirent.as_bytes_mut(),
                            &self.block_device
                        ),
                        DIRENT_SZ,
                    );
                    if dirent.inode_number() == delete_inode.block_id as u32{
                        
                        if dirent.name() == file_name {
                            debug!("+++");
                        }
                        debug!("start rewrite processed");
                        root_inode.write_at(
                            DIRENT_SZ * i,
                            DirEntry::empty().as_bytes(), 
                            &self.block_device
                        );
                        debug!("rewrite inode space successed");
                    }
                }
            })
        }
        0
    }

    /// Create a vfs inode
    pub fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            // num_of_soft_linked: 0,
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(
            self.block_id,
            Arc::clone(&self.block_device)
        ).lock().read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(
            self.block_id,
            Arc::clone(&self.block_device)
        ).lock().modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(
        &self,
        name: &str,
        disk_inode: &DiskInode,
    ) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(
                    DIRENT_SZ * i,
                    dirent.as_bytes_mut(),
                    &self.block_device,
                ),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                return Some(dirent.inode_number() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        debug!("inside find function");
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode)
            .map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        let mut fs = self.fs.lock();
        if self.modify_disk_inode(|root_inode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        }).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) 
            = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(
            new_inode_block_id as usize,
            Arc::clone(&self.block_device)
        ).lock().modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
            new_inode.initialize(DiskInodeType::File);
        });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(
                        i * DIRENT_SZ,
                        dirent.as_bytes_mut(),
                        &self.block_device,
                    ),
                    DIRENT_SZ,
                );
                v.push(String::from(dirent.name()));
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            disk_inode.read_at(offset, buf, &self.block_device)
        })
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}
