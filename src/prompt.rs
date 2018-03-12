use libc;
use std::ffi::{CStr, CString};
use std::io::Write;
use std;
use termcolor::{self, ColorSpec, Color, WriteColor};

pub fn write_prompt(buf: &mut termcolor::StandardStreamLock, home_dir: &str, status: i32) -> Result<(), std::io::Error> {
    // Status
    if status != 0 {
        buf.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
        write!(buf, "{} ", status)?;
        buf.reset()?;
    }

    // Username
    let user_id = unsafe { libc::getuid() };
    let username = unsafe {
        let pwid_ptr = libc::getpwuid(user_id);
        CStr::from_ptr((*pwid_ptr).pw_name).to_str().unwrap()
    };

    if user_id == 0{
        buf.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
    } else {
        buf.set_color(ColorSpec::new().set_fg(Some(Color::Blue)).set_bold(true))?;
    }
    write!(buf, "{}", username)?;
    buf.reset()?;

    // Hostname
    let mut hostname_container: Box<[i8; 16]> = Box::new([0; 16]);
    unsafe { libc::gethostname(hostname_container.as_mut_ptr(), 15) };
    let hostname = unsafe { CStr::from_ptr(hostname_container.as_ptr()).to_str().unwrap() };

    write!(buf, "@{} ", hostname)?;

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
    
    buf.set_color(ColorSpec::new().set_bold(true))?;
    write!(buf, "{} ", current_directory)?;
    buf.reset()?;

    write!(buf, "% ")?;

    // Source control

    Ok(())
}
