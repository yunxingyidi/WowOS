//! File and filesystem-related syscalls
use core::mem::size_of;
use crate::console::print;
use crate::fs::{open_file, OpenFlags, DiskInodeType, FileDescriptor, FileType, File, OSInode, MNT_TABLE, chdir, DirEntry, Kstat, make_pipe};
use crate::mm::{translated_byte_buffer, translated_str, translated_refmut, UserBuffer};
use crate::task::{current_task, current_user_token};
use alloc::sync::Arc;

const AT_FDCWD: isize = -100;
pub const FD_LIMIT: usize = 128;


pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file: Arc<dyn File + Send + Sync> = match &file.ftype {
            FileType::Abstr(f) => f.clone(),
            FileType::File(f) => f.clone(),
            _ => return -1,
        };
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
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
        let file: Arc<dyn File + Send + Sync> = match &file.ftype {
            FileType::Abstr(f) => f.clone(),
            FileType::File(f) => f.clone(),
            _ => return -1,
        };
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}
//ztr_open
pub fn sys_openat(fd: isize, path: *const u8, flags: u32, mode: u32) -> isize {
    let task = current_task().unwrap();
    let token = current_user_token();
    let mut inner = task.inner_exclusive_access();
    
    let path = translated_str(token, path);
    let open_flags = OpenFlags::from_bits(flags).unwrap();
    if fd == AT_FDCWD {
        // 如果是当前工作目录
        
        // if open_flags.contains(OpenFlags::O_DIRECTROY) {
        //     if let Some(inode) = open_file(
        //         inner.get_work_path().as_str(), 
        //         path.as_str(), 
        //         open_flags, 
        //         DiskInodeType::File,
        //     ) {
        //         let fd = inner.alloc_fd();
        //         inner.fd_table[fd] = Some(FileDescriptor::new(
        //             open_flags.contains(OpenFlags::CLOEXEC),
        //             FileType::File(inode),
        //         ));
        //         drop(inner);
        //         fd as isize
        //     } else {
        //         -1
        //     }
        // }
        if let Some(inode) = open_file(
            inner.get_work_path().as_str(), 
            path.as_str(), 
            open_flags, 
            DiskInodeType::File,
        ) {
            let fd = inner.alloc_fd();
            inner.fd_table[fd] = Some(FileDescriptor::new(
                open_flags.contains(OpenFlags::CLOEXEC),
                FileType::File(inode),
            ));
            drop(inner);
            fd as isize
        } else {
            -1
        }
    } else {
        let dirfd = fd as usize;
        if dirfd >= inner.fd_table.len() {
            return -1;
        }
        if let Some(filedescriptor) = &inner.fd_table[dirfd]{
            let file:Arc<OSInode> = match &filedescriptor.ftype {
                FileType::File(file) => file.clone(),
                _ => return -1,
            };
            if let Some(f) = open_file(
                file.get_name().as_str(), 
                path.as_str(), 
                open_flags, 
                DiskInodeType::Directory,
            ) {
                let fd = inner.alloc_fd();
                inner.fd_table[fd] =Some(FileDescriptor::new(
                    open_flags.contains(OpenFlags::CLOEXEC),
                    FileType::File(f),
                ));
                drop(inner);
                fd as isize
            } else {
                -1
            }
        } else {
            -1
        }
    }
}
//ztr_dup
pub fn sys_dup(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    // 检查传入 fd 的合法性
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }

    let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = inner.fd_table[fd].clone();
    new_fd as isize
}
pub fn sys_dup3( old_fd: usize, new_fd: usize )->isize{
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();

    if  old_fd >= inner.fd_table.len() || new_fd > FD_LIMIT {
        return -1;
    }
    if inner.fd_table[old_fd].is_none() {
        return -1;
    }
    if new_fd >= inner.fd_table.len() {
        for _ in inner.fd_table.len()..(new_fd + 1) {
            inner.fd_table.push(None);
        }
    }

    //let mut act_fd = new_fd;
    //if inner.fd_table[new_fd].is_some() {
    //    act_fd = inner.alloc_fd();
    //}
    //let new_fd = inner.alloc_fd();
    inner.fd_table[new_fd] = inner.fd_table[old_fd].clone();
    new_fd as isize
}
//ztr_mkdir
pub fn sys_mkdirat(dirfd: isize, path: *const u8, mode: u32) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    let path = translated_str(token, path);
    _ = mode;

    if dirfd == AT_FDCWD {
        if let Some(_) = open_file(inner.get_work_path().as_str(), path.as_str(), OpenFlags::CREATE, DiskInodeType::Directory) {
            0
        } else {
            -1
        }
    } else {
        let dirfd = dirfd as usize;
        if dirfd >= inner.fd_table.len() && dirfd > FD_LIMIT {
            return -1;
        }
        if let Some(filedescriptor) = &inner.fd_table[dirfd] {
            let file:Arc<OSInode> = match &filedescriptor.ftype {
                FileType::File(file) => file.clone(),
                _ => return -1,
            };
            if let Some(_) = open_file(file.get_name().as_str(), path.as_str(), OpenFlags::CREATE, DiskInodeType::Directory) {
                0
            } else {
                -1
            }
        } else {
            // dirfd 对应条目为 None
            -1
        }
    }
}

/// buf：用于保存当前工作目录的字符串。当 buf 设为 NULL，由系统来分配缓存区
pub fn sys_getcwd(buf: *mut u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();

    if buf as usize == 0 {
        unimplemented!();
    } else {
        let buf_vec = translated_byte_buffer(token, buf, len);
        let mut userbuf = UserBuffer::new(buf_vec);
        let cwd = inner.work_path.as_bytes();
        userbuf.write(cwd);
        return buf as isize;
    }
}

