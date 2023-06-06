#![no_std]
#![no_main]

#[macro_use]

extern crate user_lib;
use user_lib::{exit, fork, wait, waitpid, yield_, brk};
#[no_mangle]
pub fn main() -> isize{
    test_brk()
}

fn test_brk() -> isize {
    let cur_pos = brk(0);
    println!("Before alloc,heap pos:{}", cur_pos);
    brk((cur_pos + 64) as usize);
    let alloc_pos = brk(0);
    println!("After alloc,heap pos: {}",alloc_pos);
    brk((alloc_pos + 64) as usize);
    let alloc_pos_1 = brk(0);
    println!("Alloc again,heap pos: {}",alloc_pos_1);
    0
}
