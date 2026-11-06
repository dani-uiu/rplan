#[allow(
    non_snake_case,
    non_upper_case_globals,
    non_camel_case_types,
    dead_code,
    clippy::all
)]
pub mod Microsoft {
    pub mod Web {
        pub mod WebView2 {
            pub mod Win32 {
                mod windows_core {
                    // Looked up in WebView2Loader.dll when first called (see Cargo.toml).
                    macro_rules! link_webview2 {
                        ($library:literal $abi:literal fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty) => (
                            #[allow(non_snake_case)]
                            unsafe fn $name($($arg: $ty),*) -> $ret {
                                let p = crate::loader::proc(concat!(stringify!($name), "\0"));
                                if p.is_null() {
                                    // ERROR_MOD_NOT_FOUND
                                    return ::windows_core::HRESULT(0x8007007Eu32 as i32);
                                }
                                let f: unsafe extern "system" fn($($ty),*) -> $ret = unsafe { core::mem::transmute(p) };
                                unsafe { f($($arg),*) }
                            }
                        )
                    }

                    pub(crate) use ::windows_core::*;
                    pub(crate) use link_webview2 as link;
                }

                include!("bindings.rs");
            }
        }
    }
}

pub mod declared_interfaces;

mod loader {
    use core::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }
    /// The address of `name` (NUL-terminated) in WebView2Loader.dll, or null.
    pub fn proc(name: &str) -> *mut c_void {
        let dll: Vec<u16> = "WebView2Loader.dll\0".encode_utf16().collect();
        unsafe {
            let mut m = GetModuleHandleW(dll.as_ptr());
            if m.is_null() {
                m = LoadLibraryW(dll.as_ptr());
            }
            if m.is_null() {
                return core::ptr::null_mut();
            }
            GetProcAddress(m, name.as_ptr())
        }
    }
}