pub fn sys_chdir(path: *const u8) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();

    let path = translated_str(token, path);
    if let Some(new_cwd) = chdir(inner.work_path.as_str(),&path){
        inner.work_path = new_cwd;
        0
    } else {
        -1
    }
    
}
//ztr_getdents
pub fn sys_getdents64(fd: isize, buf: *mut u8, len: usize) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();

    let dirfd = fd as usize;
    if dirfd >= inner.fd_table.len() && dirfd > FD_LIMIT {
        return -1;
    }

    let buf_vec = translated_byte_buffer(token, buf, len);
    let mut userbuf = UserBuffer::new(buf_vec);
    let mut dirent = DirEntry::empty();
    let dent_len = size_of::<DirEntry>();
    let mut total_len: usize = 0;
    if let Some(filedescriptor) = &inner.fd_table[dirfd] {
        let file:Arc<OSInode> = match &filedescriptor.ftype {
            FileType::File(file) => file.clone(),
            _ => return -1,
        };
        loop {
            if total_len + dent_len > len {
                break;
            }
            if file.get_dirent(&mut dirent) > 0 {
                // 写入 userbuf
                userbuf.write_at(total_len, dirent.as_bytes());
                // 更新长度
                total_len += dent_len;
            } else {
                break;
            }
        }
        total_len as isize
    } else {
        -1
    }
}
//ztr_fstat
pub fn sys_fstat(fd: isize, buf: *mut u8) -> isize {
    let token = current_user_token();
    let task = current_task().unwrap();
    let buf_vec = translated_byte_buffer(token, buf, size_of::<Kstat>());
    let inner = task.inner_exclusive_access();

    let mut userbuf = UserBuffer::new(buf_vec);
    let mut kstat = Kstat::new();

    let dirfd = fd as usize;
    if dirfd >= inner.fd_table.len() && dirfd > FD_LIMIT {
        return -1;
    }
    if let Some(filedescriptor) = &inner.fd_table[dirfd] {
        let file:Arc<OSInode> = match &filedescriptor.ftype {
            FileType::File(file) => file.clone(),
            _ => return -1,
        };
        file.get_fstat(&mut kstat);
        userbuf.write(kstat.as_bytes());
        0
    } else {
        -1
    }
}

//ztr_pipe
pub fn sys_pipe(pipe: *mut u32, flag: usize) -> isize {
    let task = current_task().unwrap();
    let token = current_user_token();
    let mut inner = task.inner_exclusive_access();

    // todo 
    _ = flag;

    let (pipe_read, pipe_write) = make_pipe();
    let read_fd = inner.alloc_fd();
    inner.fd_table[read_fd] = Some(FileDescriptor::new(
        true,
        FileType::Abstr(pipe_read),
    ));
    let write_fd = inner.alloc_fd();
    inner.fd_table[write_fd] = Some(FileDescriptor::new(
        true,
        FileType::Abstr(pipe_write),
    ));
    *translated_refmut(token, pipe) = read_fd as u32;
    *translated_refmut(token, unsafe { pipe.add(1) }) = write_fd as u32;
    0
}
// pub fn sys_open(path: *const u8, flags: u32) -> isize {
//     let task = current_task().unwrap();
//     let token = current_user_token();
//     let path = translated_str(token, path);
//     let open_flags = OpenFlags::from_bits(flags).unwrap();
//     let mut inner = task.inner_exclusive_access();
//     if let Some(inode) = open_file(
//         inner.get_work_path().as_str(),
//         path.as_str(),
//         open_flags,
//         DiskInodeType::File,
//     ) {
//         let fd = inner.alloc_fd();
//         inner.fd_table[fd] = Some(FileDescriptor::new(
//             open_flags.contains(OpenFlags::CLOEXEC),
//             FileType::File(inode),
//         ));
//         fd as isize
//     } else {
//         -1
//     }
// }
// pub fn sys_open(path: *const u8, flags: u32) -> isize {
//     let task = current_task().unwrap();
//     let token = current_user_token();
//     let path = translated_str(token, path);
//     //ztr_file
//     let mut inner = task.inner_exclusive_access();
//     let current_path = inner.work_path.as_str();
//     let open_flags = OpenFlags::from_bits(flags).unwrap();
//     if let Some(inode) = open_file(
//         current_path,
//         path.as_str(),
//         open_flags,
//         DiskInodeType::File
//     ) {
//         let fd = inner.alloc_fd();
//         inner.fd_table[fd] = Some(inode);
//         fd as isize
//     } else {
//         -1
//     }
// }

//ztr_mount
pub fn sys_mount(special: *const u8, dir: *const u8, fstype: *const u8, flags: usize, data: *const u8) -> isize {
    let token = current_user_token();
    let special = translated_str(token, special);
    let dir = translated_str(token, dir);
    let fstype = translated_str(token, fstype);

    _ = data;

    MNT_TABLE.exclusive_access().mount(special, dir, fstype, flags as u32)
}

pub fn sys_umount(p_special: *const u8, flags: usize) -> isize {
    let token = current_user_token();
    let special = translated_str(token, p_special);
    MNT_TABLE.exclusive_access().umount(special, flags as u32)
}

pub fn sys_close(fd: usize) -> isize {
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        drop(inner);
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        drop(inner);
        return -1;
    }
    inner.fd_table[fd].take();
    drop(inner);
    0
}
