//! Starts CPython with the same AddressSanitizer runtime as the tested
//! extension.

use std::{
    env,
    ffi::CString,
    os::{
        raw::{c_char, c_int},
        unix::ffi::OsStrExt,
    },
};

unsafe extern "C" {
    fn Py_BytesMain(argc: c_int, argv: *mut *mut c_char) -> c_int;
}

fn main() {
    // The first supplied argument is the virtual environment's Python executable.
    let arguments: Vec<_> = env::args_os()
        .skip(1)
        .map(|argument| CString::new(argument.as_bytes()).expect("argument contains NUL"))
        .collect();
    assert!(
        !arguments.is_empty(),
        "expected Python executable and arguments"
    );
    let mut pointers: Vec<_> = arguments
        .iter()
        .map(|argument| argument.as_ptr().cast_mut())
        .collect();
    let argc = c_int::try_from(pointers.len()).expect("too many arguments");
    pointers.push(std::ptr::null_mut());
    // SAFETY: CPython reads these NUL-terminated arguments during this call.
    // Both the strings and the writable pointer array outlive the interpreter.
    let status = unsafe { Py_BytesMain(argc, pointers.as_mut_ptr()) };
    std::process::exit(status);
}
