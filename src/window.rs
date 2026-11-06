//! The app's own window: a native window hosting the system web view
//! (WebView2 on Windows, WKWebView on macOS). No browser is started.

use std::path::{Path, PathBuf};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{NewWindowResponse, WebContext, WebViewBuilder};

enum UserEvent {
    Title(String),
}

fn data_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("HOME").map(|h| format!("{h}/Library/Application Support")))
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string());
    Path::new(&base).join("rplan").join("webview")
}

/// Microsoft's signed WebView2 loader, delay-loaded (see build.rs). Unpacking it and
/// loading it by full path first means the delay-load finds it already in memory.
#[cfg(windows)]
fn load_webview2(dir: &Path) -> Result<(), String> {
    #[cfg(target_arch = "aarch64")]
    const DLL: &[u8] = include_bytes!("../assets/webview2/arm64/WebView2Loader.dll");
    #[cfg(not(target_arch = "aarch64"))]
    const DLL: &[u8] = include_bytes!("../assets/webview2/x64/WebView2Loader.dll");
    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut std::ffi::c_void;
    }
    let f = dir.join("WebView2Loader.dll");
    if std::fs::read(&f).ok().as_deref() != Some(DLL) {
        // A copy may be in use by another open window; write beside it then.
        let tmp = dir.join(format!("WebView2Loader-{}.dll", std::process::id()));
        if std::fs::write(&f, DLL).is_err() {
            std::fs::write(&tmp, DLL).map_err(|e| e.to_string())?;
            return load_path(&tmp);
        }
    }
    return load_path(&f);
    fn load_path(f: &Path) -> Result<(), String> {
        use std::os::windows::ffi::OsStrExt;
        let w: Vec<u16> = f.as_os_str().encode_wide().chain([0]).collect();
        if unsafe { LoadLibraryW(w.as_ptr()) }.is_null() {
            return Err(format!("could not load {}", f.display()));
        }
        Ok(())
    }
}

#[cfg(windows)]
fn icon(dir: &Path) -> Option<tao::window::Icon> {
    use tao::platform::windows::IconExtWindows;
    let f = dir.join("rplan.ico");
    if std::fs::read(&f).ok().as_deref() != Some(crate::server::ICON_ICO) {
        std::fs::write(&f, crate::server::ICON_ICO).ok()?;
    }
    tao::window::Icon::from_path(&f, None).ok()
}
#[cfg(not(windows))]
fn icon(_: &Path) -> Option<tao::window::Icon> {
    None
}

/// Opens `url` in a native window and runs until the window is closed.
/// Returns an error (without having shown anything) if the web view can't start.
pub fn run(url: &str, title: &str, on_close: impl Fn() + 'static) -> Result<(), String> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(windows)]
    load_webview2(&dir)?;
    let window = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(tao::dpi::LogicalSize::new(1440.0, 900.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(720.0, 480.0))
        .with_window_icon(icon(&dir))
        .with_visible(false)
        .build(&event_loop)
        .map_err(|e| e.to_string())?;
    let mut ctx = WebContext::new(Some(dir));
    let origin = url.trim_end_matches('/').to_string();
    let own = origin.clone();
    let builder = WebViewBuilder::new_with_web_context(&mut ctx)
        .with_url(url)
        // Zoom is the app's own (View ▸ Zoom), so the web view's is turned off.
        .with_hotkeys_zoom(false)
        .with_devtools(cfg!(debug_assertions))
        .with_document_title_changed_handler(move |t| {
            let _ = proxy.send_event(UserEvent::Title(t));
        })
        // Links to other sites open in the normal browser, never inside the app.
        .with_navigation_handler(move |u| {
            if u.starts_with(&own) || u.starts_with("about:") || u.starts_with("data:") || u.starts_with("blob:") {
                return true;
            }
            crate::server::open_browser(&u);
            false
        })
        .with_new_window_req_handler(move |u, _| {
            if !u.starts_with(&origin) {
                crate::server::open_browser(&u);
            }
            NewWindowResponse::Deny
        });
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_browser_accelerator_keys(true)
    };
    let webview = builder.build(&window).map_err(|e| e.to_string())?;
    window.set_visible(true);
    let mut on_close = Some(on_close);
    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        let _keep = &webview;
        match event {
            Event::UserEvent(UserEvent::Title(t)) => window.set_title(&t),
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if let Some(f) = on_close.take() {
                    f();
                }
                *flow = ControlFlow::Exit;
            }
            _ => {}
        }
    })
}
