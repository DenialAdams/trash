use std::{self, env};
use std::collections::HashMap;
use std::convert::From;
use std::path::PathBuf;
use std::ffi::{self, CString};
use std::fs::File;
use std::io::{self, BufReader, BufRead};
use libc;

pub enum Error {
    IoError(io::Error),
    IntoStringError(ffi::IntoStringError),
    ParseError(String),
    NulError(ffi::NulError)
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::IoError(e)
    }
}

impl From<ffi::IntoStringError> for Error {
    fn from(e: ffi::IntoStringError) -> Error {
        Error::IntoStringError(e)
    }
}

impl From<ffi::NulError> for Error {
    fn from(e: ffi::NulError) -> Error {
        Error::NulError(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
        match *self {
            Error::IoError(ref e) => write!(f, "Encountered I/O error while attempting to load .trashrc: {}", e),
            Error::IntoStringError(ref e) => write!(f, "Failed to parse pw_dir as String: {}", e),
            Error::ParseError(ref e) => write!(f, "Error while parsing .trashrc: {}", e),
            Error::NulError(ref e) => write!(f, "Interior null byte found when parsing aliases or exports, don't pull null bytes there: {}", e)
        }
    }
}

enum ParserState {
    LookingForSection,
    PathSection,
    ExportsSection,
    AliasesSection,
}

/// Loads the .trashrc in the user's home directory
pub fn load_settings() -> Result<((Vec<PathBuf>, Vec<CString>, HashMap<CString, String>)), Error> {
    let mut exports: Vec<CString> = Vec::with_capacity(16);
    let mut path: Vec<PathBuf> = Vec::with_capacity(16);
    let mut aliases: HashMap<CString, String> = HashMap::with_capacity(16);

    // Now, read the RC file if it exists
    // Find the user's home directory
    let home_dir = if let Ok(value) = env::var("HOME") {
        value
    } else {
        // I'm not sure under what circumstances HOME would be unset, so this may be wholly unnecessary
        // @Robustness getpwuid is not re-entrant, it's fine for us but just for kicks maybe we shoudld use getpwuid_r
        let home_dir = unsafe {
            let user_id = libc::getuid();
            let pwid_ptr = libc::getpwuid(user_id);
            CString::from_raw((*pwid_ptr).pw_dir).into_string()?
        };
        env::set_var("HOME", &home_dir);
        home_dir
    };

    let mut trash_rc_path = PathBuf::from(home_dir);
    trash_rc_path.push(".trashrc");

    if trash_rc_path.is_file() {
        let f = File::open(trash_rc_path)?;
        let f = BufReader::new(f);

        let mut parser_state = ParserState::LookingForSection;
        let mut visited_path = false;
        let mut visited_exports = false;
        let mut visited_aliases = false;
        let mut expected_open = false;

        // @Helpfulness add line to error
        for line in f.lines() {
            let line = line?;
            for token in line.split_whitespace() {
                if token == "#" {
                    break;
                }
                if expected_open {
                    if token == "{" {
                        expected_open = false;
                        continue;
                    } else {
                        let issue = match parser_state {
                            ParserState::LookingForSection => unreachable!(),
                            ParserState::PathSection => "PATH section identifier was not immediately proceeded by an opening section token `{`",
                            ParserState::ExportsSection => "EXPORTS section identifier was not immediately proceeded by an opening section token `{`",
                            ParserState::AliasesSection => "ALIASES section identifier was not immediately proceeded by an opening section token `{`",
                        };
                        return Err(Error::ParseError(issue.into()));
                    }
                }
                match token {
                    "PATH" => {
                        if visited_path {
                            return Err(Error::ParseError("Encountered PATH identifier but PATH already set".into()));
                        }
                        match parser_state {
                            ParserState::LookingForSection => (),
                            ParserState::PathSection => return Err(Error::ParseError("Encountered PATH section identifier while still processing PATH".into())),
                            ParserState::ExportsSection => return Err(Error::ParseError("Encountered PATH section identifier while still processing EXPORTS".into())),
                            ParserState::AliasesSection => return Err(Error::ParseError("Encountered ALIASES section identifier while still processing ALIASES".into())),
                        }
                        expected_open = true;
                        parser_state = ParserState::PathSection;
                    },
                    "EXPORTS" => {
                        if visited_exports {
                            return Err(Error::ParseError("Encountered EXPORTS identifier but EXPORTS already set".into()));
                        }
                        match parser_state {
                            ParserState::LookingForSection => (),
                            ParserState::PathSection => return Err(Error::ParseError("Encountered EXPORTS identifier while still processing PATH".into())),
                            ParserState::ExportsSection => return Err(Error::ParseError("Encountered EXPORTS identifier while still processing EXPORTS".into())),
                            ParserState::AliasesSection => return Err(Error::ParseError("Encountered ALIASES identifier while still processing ALIASES".into())),
                        }
                        expected_open = true;
                        parser_state = ParserState::ExportsSection;
                    },
                    "ALIASES" => {
                        if visited_aliases {
                            return Err(Error::ParseError("Encountered ALIASES identifier but ALIASES already set.".into()));
                        }
                        expected_open = true;
                        parser_state = ParserState::AliasesSection;
                    },
                    "}" => {
                        match parser_state {
                            ParserState::LookingForSection => return Err(Error::ParseError("Encountered closing section token `}` but no section was open".into())),
                            ParserState::ExportsSection => {
                                visited_exports = true
                            },
                            ParserState::PathSection => {
                                visited_path = true
                            },
                            ParserState::AliasesSection =>{
                                visited_aliases = true
                            },
                        }
                        parser_state = ParserState::LookingForSection;
                    },
                    "{" => {
                        return Err(Error::ParseError("Received opening section token `{` but without a preceding identifier".into()));
                    },
                    _ => {
                        match parser_state {
                            ParserState::LookingForSection => {
                                return Err(Error::ParseError(format!("Encountered unexpected token `{}`; expected section identifier", token)));
                            },
                            ParserState::PathSection => {
                                path.push(PathBuf::from(token))
                            },
                            ParserState::ExportsSection => {
                                let bytes: Vec<u8> = token.bytes().collect();
                                exports.push(CString::new(bytes)?);
                            },
                            ParserState::AliasesSection => {
                                let alias: Vec<&str> = line.trim().splitn(2, |x| x == '=').collect();
                                if alias.len() != 2 {
                                    return Err(Error::ParseError(format!("Failed to create alias from `{}`", line.trim())));
                                }
                                let mut replacement = String::from(alias[1]);
                                unsafe {
                                    for byte in replacement.as_bytes_mut() {
                                        if *byte == b' ' {
                                            *byte = 0;
                                        }
                                    }
                                }
                                replacement.push('\0');
                                aliases.insert(CString::new(alias[0])?, replacement);
                                break
                            },
                        }
                    }
                }
            }
        }

        match parser_state {
            ParserState::LookingForSection => (),
            ParserState::PathSection => return Err(Error::ParseError("Still parsing PATH section when end of .trashrc was reached".into())),
            ParserState::ExportsSection => return Err(Error::ParseError("Still parsing EXPORTS section when end of .trashrc was reached".into())),
            ParserState::AliasesSection => return Err(Error::ParseError("Still parsing ALIASES section when end of .trashrc was reached".into()))
        }
    }

    // If a PATH is already set, append those values
    if let Ok(path_string) = env::var("PATH") {
        for segment in path_string.split(":") {
            path.push(PathBuf::from(segment));
        }
    }

    Ok((path, exports, aliases))
}
