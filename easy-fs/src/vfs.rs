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

    pub fn fstat(&self, inode: &Arc<Inode>) -> (u64, u8, u32) {
        debug!("inside fstat");
        let inode_id = inode.get_inode_id();
        debug!("a");
        let mode = inode.get_inode_mode();
        // debug!("b");
        let nlink = inode.get_disk_hard_linked();
        // debug!("c");
        (inode_id, mode, nlink as u32)
    }
    // pub fn fstat(&self, inode: &Arc<Inode>) -> u64 {
    //     let inode_id = inode.get_inode_id();
    //     inode_id
    // }

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


    // NOTE: 按照原版额的想法是尝试复制一个inode的实现，生成一个硬链接，但是后面发现似乎可以直接添加一个DirEntry?而不重新alloc inode
    // 但是可能出现bug，bug的原因主要在删除上面【但是好像没有牵涉到了这个部分？】
    // 注意：基本所有内容都与之相同。
    pub fn create_a_hard_link(&self, old_name: &str, new_name: &str) {
        // debug!("Happy");
        let mut fs = self.fs.lock();            // gain the mutex lock of 
        // check if old_name has been existed in root_inode
        // if self.modify_disk_inode(|root_inode| {
        //     assert!(root_inode.is_dir());                                   // assert it is a directory
        //     self.find_inode_id(old_name, root_inode)   
        // }).is_none() {
        //     return 
        // }

        // check if old_name is equal to new_name [finish outside]
        // just like create a new file

        // // create a new file
        // let new_inode_id = fs.alloc_inode();        // alloc a inode
        // // initalize inode
        // let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id); // get information from inode
        // get_block_cache(
        //     new_inode_block_id as usize, 
        //     Arc::clone(&self.block_device)
        // ).lock().modify(new_inode_block_offset, |new_inode: & mut DiskInode| {
        //     new_inode.initialize(DiskInodeType::File);
        // });

        // i thought i could finish it just by copy the infomation on old_name
        let old_inode_block =self.find(old_name).unwrap();
        let old_inode_block_id = old_inode_block.block_id;
        // old_inode_block.block_offset
        drop(old_inode_block);

        // let old_inode_block_id = self.find_inode_id(old_name, root_inode).unwrap();
        // let (old_inode_block_id) = ( old_inode_block.block_id, old_inode_block.block_offset);

        // insert this inode into root_inode
        self.modify_disk_inode(|root_inode| {
            // append file in dirent [add file size]
            let file_account = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_account + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(new_name, old_inode_block_id as u32);
            root_inode.write_at(
                file_account * DIRENT_SZ,
                dirent.as_bytes(), 
                &self.block_device,
            );
        });
        self.add_disk_hard_linked();

        // let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        // return inode
        // Some(Arc::new(Self::new(
        //     block_id,
        //     block_offset,
        //     self.fs.clone(),
        //     self.block_device.clone(),
        // )))
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
