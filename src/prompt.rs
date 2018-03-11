use libc;
use std::ffi::{CStr, CString};
use std::fmt::Write;

pub fn generate_prompt(buf: &mut String, home_dir: &str) {
    buf.clear();
    let username = unsafe {
        let user_id = libc::getuid();
        let pwid_ptr = libc::getpwuid(user_id);
        CStr::from_ptr((*pwid_ptr).pw_name).to_str().unwrap()
    };
    let mut hostname_container: Box<[i8; 16]> = Box::new([0; 16]);
    unsafe { libc::gethostname(hostname_container.as_mut_ptr(), 15) };
    let hostname = unsafe { CStr::from_ptr(hostname_container.as_ptr()).to_str().unwrap() };
    // current directory
    let mut dir_container: Box<[i8; 64]> = Box::new([0; 64]);
    let mut current_directory = unsafe { CString::from_raw(libc::getcwd(dir_container.as_mut_ptr(), 64)).into_string().unwrap() };
    if current_directory.starts_with(home_dir) {
        current_directory = current_directory.replacen(home_dir, "~", 1);
    }
    // source control
    write!(buf, "{}@{} {} % ", username, hostname, current_directory);
}
