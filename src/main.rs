#![feature(catch_expr)]

extern crate libc;
extern crate termcolor;

mod config;
mod prompt;

use std::io::{self, Write};
use std::ffi::{CString, CStr};
use std::env;
use termcolor::{ColorChoice, StandardStream};

fn main() {
    let stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut handle = stdout.lock();
    let mut input_line = String::with_capacity(256);
    let mut argv = Vec::with_capacity(16);
    let errno = unsafe { libc::__errno_location() };
    let mut exit_status = 0;

    // Mask out some signals
    /* unsafe {
        let mut signals: libc::sigset_t = std::mem::uninitialized();
        libc::sigemptyset(&mut signals);
        libc::sigaddset(&mut signals, libc::SIGABRT);
        libc::sigprocmask(libc::SIG_SETMASK, &signals, std::ptr::null_mut());
    } */

    // Find the user's home directory
    let home_dir = if let Ok(value) = env::var("HOME") {
        value
    } else {
        // I'm not sure under what circumstances HOME would be unset, so this may be wholly unnecessary
        // @Robustness getpwuid is not re-entrant, it's fine for us but just for kicks maybe we shoudld use getpwuid_r
        let home_dir = unsafe {
            let user_id = libc::getuid();
            let pwid_ptr = libc::getpwuid(user_id);
            CStr::from_ptr((*pwid_ptr).pw_dir).to_str().unwrap().to_string()
        };
        env::set_var("HOME", &home_dir);
        home_dir
    };

    let (path_list, owned_exports, aliases) = match config::load_settings(&home_dir) {
        Ok((path, owned_exports, aliases)) => (path, owned_exports, aliases),
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };

    let mut exports: Vec<*const i8> = owned_exports.iter().map(|c_string| c_string.as_ptr()).collect();
    exports.push(std::ptr::null());

    loop {
        
        // IO: print out, get input in
        let result: Result<(), io::Error> = do catch {
            prompt::write_prompt(&mut handle, &home_dir, exit_status)?;
            handle.flush()?;
            io::stdin().read_line(&mut input_line)?;
            let _ = input_line.pop(); // Newline
            input_line = input_line.trim().into(); // Spaces
            if input_line.is_empty() {
                continue;
            }
            input_line.push('\0'); // Needed because libc expects null termined arguments
            Ok(())
        };

        if let Err(e) = result {
            eprintln!("Error performing shell I/O: {:?}", e);
            break;
        }

        // split up to command args, etc
        unsafe {
            argv.push(input_line.as_ptr());
            let input_bytes = input_line.as_bytes_mut();
            for byte in input_bytes.iter_mut() {
                if *byte == b' ' {
                    *byte = 0;
                    argv.push((byte as *const u8).offset(1));
                }
            }
            argv.push(std::ptr::null());
        }

        // Path lookup and execution, plus error handling
        {
            let mut no_access = false;
            let mut success = false;

            let mut binary_name = unsafe { CStr::from_ptr(argv[0] as *const i8) };

            if let Some(replacement) = aliases.get(binary_name) {
                argv[0] = replacement.as_ptr();
                for token in replacement.split("\0").skip(1).filter(|x| !x.is_empty()) {
                    argv.insert(1, token.as_ptr())
                }
                binary_name = unsafe { CStr::from_ptr(replacement.as_ptr() as *const i8) };
            }

            for path in path_list.iter() {
                let mut temp_path = path.clone();
                temp_path.push(binary_name.to_str().unwrap());
                {
                    let full_path = CString::new(temp_path.to_str().unwrap()).unwrap();
                    unsafe { *errno = 0 };
                    // Fork + exec
                    {
                        let pid = unsafe { libc::vfork() };

                        // child
                        if pid == 0 {
                            unsafe {
                                libc::execve(full_path.as_ptr(), argv.as_ptr() as *const *const i8, exports.as_ptr());
                                // oh no, we're still executing so something must have gone wrong
                                libc::_exit(127);
                            }
                        }
                    }
                    // Wait for our child to finish
                    let mut wstatus: i32 = unsafe { std::mem::uninitialized() };
                    unsafe { libc::wait(&mut wstatus as *mut i32) };
                    if unsafe { *errno == 0 } {
                        success = true;
                        exit_status = unsafe { libc::WEXITSTATUS(wstatus) };
                        break;
                    } else if unsafe { *errno } == libc::EACCES {
                        no_access = true;
                    }
                }
            }

            if !success && no_access {
                eprintln!("Found matching item for {:?} on path, but didn't have rights to execute it.", binary_name);
                exit_status = 126;
            } else if !success {
                eprintln!("Command not found {:?}.", binary_name);
                exit_status = 127;
            }
        }

        input_line.clear();
        argv.clear();
    }
}
