use libc;
use std::ffi::CStr;
use std::io::Write;
use std;
use termcolor::{self, ColorSpec, Color, WriteColor};

pub fn write_prompt(buf: &mut termcolor::StandardStreamLock, username: &str, user_id: libc::uid_t, home_dir: &str, status: i32) -> Result<(), std::io::Error> {
    // Status
    if status != 0 {
        buf.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
        write!(buf, "{} ", status)?;
        buf.reset()?;
    }

    // Username

    if user_id == 0{
        buf.set_color(ColorSpec::new().set_fg(Some(Color::Red)).set_bold(true))?;
    } else {
        buf.set_color(ColorSpec::new().set_fg(Some(Color::Blue)).set_bold(true))?;
    }
    write!(buf, "{}", username)?;
    buf.reset()?;

    // Hostname
    let mut hostname_container: Box<[i8; 16]> = Box::new([0; 16]);
    let ret_val = unsafe { libc::gethostname(hostname_container.as_mut_ptr(), hostname_container.len() - 1) };
    let print_hostname = if ret_val == -1 {
        // match errno
        match unsafe { *libc::__errno_location() } {
            libc::ENAMETOOLONG => {
                // This is fine, we will just print truncated version
                true
            },
            _ => {
                false
            }
        }
    } else {
        true
    };

    if print_hostname {
        let hostname = unsafe { CStr::from_ptr(hostname_container.as_ptr()).to_str().unwrap() };

        write!(buf, "@{} ", hostname)?;
    }

    // Current Directory
    let mut current_directory = std::env::current_dir()?.to_string_lossy().into_owned();
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
