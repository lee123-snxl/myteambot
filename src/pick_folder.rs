use std::ffi::{c_void, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

const BIF_RETURNONLYFSDIRS: u32 = 0x0001;
const BIF_EDITBOX: u32 = 0x0010;
const BIF_VALIDATE: u32 = 0x0020;
const BIF_NEWDIALOGSTYLE: u32 = 0x0040;
const BFFM_INITIALIZED: u32 = 1;
const BFFM_SETSELECTIONW: u32 = 1127;

#[repr(C)]
struct BrowseInfoW {
    hwnd_owner: *mut c_void,
    pidl_root: *mut c_void,
    psz_display_name: *mut u16,
    lpsz_title: *const u16,
    ul_flags: u32,
    lpfn: Option<unsafe extern "system" fn(*mut c_void, u32, isize, isize) -> i32>,
    l_param: isize,
    i_image: i32,
}

#[link(name = "shell32")]
extern "system" {
    fn SHBrowseForFolderW(info: *mut BrowseInfoW) -> *mut c_void;
    fn SHGetPathFromIDListEx(pidl: *const c_void, path: *mut u16, chars: u32, opts: u32) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    fn OleInitialize(reserved: *mut c_void) -> i32;
    fn OleUninitialize();
    fn CoTaskMemFree(block: *mut c_void);
}

#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
    fn SendMessageW(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
}

struct OleGuard(bool);

impl Drop for OleGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { OleUninitialize() }
        }
    }
}

pub fn pick(title: &str, initial: &str) -> Option<PathBuf> {
    let started = unsafe { OleInitialize(std::ptr::null_mut()) };
    let ready = started >= 0;
    let _guard = OleGuard(ready);
    let tree = ready;
    let mut flags = BIF_RETURNONLYFSDIRS | BIF_EDITBOX | BIF_VALIDATE;
    if tree {
        flags |= BIF_NEWDIALOGSTYLE;
    }
    let instruction = format!("{title}。点一下文件夹把它选中，再点确定。");
    let title_wide = wide(&instruction);
    let initial_wide = initial_wide(initial);
    let mut display = [0u16; 260];
    let mut info = BrowseInfoW {
        hwnd_owner: unsafe { GetForegroundWindow() },
        pidl_root: std::ptr::null_mut(),
        psz_display_name: display.as_mut_ptr(),
        lpsz_title: title_wide.as_ptr(),
        ul_flags: flags,
        lpfn: Some(on_browse),
        l_param: initial_wide.as_ref().map(|item| item.as_ptr() as isize).unwrap_or(0),
        i_image: 0,
    };
    let pidl = unsafe { SHBrowseForFolderW(&mut info) };
    if pidl.is_null() {
        return None;
    }
    let mut path = vec![0u16; 32_768];
    let ok = unsafe { SHGetPathFromIDListEx(pidl, path.as_mut_ptr(), path.len() as u32, 0) };
    unsafe { CoTaskMemFree(pidl) };
    if ok == 0 {
        return None;
    }
    path_from_wide(&path)
}

pub fn path_from_wide(buf: &[u16]) -> Option<PathBuf> {
    let end = buf.iter().position(|unit| *unit == 0).unwrap_or(buf.len());
    if end == 0 {
        return None;
    }
    let path = PathBuf::from(OsString::from_wide(&buf[..end]));
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path)
    }
}

pub fn dialog_layout_ok() -> bool {
    use std::mem::{align_of, offset_of, size_of};
    size_of::<BrowseInfoW>() == 64
        && align_of::<BrowseInfoW>() == 8
        && offset_of!(BrowseInfoW, ul_flags) == 32
        && offset_of!(BrowseInfoW, lpfn) == 40
        && offset_of!(BrowseInfoW, l_param) == 48
        && offset_of!(BrowseInfoW, i_image) == 56
}

fn initial_wide(initial: &str) -> Option<Vec<u16>> {
    let initial = initial.trim();
    let path = Path::new(initial);
    if initial.is_empty() || !path.is_dir() {
        return None;
    }
    Some(wide(initial))
}

fn wide(text: &str) -> Vec<u16> {
    OsString::from(text).encode_wide().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn on_browse(hwnd: *mut c_void, msg: u32, _param: isize, data: isize) -> i32 {
    if msg == BFFM_INITIALIZED && data != 0 {
        unsafe {
            SendMessageW(hwnd, BFFM_SETSELECTIONW, 1, data);
        }
    }
    0
}
