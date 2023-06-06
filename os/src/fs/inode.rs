//! `Arc<Inode>` -> `OSInodeInner`: In order to open files concurrently
//! we need to wrap `Inode` into `Arc`,but `Mutex` in `Inode` prevents
//! file systems from being accessed simultaneously
//!
//! `UPSafeCell<OSInodeInner>` -> `OSInode`: for static `ROOT_INODE`,we
//! need to wrap `OSInodeInner` into `UPSafeCell`
use super::dir::DirEntry;
use super::stat::Kstat;

use super::File;
use crate::{drivers::BLOCK_DEVICE, console::print};
use crate::mm::UserBuffer;
use crate::sync::UPSafeCell;
use _core::str::FromStr;
use alloc::sync::Arc;
use alloc::vec::Vec;
use bitflags::*;
//ztr_file
use easy_fs::{FAT32Manager, VFile, ATTRIBUTE_ARCHIVE, ATTRIBUTE_DIRECTORY};
//use easy_fs::{EasyFileSystem, Inode};
use lazy_static::*;
use alloc::string::String;
/// A wrapper around a filesystem inode
/// to implement File trait atop
//ztr_file
pub enum DiskInodeType {
    File,
    Directory,
}

pub struct OSInode {
    readable: bool,
    writable: bool,
    inner: UPSafeCell<OSInodeInner>,
}
/// The OS inode inner in 'UPSafeCell'
pub struct OSInodeInner {
    offset: usize,
    //inode: Arc<Inode>,
    //ztr_file
    inode: Arc<VFile>,
}

