mod dir;
mod inode;
mod pipe;
mod stdio;
mod stat;
mod mount;

use crate::mm::UserBuffer;
use alloc::sync::Arc;
use alloc::string::String;
pub use stat::Kstat;
pub use mount::MNT_TABLE;

#[derive(Clone)]
pub struct FileDescriptor {
    pub cloexec: bool,
    pub ftype: FileType,
}

impl FileDescriptor {
    pub fn new(flag: bool, ftype: FileType) -> Self {
        Self {
            cloexec: flag,
            ftype: ftype,
        }
    }

    pub fn set_cloexec(&mut self, flag: bool) {
        self.cloexec = flag;
    }

    pub fn get_cloexec(&self) -> bool {
        self.cloexec
    }
}

/// 文件类型
#[derive(Clone)]
pub enum FileType {
    File(Arc<OSInode>),
    Abstr(Arc<dyn File + Send + Sync>),
}

pub trait File: Send + Sync {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(&self, buf: UserBuffer) -> usize;
    fn write(&self, buf: UserBuffer) -> usize;
    fn get_fstat(&self, kstat: &mut Kstat);

    fn get_dirent(&self, dirent: &mut DirEntry) -> isize;

    fn get_name(&self) -> String;

    fn set_offset(&self, offset: usize);

}

pub use dir::{DirEntry, DT_DIR, DT_REG, DT_UNKNOWN};
pub use inode::{list_apps, open_file, DiskInodeType, OSInode, OpenFlags, add_initproc_shell,chdir};
pub use pipe::{make_pipe, Pipe};
pub use stdio::{Stdin, Stdout};
