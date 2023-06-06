use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::sync::Arc;
use crate::fs::{FileDescriptor, FileType};
use crate::{config::PAGE_SIZE, fs::File};


use crate::mm::address::VPNRange;
use crate::mm::memory_set::MapType;

use super::{
    frame_alloc, FrameTracker, MapPermission, page_table::PTEFlags, PageTable,
    PhysPageNum, translated_byte_buffer, UserBuffer, VirtAddr, VirtPageNum, 
};
//MMapArea相较于MapArea多了关于文件映射的信息，例如offset
pub struct MMapArea {
    pub vpn_range: VPNRange,
    pub data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    pub map_type: MapType,
    pub map_perm: MapPermission,
    pub fd: usize,
    pub offset: usize,
    pub flags: usize,
    pub length: usize,
}

impl MMapArea {
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_perm: MapPermission,
        map_type: MapType,
        fd: usize,
        offset: usize,
        flags: usize,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
            fd,
            offset,
            flags,
            length: end_va.0 - start_va.0,
        }
    }

    /// 取消映射所有页
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            if self.data_frames.contains_key(&vpn) {
                self.data_frames.remove(&vpn);
                page_table.unmap(vpn);
            }
        }
    }

    //分配一次
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn: PhysPageNum;
        match self.map_type {
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            MapType::Framed => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits()).unwrap();
        
        page_table.map(vpn, ppn, pte_flags);
    }
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        match self.map_type {
            MapType::Framed => {
                self.data_frames.remove(&vpn);
            }
            _ => {
            }
        }
        page_table.unmap(vpn);
    }
    /// 分配内存, 同时映射文件
    /// 一次分配所有页
    pub fn map(
        &mut self,
        page_table: &mut PageTable,
        fd_table: Vec<Option<FileDescriptor>>
    ) -> isize {
        // 对所有使用的虚拟页号进行与物理内存的映射
        for vpn in self.vpn_range { 
            self.map_one(page_table, vpn);
        }
        if self.fd < 0 {
            return 0;
        }
        //获取分配地址
        let vaddr: usize = VirtAddr::from(self.vpn_range.get_start()).into();
        if let Some(file) = &fd_table[self.fd] {
            let f: Arc<dyn File + Send + Sync> = match &file.ftype {
                FileType::Abstr(f) => f.clone(),
                FileType::File(f) => f.clone(),
                _ => return 0,
            };
            if !f.readable() { 
                return 0; 
            }
            //设置偏移量
            f.set_offset(self.offset);
            // println!{"The va_start is 0x{:X}, offset of file is {}", va_start.0, offset};
            //将文件读入内存
            let _read_len = f.read(UserBuffer::new(translated_byte_buffer(
                page_table.token(),
            vaddr as *const u8,
            self.length,)));
            return vaddr as isize
        }
        else { 
            return 0 
        }
    }
}
