use libc;
use std::ffi::{CStr, CString};
use std::fmt::Write;
use std;

pub fn generate_prompt(buf: &mut String, home_dir: &str, status: i32) {
    buf.clear();

    // Status
    if status != 0 {
        write!(buf, "{} ", status);
    }

    // Username
    let username = unsafe {
        let user_id = libc::getuid();
        let pwid_ptr = libc::getpwuid(user_id);
        CStr::from_ptr((*pwid_ptr).pw_name).to_str().unwrap()
    };

    // Hostname
    let mut hostname_container: Box<[i8; 16]> = Box::new([0; 16]);
    unsafe { libc::gethostname(hostname_container.as_mut_ptr(), 15) };
    let hostname = unsafe { CStr::from_ptr(hostname_container.as_ptr()).to_str().unwrap() };

    // Current Directory
    let mut current_directory = unsafe {
        let mut dir_container: Box<[i8; 64]> = Box::new([0; 64]);
        libc::getcwd(dir_container.as_mut_ptr(), 64);
        let dir = CStr::from_ptr(dir_container.as_ptr());
        dir.to_str().unwrap().to_string()
    };
    if current_directory.starts_with(home_dir) {
        current_directory = current_directory.replacen(home_dir, "~", 1);
    }

    // Source control
    
    write!(buf, "{}@{} {} % ", username, hostname, current_directory);
}
