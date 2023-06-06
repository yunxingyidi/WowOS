use crate::fs::{open_file, OpenFlags, DiskInodeType};
use crate::mm::{translated_refmut, translated_str, UserBuffer, translated_byte_buffer,translated_ref};
use crate::task::{
    add_task, current_task, current_user_token, exit_current_and_run_next,
    suspend_current_and_run_next, Utsname, UTSNAME,
};
use crate::timer::{TimeVal, tms, get_TimeVal, get_time_ms};
use alloc::sync::Arc;
//ztr_brk
use log::{info};

pub fn sys_exit(exit_code: i32) -> ! {
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

pub fn sys_yield() -> isize {
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    // let re = current_task().unwrap().pid.0;
    // re as isize
    2
}
//ztr_getppid
pub fn sys_getppid() -> isize {
    unsafe{
        current_task().unwrap().inner_exclusive_access().parent.clone().unwrap().as_ptr().as_ref().unwrap().pid.0 as isize
    }
}

//ztr_brk
pub fn sys_brk(brk_addr: usize) -> isize{
    //获取当前任务
    let task = current_task().unwrap();
    //当前任务地址空间
    //???
    let memory_set = &mut task.inner_exclusive_access().memory_set;
    let new_ptr;
    if brk_addr == 0 {
        new_ptr = memory_set.sbrk(0);
    } else {
        let former_addr = memory_set.sbrk(0);
        let grow_size: isize = (brk_addr - former_addr) as isize;
        new_ptr = memory_set.sbrk(grow_size);
    }
    drop(memory_set);
    info!(
        "[sys_brk] brk_addr: {:X}; new_addr: {:X}",
        brk_addr, new_ptr
    );
    if new_ptr == 0 {
        -1
    }else{
        new_ptr as isize
    }
}
//ztr_unname
pub fn sys_uname(buf: *const u8) -> isize {
    let token = current_user_token();
    let uname = UTSNAME.exclusive_access();
    let buf_vec = translated_byte_buffer(token, buf, core::mem::size_of::<Utsname>());
    let mut userbuf = UserBuffer::new(buf_vec);
    userbuf.write(uname.as_bytes());
    0
}
//ztr_time
pub fn sys_get_time(buf: *const u8) -> isize {
    let token = current_user_token();
    let buffers = translated_byte_buffer(token, buf, core::mem::size_of::<TimeVal>());
    let mut userbuf = UserBuffer::new(buffers);
    userbuf.write(get_TimeVal().as_bytes());
    0
}
//ztr_sleep
pub fn sys_nanosleep(buf: *const u8) -> isize {
    let tic = get_time_ms();
    println!("Sleep");
    let token = current_user_token();
    let len_timeval = translated_ref(token, buf as *const TimeVal);
    let len = len_timeval.sec * 1000 + len_timeval.usec / 1000;
    loop {
        let toc = get_time_ms();
        if toc - tic >= len {
            break;
        }
    };
    0
}
pub fn sys_fork() -> isize {
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    let token = current_user_token();
    let path = translated_str(token, path);
    //let inner = &mut task.inner_exclusive_access();
    //ztr_file
    if let Some(app_inode) = open_file(
        //ztr_file
        "/",
        path.as_str(),
        OpenFlags::RDONLY,
        DiskInodeType::File,) {
        let all_data = app_inode.read_all();
        let task = current_task().unwrap();
        task.exec(all_data.as_slice());
        0
    }
    else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}
//ztr_mmap
pub fn sys_mmap(start: usize, len: usize, prot: u32, _flags: u32, fd: usize, off: usize) -> isize{
    let task = current_task().unwrap();
    task.mmap(start, len, prot, _flags, fd, off)
}
//ztr_munmap
pub fn sys_munmap(start: usize, len: usize) -> isize {
    let task = current_task().unwrap();
    task.munmap(start, len)
}