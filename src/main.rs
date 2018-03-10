#![feature(catch_expr)]

extern crate libc;

mod config;

use std::io::{self, Write};
use std::ffi::{CString, CStr};

fn main() {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let mut input_line = String::with_capacity(256);
    let mut argv = Vec::with_capacity(16);
    let errno = unsafe { libc::__errno_location() };
    //let mut errno = unsafe { libc::__errno_location() };

    // Mask out some signals
    /* unsafe {
        let mut signals: libc::sigset_t = std::mem::uninitialized();
        libc::sigemptyset(&mut signals);
        libc::sigaddset(&mut signals, libc::SIGABRT);
        libc::sigprocmask(libc::SIG_SETMASK, &signals, std::ptr::null_mut());
    } */

    let (path_list, owned_exports) = match config::load_settings() {
        Ok((path, owned_exports)) => (path, owned_exports),
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
            handle.write(b"brick@cohiba % ")?;
            handle.flush()?;
            io::stdin().read_line(&mut input_line)?;
            let _ = input_line.pop(); // Newline
            input_line = input_line.trim().into(); // Spaces
            input_line.push('\0'); // Needed because libc expects null termined arguments
            Ok(())
        };

        if let Err(e) = result {
            eprintln!("Error performing shell I/O:{:?}", e);
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

            let binary_name = unsafe { CStr::from_ptr(argv[0] as *const i8) };

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
                    let mut status: i32 = unsafe { std::mem::uninitialized() };
                    unsafe { libc::wait(&mut status as *mut i32) };
                    if unsafe { *errno == 0 } {
                        success = true;
                        break;
                    } else if unsafe { *errno } == libc::EACCES {
                        no_access = true;
                    }
                }
            }

            if !success && no_access {
                eprintln!("Found matching executable for {:?} on path, but didn't have rights to execute it.", binary_name);
            } else if !success {
                eprintln!("Command not found {:?}.", binary_name);
            }
        }

        input_line.clear();
        argv.clear();
    }
}
