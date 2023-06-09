#[allow(unused)]
use clap::{App, Arg};
#[allow(unused)]
use easy_fs::{
    BlockDevice, FAT32Manager, VFile, ShortDirEntry, ATTRIBUTE_ARCHIVE, ATTRIBUTE_DIRECTORY,
};
#[allow(unused)]
use std::fs::{read_dir, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
#[allow(unused)]
use std::sync::Arc;
use std::sync::Mutex;

const BLOCK_SZ: usize = 512;

struct BlockFile(Mutex<File>);

impl BlockDevice for BlockFile {
    fn read_block(&self, block_id: usize, buf: &mut [u8]) {
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SZ) as u64))
            .expect("Error when seeking!");
        assert_eq!(file.read(buf).unwrap(), BLOCK_SZ, "Not a complete block!");
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) {
        let mut file = self.0.lock().unwrap();
        file.seek(SeekFrom::Start((block_id * BLOCK_SZ) as u64))
            .expect("Error when seeking!");
        assert_eq!(file.write(buf).unwrap(), BLOCK_SZ, "Not a complete block!");
    }
}

fn main() {
    fat32_pack().expect("Error when packing fat32!");
}

fn fat32_pack() -> std::io::Result<()> {
    // clap::matches 用于捕获用户输入的参数
    // 在makefile中，命令为
    // @cd ../easy-fs-fuse && cargo run --release \
    // -- -s ../user/src/bin/ \
    // -t ../user/target/riscv64gc-unknown-none-elf/release/
    // 因此得到的参数就是两个路径
    let matches = App::new("EasyFileSystem packer")
        .arg(
            Arg::with_name("source")
                .short("s") // 对应输入的 -s
                .long("source") //对应输入 --source
                .takes_value(true)
                .help("Executable source dir(with backslash)"),
        )
        .arg(
            Arg::with_name("target")
                .short("t")
                .long("target")
                .takes_value(true)
                .help("Executable target dir(with backslash)"),
        )
        .get_matches();
    let src_path = matches.value_of("source").unwrap();
    let target_path = matches.value_of("target").unwrap();
    println!("src_path = {}\ntarget_path = {}", src_path, target_path);

    // 打开U盘
    let block_file = Arc::new(BlockFile(Mutex::new({
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            // .open("/dev/sdb")?;
            .open("fat32.img")?;
        // 注意当新建文件的时候大小是0，需要先设置大小，这样读块的时候不会出错
        f.set_len(8192 * 512).unwrap();
        f
    })));

    let fs_manager = FAT32Manager::open(block_file.clone());
    let fs_reader = fs_manager.read();
    let root_inode = fs_reader.get_root_vfile(&fs_manager);
    println!("first date sec = {}", fs_reader.first_data_sector());
    drop(fs_reader);

    // 从host获取应用名
    let apps: Vec<_> = read_dir(src_path)
        .unwrap()
        .into_iter()
        .map(|dir_entry| {
            let mut name_with_ext = dir_entry.unwrap().file_name().into_string().unwrap();
            // 丢弃后缀 从'.'到末尾(len-1)
            name_with_ext.drain(name_with_ext.find('.').unwrap()..name_with_ext.len());
            name_with_ext
        })
        .collect();
    for app in apps {
        // load app data from host file system
        let mut host_file = File::open(format!("{}{}", target_path, app)).unwrap();
        let mut all_data: Vec<u8> = Vec::new();
        host_file.read_to_end(&mut all_data).unwrap();
        // create a file in easy-fs
        println!("before create");
        let o_inode = root_inode.create(app.as_str(), ATTRIBUTE_ARCHIVE);
        if o_inode.is_none() {
            continue;
        }
        let inode = o_inode.unwrap();
        println!("after create");
        // write data to easy-fs
        println!("file_len = {}", all_data.len());
        inode.write_at(0, all_data.as_slice());
        fs_manager.read().cache_write_back();
    }

    // list apps
    for app in root_inode.ls_lite().unwrap() {
        println!("{}", app.0);
    }
    Ok(())
}

#[allow(unused)]
macro_rules! color_text {
    ($text:expr, $color:expr) => {{
        format_args!("\x1b[{}m{}\x1b[0m", $color, $text)
    }};
}

