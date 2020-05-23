extern crate imagemagick_sys;

use libc;
use std::ffi::CStr;
use std::str;

extern "C" {
    // pub fn GetMagickVersion(arg1: *mut usize) -> *const libc::c_char;
    pub fn GetMagickPackageName() -> *const libc::c_char;
}

#[test]
fn it_works() {
    let c_buf: *const libc::c_char = unsafe { GetMagickPackageName() };
    let c_str: &CStr = unsafe { CStr::from_ptr(c_buf) };
    let str_slice: &str = c_str.to_str().unwrap();
    assert_eq!("ImageMagick", str_slice);
}
