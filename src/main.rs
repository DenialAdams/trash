#![feature(catch_expr)]

extern crate libc;
extern crate termcolor;

mod config;
mod prompt;

use std::io::{self, Write};
use std::ffi::{CString, CStr};
use std::env;
use termcolor::{ColorChoice, StandardStream};
use std::path::Path;

fn main() {
    let stdout = StandardStream::stdout(ColorChoice::Auto);
    let mut handle = stdout.lock();
    let mut input_line = String::with_capacity(256);
    let mut argv = Vec::with_capacity(16);
    let mut exit_status = 0;

    // Mask out some signals
    /* unsafe {
        let mut signals: libc::sigset_t = std::mem::uninitialized();
        libc::sigemptyset(&mut signals);
        libc::sigaddset(&mut signals, libc::SIGABRT);
        libc::sigprocmask(libc::SIG_SETMASK, &signals, std::ptr::null_mut());
    } */

    let user_id = unsafe { libc::getuid() };
    let (home_dir, user_name) = unsafe {
        let pwid_ptr = libc::getpwuid(user_id);

        if pwid_ptr.is_null() {
            match { *libc::__errno_location() } {
                libc::EIO => eprintln!("I/O error occurred while trying to access user information"),
                libc::EINTR => eprintln!("Signal caught while trying to access user information"), // @Robustness do we handle this?
                libc::EMFILE => eprintln!("Have no more file descriptors available; can't access user information"),
                _ => eprintln!("Unknown error occurred while trying to access user information")
            }
            std::process::exit(-1);
        }

        let home_dir = if let Ok(value) = env::var("HOME") {
            value
        } else {
            let value = CStr::from_ptr((*pwid_ptr).pw_dir).to_str().unwrap_or_else(|_| {
                eprintln!("$HOME not set and home directiory in pw_dir contained invalid utf-8");
                std::process::exit(-1);
            });
            value.to_string()
        };

        let user_name = if let Ok(value) = env::var("USER") {
            value
        } else {
            let value = CStr::from_ptr((*pwid_ptr).pw_name).to_str().unwrap_or_else(|_| {
                eprintln!("$USER not set and user information in pwid_ptr contained invalid utf-8");
                std::process::exit(-1);
            });
            value.to_string()
        };

        (home_dir, user_name)
    };

    let (path_list, owned_exports, aliases) = match config::load_settings(&home_dir) {
        Ok((path, owned_exports, aliases)) => (path, owned_exports, aliases),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(-1);
        }
    };

    let mut exports: Vec<*const i8> = owned_exports.iter().map(|c_string| c_string.as_ptr()).collect();
    exports.push(std::ptr::null());

    loop {
        input_line.clear();
        argv.clear();
        
        // IO: print out, get input in
        let result: Result<(), io::Error> = do catch {
            prompt::write_prompt(&mut handle, &user_name, user_id, &home_dir, exit_status)?;
            handle.flush()?;
            io::stdin().read_line(&mut input_line)?;
            let _ = input_line.pop(); // Newline
            input_line = input_line.trim().into(); // Spaces
            Ok(())
        };

        if let Err(e) = result {
            eprintln!("Error performing shell I/O: {:?}", e);
            break;
        }

        if input_line.is_empty() {
            continue;
        }

        let do_aliases = if input_line.starts_with("\\") {
            input_line.remove(0);
            false
        } else {
            true
        };


        {
            let command: Vec<&str> = input_line.split_whitespace().collect();

            // Bultin check
            match command[0] {
                "cd" => {
                    let result: Result<(), _> = if command.len() > 2 {
                        eprintln!("cd: Expected 0 or 1 arguments, got {}", command.len() - 1);
                        exit_status = 1;
                        Ok(())
                    } else if command.len() == 1 {
                        env::set_current_dir(Path::new(&home_dir))
                    } else {
                        env::set_current_dir(Path::new(command[1]))
                    };

                    if let Err(e) = result {
                        eprintln!("cd: {}", e);
                        exit_status = 1;
                    }

                    continue;
                },
                _ => ()
            }
        }

        // Not a builtin, proceed to split it up for fork+exec
        input_line.push('\0'); // Needed because libc expects null termined arguments

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

        {
            let mut no_access = false;
            let mut success = false;

            let mut binary_name = unsafe { CStr::from_ptr(argv[0] as *const i8) };

            // Alias handling
            if do_aliases {
                if let Some(replacement) = aliases.get(binary_name) {
                    argv[0] = replacement.as_ptr();
                    for token in replacement.split("\0").skip(1).filter(|x| !x.is_empty()) {
                        argv.insert(1, token.as_ptr())
                    }
                    binary_name = unsafe { CStr::from_ptr(replacement.as_ptr() as *const i8) };
                }
            }

            // Path lookup + execution
            for path in path_list.iter() {
                let mut temp_path = path.clone();
                temp_path.push(binary_name.to_str().unwrap());
                {
                    let full_path = CString::new(temp_path.to_str().unwrap()).unwrap();
                    unsafe { *libc::__errno_location() = 0 };
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
                        } else if pid == -1 {
                            match unsafe { *libc::__errno_location() } {
                                libc::EAGAIN => eprintln!("Can't allocate resources to fork"),
                                libc::ENOMEM => eprintln!("Can't allocate memory to fork"),
                                libc::ENOSYS => eprintln!("Fork unsupported on this platform"),
                                _ => eprintln!("Unknown error occurred while trying to wait for child process")
                            }

                            std::process::exit(-1);
                        }
                    }

                    // Wait for our child to finish
                    let mut wstatus: i32 = unsafe { std::mem::uninitialized() };
                    {
                        let wait_ret_val = unsafe { libc::wait(&mut wstatus as *mut i32) };
                        if wait_ret_val == -1 {
                            match unsafe { *libc::__errno_location() } {
                                libc::ECHILD => eprintln!("Somehow, no child process to wait for"),
                                libc::EINTR => eprintln!("Signal caught while waiting for child process"), // @Robustness do we handle this?
                                _ => eprintln!("Unknown error occurred while trying to wait for child process")
                            }

                            std::process::exit(-1);
                        }
                    }

                    if unsafe { *libc::__errno_location() == 0 } {
                        success = true;
                        exit_status = unsafe { libc::WEXITSTATUS(wstatus) };
                        break;
                    } else if unsafe { *libc::__errno_location() } == libc::EACCES {
                        no_access = true;
                    }
                }
            }

            if !success && no_access {
                eprintln!("Found matching item for {:?} on path, but couldn't execute it", binary_name);
                exit_status = 126;
            } else if !success {
                eprintln!("Command not found {:?}.", binary_name);
                exit_status = 127;
            }
        }
    }
}