#[test]
fn ufs_test() -> std::io::Result<()> {
    println!("0");
    let block_file = Arc::new(BlockFile(Mutex::new({
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open("./fat32.img")?;
        f
    })));

    let print_flist = |flist: &mut Vec<(String, u8)>| {
        println!("### list:");
        #[allow(unused)]
        for i in 0..flist.len() {
            let part = flist.pop().unwrap();
            let name = part.0;
            let attri = part.1;
            if (attri & ATTRIBUTE_DIRECTORY) != 0 {
                println!("{} ", color_text!(name, 96));
            } else {
                println!("{} ", name);
            }
        }
        println!("### end list")
    };

    let simple_rwtest = |VFile: &VFile| {
        let greet_str = "hello world!\n";
        println!("*** simple r/w test");
        println!("  name = {}", VFile.name);
        println!(
            "  1: write file. wlen={}",
            VFile.write_at(0, greet_str.as_bytes())
        );
        let mut buffer = [0u8; 256];
        let len = VFile.read_at(0, &mut buffer);
        println!("  2: read file. rlen = {}", len);
        println!("  text = {}", core::str::from_utf8(&buffer[..len]).unwrap());
        assert_eq!(greet_str, core::str::from_utf8(&buffer[..len]).unwrap(),);
        println!("*** simple r/w test pass");
    };

    let fs_manager = FAT32Manager::open(block_file.clone());
    let fs_reader = fs_manager.read();
    println!(
        "{:X}",
        fs_reader
            .get_fat()
            .read()
            .get_next_cluster(2, block_file.clone())
    );

    // 测试根目录
    let root_inode = fs_reader.get_root_vfile(&fs_manager);
    drop(fs_reader);
    let mut flist = root_inode.ls_lite().unwrap();
    print_flist(&mut flist);


    // 测试 riscv64-rootfs
    // let riscv64 = root_inode.find_inode_byname("riscv64").unwrap();
    // let mut flist = riscv64.ls_lite().unwrap();
    // print_flist(&mut flist);
    
    // 创建文件
    root_inode.create("hello2", ATTRIBUTE_ARCHIVE).unwrap();
    fs_manager.read().cache_write_back();
    println!("*** after create");
    let hello = root_inode.find_vfile_byname("hello2").unwrap();
    simple_rwtest(&hello);
    fs_manager.read().cache_write_back();
    let hello = root_inode.find_vfile_byname("hello2").unwrap();
    let mut buffer = [0u8; 256];
    let len = hello.read_at(0, &mut buffer);
    println!("  text = {}", core::str::from_utf8(&buffer[..len]).unwrap());
    assert_eq!(
        "hello world!\n",
        core::str::from_utf8(&buffer[..len]).unwrap(),
    );
    let mut flist = root_inode.ls_lite().unwrap();
    print_flist(&mut flist);


    // dirtest
    println!("directory test ... start");
    let dir0 = root_inode.create("dir0", ATTRIBUTE_DIRECTORY).unwrap();

    println!("list root:");
    let mut flist = root_inode.ls_lite().unwrap();
    print_flist(&mut flist);

    println!("list dir0:");
    let mut flist = dir0.ls_lite().unwrap();
    print_flist(&mut flist);

    dir0.create("file1", ATTRIBUTE_ARCHIVE).unwrap();
    let dir0_dir1 = dir0.create("dir1", ATTRIBUTE_DIRECTORY).unwrap();
    dir0_dir1.create("file2", ATTRIBUTE_ARCHIVE);

    let file1 = root_inode.find_vfile_bypath(vec!["dir0", "file1"]).unwrap();
    let file2 = root_inode
        .find_vfile_bypath(vec!["dir0", "dir1", "file2"])
        .unwrap();
    simple_rwtest(&file1);
    simple_rwtest(&file2);
    fs_manager.read().cache_write_back();
    println!("directory test ... end");


    // random str rw test
    println!("random str rw test ... start");
    let filea = root_inode.create("filea", ATTRIBUTE_ARCHIVE).unwrap();
    fs_manager.read().cache_write_back();
    let mut random_str_test = |len: usize| {
        println!("before clear");
        filea.clear();
        println!("after clear");
        assert_eq!(filea.read_at(0, &mut buffer), 0,);
        let mut str = String::new();
        use rand;
        // random digit
        for _ in 0..len {
            str.push(char::from('0' as u8 + rand::random::<u8>() % 10));
        }
        let mut str1 = str.clone();
        filea.write_at(0, str.as_bytes());
        println!("after write, size = {}", filea.get_size());
        unsafe {
            filea.read_at(0, str1.as_bytes_mut());
        }
        assert_eq!(str, str1);
        drop(str1);
        let mut read_buffer = [0u8; 127];
        let mut offset = 0usize;
        let mut read_str = String::new();
        loop {
            let len = filea.read_at(offset, &mut read_buffer);
            if len == 0 {
                break;
            }
            offset += len;
            read_str.push_str(core::str::from_utf8(&read_buffer[..len]).unwrap());
        }
        fs_manager.read().cache_write_back();
        println!("offset = {} ... pass", offset);
        assert_eq!(str.len(), read_str.len());
    };

    random_str_test(4 * BLOCK_SZ);
    random_str_test(8 * BLOCK_SZ + BLOCK_SZ / 2);
    random_str_test(33 * BLOCK_SZ);
    random_str_test(70 * BLOCK_SZ + BLOCK_SZ / 7);
    random_str_test((12 + 128) * BLOCK_SZ);
    random_str_test(400 * BLOCK_SZ);
    random_str_test(1000 * BLOCK_SZ);
    random_str_test(2000 * BLOCK_SZ);
    fs_manager.read().cache_write_back();
    println!("random str rw test ... {}", color_text!("pass", 92));
    assert_eq!(0, 1);
    Ok(())
}