impl OSInode {
    /// Construct an OS inode from a inode
    pub fn new(readable: bool, writable: bool, inode: Arc<VFile>) -> Self {
        Self {
            readable,
            writable,
            inner: unsafe { UPSafeCell::new(OSInodeInner { offset: 0, inode }) },
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
    pub fn is_dir(&self) -> bool {
        let inner = self.inner.exclusive_access();
        inner.inode.is_dir().clone()
    }
    //ztr_open
    pub fn get_name(&self) -> String {
        let inner = &self.inner.exclusive_access();
        let inode = &inner.inode;
        inode.name.clone()
    }
}
//ztr_file
lazy_static! {
    pub static ref ROOT_INODE: Arc<VFile> = {
        let fat32_manager = FAT32Manager::open(BLOCK_DEVICE.clone());
        let manager_reader = fat32_manager.read();
        Arc::new(manager_reader.get_root_vfile(&fat32_manager))
    };
}

// lazy_static! {
//     pub static ref ROOT_INODE: Arc<Inode> = {
//         let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
//         Arc::new(EasyFileSystem::root_inode(&efs))
//     };
// }
/// List all files in the filesystems
// pub fn list_apps() {
//     println!("/**** APPS ****");
//     for app in ROOT_INODE.ls() {
//         println!("{}", app);
//     }
//     println!("**************/");
// }

//ztr_file
pub fn list_apps() {
    println!("/**** APPS ****");
    for app in ROOT_INODE.ls_lite().unwrap() {
        if app.1 & ATTRIBUTE_DIRECTORY == 0 {
            println!("{}", app.0);
        }
    }
    println!("**************/")
}
bitflags! {
    ///Open file flags
    pub struct OpenFlags: u32 {
        ///Read only
        const RDONLY = 0;
        ///Write only
        const WRONLY = 1 << 0;
        ///Read & Write
        const RDWR = 1 << 1;
        ///Allow create
        const CREATE = 1 << 6;
        ///Clear file and return an empty one
        const TRUNC = 1 << 10;
        const O_DIRECTROY = 1 << 21;
        const LARGEFILE  = 0100000;
        const CLOEXEC = 02000000;
    }
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
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
///Open file with flags
// pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
//     let (readable, writable) = flags.read_write();
//     if flags.contains(OpenFlags::CREATE) {
//         if let Some(inode) = ROOT_INODE.find(name) {
//             // clear size
//             inode.clear();
//             Some(Arc::new(OSInode::new(readable, writable, inode)))
//         } else {
//             // create file
//             ROOT_INODE
//                 .create(name)
//                 .map(|inode| Arc::new(OSInode::new(readable, writable, inode)))
//         }
//     } else {
//         ROOT_INODE.find(name).map(|inode| {
//             if flags.contains(OpenFlags::TRUNC) {
//                 inode.clear();
//             }
//             Arc::new(OSInode::new(readable, writable, inode))
//         })
//     }
// }

//ztr_file
pub fn open_file(
    work_path: &str,
    path: &str,
    flags: OpenFlags,
    dtype: DiskInodeType,
) -> Option<Arc<OSInode>> {
    // 找到当前路径的inode(file, directory)
    let cur_inode = {
        if work_path == "/" {
            ROOT_INODE.clone()
        } else {
            let wpath: Vec<&str> = work_path.split('/').collect();
            ROOT_INODE.find_vfile_bypath(wpath).unwrap()
        }
    };
    let mut pathv: Vec<&str> = path.split('/').collect();
    let (readable, writeable) = flags.read_write();
    if flags.contains(OpenFlags::CREATE) {
        if let Some(inode) = cur_inode.find_vfile_bypath(pathv.clone()) {
            inode.remove();
        }
        {
            // create file
            let name = pathv.pop().unwrap();
            if let Some(temp_inode) = cur_inode.find_vfile_bypath(pathv.clone()) {
                let attribute = {
                    match dtype {
                        DiskInodeType::Directory => ATTRIBUTE_DIRECTORY,
                        DiskInodeType::File => ATTRIBUTE_ARCHIVE,
                    }
                };
                temp_inode
                    .create(name, attribute)
                    .map(|inode| Arc::new(OSInode::new(readable, writeable, inode)))
            } else {
                None
            }
        }
    } 
    else if flags.contains(OpenFlags::O_DIRECTROY) {
        let name = pathv.pop().unwrap();
        if let Some(temp_inode) = cur_inode.find_vfile_bypath(pathv.clone()) {
            temp_inode
                .create(name, ATTRIBUTE_DIRECTORY)
                .map(|inode| Arc::new(OSInode::new(readable, writeable, inode)))
        } else {
            None
        }
    }
    else {
        cur_inode.find_vfile_bypath(pathv).map(|inode| {
            if flags.contains(OpenFlags::TRUNC) {
                inode.clear();
            }
            Arc::new(OSInode::new(readable, writeable, inode))
        })
    }
}

impl File for OSInode {
    fn readable(&self) -> bool {
        self.readable
    }
    fn writable(&self) -> bool {
        self.writable
    }
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
    fn get_fstat(&self, kstat: &mut Kstat) {
        let inner = self.inner.exclusive_access();
        let vfile = inner.inode.clone();
        // todo
        let (st_size, st_blksize, st_blocks) = vfile.stat();
        drop(inner);
        kstat.init(st_size, st_blksize, st_blocks);
    }

    fn get_name(&self) -> String {
        self.get_name()
    }

    fn set_offset(&self, offset: usize){
        let mut inner = self.inner.exclusive_access();
        inner.offset = offset;
        drop(inner);
    }
    fn get_dirent(&self, dirent: &mut DirEntry) -> isize {
        if !self.is_dir() {
            return -1;
        }
        let mut inner = self.inner.exclusive_access();
        let offset = inner.offset as u32;
        if let Some((name, off, _, _)) = inner.inode.dirent_info(offset as usize) {
            dirent.set_name(name.as_str());
            inner.offset = off as usize;
            let len = (name.len() + 8 * 4) as isize;
            drop(inner);
            len
        } else {
            -1
        }
    }
}
//ztr_chdir
pub fn chdir(work_path: &str, path: &str) -> Option<String> {
    let current_inode = {
        if path.chars().nth(0).unwrap() == '/' {
            // 传入路径是绝对路径
            ROOT_INODE.clone()
        } else {
            // 传入路径是相对路径
            let current_work_pathv: Vec<&str> = work_path.split('/').collect();
            ROOT_INODE.find_vfile_bypath(current_work_pathv).unwrap()
        }
    };
    let pathv: Vec<&str> = path.split('/').collect();
    if let Some(_) = current_inode.find_vfile_bypath(pathv) {
        let new_current_path = String::from_str("/").unwrap() + &String::from_str(path).unwrap();
        if current_inode.get_name() == "/" {
            Some(new_current_path)
        } else {
            Some(String::from_str(current_inode.get_name()).unwrap() + &new_current_path)
        }
    } else {
        None
    }
}
//ztr_test
pub fn add_initproc_shell() {
    extern "C" {
        fn _num_app();
    }
    let num_app_ptr = _num_app as usize as *mut usize;
    let app_start = unsafe { core::slice::from_raw_parts_mut(num_app_ptr.add(1), 3) };

    if let Some(inode) = open_file("/", "initproc", OpenFlags::CREATE, DiskInodeType::File) {
        println!("Create initproc ");
        let mut data: Vec<&'static mut [u8]> = Vec::new();
        data.push(unsafe {
            core::slice::from_raw_parts_mut(app_start[0] as *mut u8, app_start[1] - app_start[0])
        });
        println!("Start write initproc ");
        inode.write(UserBuffer::new(data));
        println!("initproc OK");
    } else {
        panic!("initproc create fail!");
    }

    if let Some(inode) = open_file("/", "user_shell", OpenFlags::CREATE, DiskInodeType::File) {
        println!("Create user_shell ");
        let mut data: Vec<&'static mut [u8]> = Vec::new();
        data.push(unsafe {
            core::slice::from_raw_parts_mut(app_start[1] as *mut u8, app_start[2] - app_start[1])
        });
        println!("Start write user_shell ");
        inode.write(UserBuffer::new(data));
        println!("User_shell OK");
    } else {
        panic!("user_shell create fail!");
    }

    println!("Write apps(initproc & user_shell) to disk from mem");
}

