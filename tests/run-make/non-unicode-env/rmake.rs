extern crate run_make_support;

use run_make_support::rustc;

fn main() {
    #[cfg(unix)]
    let non_unicode: &std::ffi::OsStr = std::os::unix::ffi::OsStrExt::from_bytes(&[0xFF]);
    #[cfg(windows)]
    let non_unicode: std::ffi::OsString = std::os::windows::ffi::OsStringExt::from_wide(&[0xD800]);
    let output = rustc().input("non_unicode_env.rs").env("NON_UNICODE_VAR", non_unicode).run_fail();
    let actual = std::str::from_utf8(&output.stderr).unwrap();
    let expected = std::fs::read_to_string("non_unicode_env.stderr").unwrap();
    assert_eq!(actual, expected);
}
