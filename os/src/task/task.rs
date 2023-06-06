//!Implementation of [`TaskControlBlock`]
use super::TaskContext;
use super::{pid_alloc, KernelStack, PidHandle};
use crate::config::{TRAP_CONTEXT, PAGE_SIZE, USER_HEAP_SIZE};
use crate::fs::{File, Stdin, Stdout, FileDescriptor, FileType};
use crate::mm::{MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE, MapPermission, MMapArea, MapType, VirtPageNum};
use crate::sync::UPSafeCell;
use crate::trap::{trap_handler, TrapContext};
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
//ztr_file
use alloc::string::String;
use riscv::register::fcsr::{Flags, Flag};
use core::cell::RefMut;
use core::iter::Map;
use core::panic;

pub struct TaskControlBlock {
    // immutable
    pub pid: PidHandle,
    pub kernel_stack: KernelStack,
    // mutable
    inner: UPSafeCell<TaskControlBlockInner>,
}

pub struct TaskControlBlockInner {
    pub trap_cx_ppn: PhysPageNum,
    pub base_size: usize,
    pub task_cx: TaskContext,
    pub task_status: TaskStatus,
    pub memory_set: MemorySet,
    pub parent: Option<Weak<TaskControlBlock>>,
    pub children: Vec<Arc<TaskControlBlock>>,
    pub exit_code: i32,
    pub fd_table: Vec<Option<FileDescriptor>>,
    //ztr_file
    pub work_path: String,
}

impl TaskControlBlockInner {
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
    //ztr_open
    pub fn get_work_path(&self) -> String {
        self.work_path.clone()
    }
}

impl TaskControlBlock {
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }
    pub fn new(elf_data: &[u8]) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: user_sp,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: vec![
                        // 0 -> stdin
                        Some(FileDescriptor::new(false, FileType::Abstr(Arc::new(Stdin)))),
                        // 1 -> stdout
                        Some(FileDescriptor::new(
                            false,
                            FileType::Abstr(Arc::new(Stdout)),
                        )),
                        // 2 -> stderr
                        Some(FileDescriptor::new(
                            false,
                            FileType::Abstr(Arc::new(Stdout)),
                        )),
                    ],
                    work_path: String::from("/"),
                })
            }
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
    pub fn exec(&self, elf_data: &[u8]) {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();      
        // **** access current TCB exclusively
        let mut inner = self.inner_exclusive_access();
        // substitute memory_set
        inner.memory_set = memory_set;
        // update trap_cx ppn
        inner.trap_cx_ppn = trap_cx_ppn;
        // initialize trap_cx  
        let f_user_sp  = user_sp - 8;       
        let trap_cx = TrapContext::app_init_context(
            entry_point,
            f_user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        *inner.get_trap_cx() = trap_cx;
        // **** release current PCB
    }
    pub fn fork(self: &Arc<TaskControlBlock>) -> Arc<TaskControlBlock> {
        // ---- hold parent PCB lock
        let mut parent_inner = self.inner_exclusive_access();
        // copy user space(include trap context)
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        // copy fd table
        let mut new_fd_table: Vec<Option<FileDescriptor>> = Vec::new();
        for fd in parent_inner.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: parent_inner.base_size,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    fd_table: new_fd_table,
                    work_path: parent_inner.work_path.clone(),
                })
            },
        });
        // add child
        parent_inner.children.push(task_control_block.clone());
        // modify kernel_sp in trap_cx
        // **** access child PCB exclusively
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;
        // return
        task_control_block
        // **** release child PCB
        // ---- release parent PCB
    }
    pub fn getpid(&self) -> usize {
        self.pid.0
    }
    //ztr_mmap
    pub fn mmap(&self, start: usize, len: usize, prot: u32, _flags: u32, fd: usize, off: usize) -> isize {
        let mut inner = self.inner_exclusive_access();
        //将usize转换为虚拟地址
        let start_addr = VirtAddr::from(start);
        //获取当前地址所在虚拟地址页号
        let mut start_vpn = start_addr.floor();
        //获取文件描述符表
        let fd_table = inner.fd_table.clone();
        //确定权限控制
        let map_perm = (((prot & 0b111)<<1) + (1<<4))  as u8;
        
        //当start有指定值时，需判断当前虚拟地址是否已经被分配
        if start != 0 {
            if start % PAGE_SIZE!= 0 {
                panic!("mmap: The address :{} is illegal!", start);
            }
            //检查当前地址到分配结束是否被占用
            while start_vpn.0 < (start + len) % PAGE_SIZE {
                if !inner.memory_set.translate(start_vpn).unwrap().is_valid() {
                    return -1;
                }
                start_vpn.0 += 1;
            }
            //如果没有被占用，则插入mmap区域，需要确定是否插入成功
            let mmap_areas = MMapArea::new(VirtAddr::from(start), VirtAddr::from(start+ len), MapPermission::from_bits(map_perm).unwrap(), MapType::Framed, fd, _flags as usize, off);
            let tags = inner.memory_set.push_mmap_area(mmap_areas, fd_table);
            drop(inner);
            if tags == 0 {
                return -1;
            }
            return start as isize;
        }
        //如果为NULL，自主找到空闲区域进行分配
        else {
            let re_addr = VirtAddr::from(inner.memory_set.get_max_vpn()).0;
            let mmap_areas = MMapArea::new(VirtAddr::from(re_addr), VirtAddr::from(re_addr + len), MapPermission::from_bits(map_perm).unwrap(), MapType::Framed, fd, _flags as usize, off);
            let tags = inner.memory_set.push_mmap_area(mmap_areas, fd_table);
            inner.memory_set.set_max_vpn(re_addr + len);
            drop(inner);
            if tags == 0 {
                return -1;
            }
            return re_addr as isize;
        }
    }

    pub fn munmap(&self, start: usize, len: usize) -> isize {
        let mut inner = self.inner_exclusive_access();
        let tags = inner.memory_set.remove_MMapArea_with_start_vpn(VirtPageNum::from(start), VirtPageNum::from(start + len));
        drop(inner);
        tags
    }
}

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    Ready,
    Running,
    Zombie,
}
