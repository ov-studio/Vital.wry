#[macro_use]
mod macros;
mod godot_window;
mod keys;
mod protocols;

use godot::global::MouseButtonMask;
use godot::init::*;
use godot::prelude::*;
use godot::classes::{Control, DisplayServer, IControl, InputEvent, InputEventMouseButton, InputEventMouseMotion, InputEventKey, ProjectSettings, Viewport};
use godot::classes::display_server::WindowMode;
use godot::global::{Key, MouseButton};
use serde_json;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use wry::{WebViewBuilder, WebContext, Rect, PageLoadEvent};
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::http::Request;

use crate::godot_window::GodotWindow;
use crate::keys::{CURRENT_BUTTON_MASK, GODOT_KEYS};
use crate::protocols::get_res_response;

#[cfg(target_os = "windows")]
use {
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    windows::Win32::Foundation::HWND,
    windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GetWindowLongPtrA, SetWindowLongPtrA, SetWindowPos,
        GWL_STYLE, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE, SWP_NOACTIVATE,
    },
    wry::WebViewExtWindows,
};

#[cfg(target_os = "windows")]
#[link(name = "wevtapi")]
extern "system" {}

struct GodotWRY;

#[gdextension]
unsafe impl ExtensionLibrary for GodotWRY {}

#[derive(GodotClass)]
#[class(base=Control)]
struct WebView {
    base: Base<Control>,
    webview: Option<wry::WebView>,
    window_id: i32,
    previous_global_position: Vector2,
    previous_viewport_size: Vector2i,
    previous_window_position: Vector2i,
    previous_content_scale_factor: f32,
    #[export]
    #[var(get = get_window_z_index, set = set_window_z_index)]
    window_z_index: i32,
    #[export]
    full_window_size: bool,
    #[export]
    url: GString,
    #[export]
    html: GString,
    #[export]
    transparent: bool,
    #[export]
    background_color: Color,
    #[export]
    devtools: bool,
    #[export]
    headers: VarDictionary,
    #[export]
    user_agent: GString,
    #[export]
    zoom_hotkeys: bool,
    #[export]
    clipboard: bool,
    #[export]
    incognito: bool,
    #[export]
    focused_when_created: bool,
    #[export]
    autoplay: bool,
    #[export]
    overlay: bool,
    webview_hwnd: Option<isize>,
    // Set when build_webview() bails out because the window is minimized
    // or the native webview controller failed to construct. While true,
    // update_webview() retries creation every frame the window is no
    // longer minimized, instead of leaving the node permanently broken.
    webview_creation_pending: bool,
    // Prevents re-logging the same construction failure every retry frame.
    webview_creation_failed_logged: bool,
    // set_visible() used to write straight through to the native webview
    // and silently no-op if self.webview was still None (e.g. creation
    // deferred because the window was minimized). That dropped the
    // hidden request entirely, so once creation succeeded later the
    // webview came up visible by default -- fullscreen, on top, and
    // eating all input. This tracks the last requested state so it can
    // be (re)applied the moment the native webview actually exists.
    desired_visible: bool,
    // If load_url()/load_html() is called before self.webview exists yet
    // (creation deferred while minimized), the call used to just vanish --
    // there was nothing to apply it to and nothing tracking it for later.
    // This stores whichever one was last requested so build_webview() can
    // replay it the moment the native webview actually gets constructed.
    pending_load: Option<PendingLoad>,
}

enum PendingLoad {
    Url(String),
    Html(String),
}

#[godot_api]
impl IControl for WebView {
    fn init(base: Base<Control>) -> Self {
        Self {
            base,
            webview: None,
            window_id: 0,
            previous_global_position: Vector2::default(),
            previous_viewport_size: Vector2i::default(),
            previous_window_position: Vector2i::default(),
            previous_content_scale_factor: 1.0,
            window_z_index: 0,
            full_window_size: true,
            url: "".into(),
            html: "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>html,body{margin:0;padding:0;background:transparent}</style></head><body></body></html>".into(),
            transparent: false,
            background_color: Color::from_rgb(1.0, 1.0, 1.0),
            devtools: true,
            headers: VarDictionary::new(),
            user_agent: "".into(),
            zoom_hotkeys: false,
            clipboard: true,
            incognito: false,
            focused_when_created: true,
            autoplay: false,
            overlay: false,
            webview_hwnd: None,
            webview_creation_pending: false,
            webview_creation_failed_logged: false,
            desired_visible: true,
            pending_load: None,
        }
    }

    fn ready(&mut self) {
        self.create_webview();
    }

    fn enter_tree(&mut self) {
        if self.webview.is_some() {
            if let Some(gd_window) = self.base().get_window() {
                let current_window_id = gd_window.get_window_id();
                if current_window_id != self.window_id {
                    self.reparent_webview(current_window_id);
                }
            }
        }
    }

    fn process(&mut self, _delta: f64) {
        self.update_webview();
    }

    fn input(&mut self, event: Gd<InputEvent>) {
        if self.webview.is_none() || self.full_window_size {
            return;
        }

        if let Ok(mouse_event) = event.try_cast::<InputEventMouseButton>() {
            if mouse_event.is_pressed() {
                let mouse_pos = self.base().get_global_mouse_position();
                let rect = self.base().get_global_rect();

                if !rect.contains_point(mouse_pos) {
                    self.base_mut().call_deferred("focus_parent", &[]);
                }
            }
        }
    }
}

#[godot_api]
impl WebView {
    #[signal]
    fn ipc_message(message: GString);

    #[signal]
    fn page_load_started(message: GString);

    #[signal]
    fn page_load_finished(message: GString);

    fn update_webview(&mut self) {
        if self.webview.is_none() {
            if self.webview_creation_pending {
                let window_mode = DisplayServer::singleton()
                    .window_get_mode_ex()
                    .window_id(self.window_id)
                    .done();
                if window_mode != WindowMode::MINIMIZED {
                    debug_print!("[Godot WRY] update_webview(): retrying creation, window_id={} mode={:?}", self.window_id, window_mode);
                    self.create_webview();
                }
            }
            return;
        }

        let viewport_size = self.base().get_window()
            .map(|w| w.get_size())
            .unwrap_or_else(|| {
                self.base().get_tree()
                    .get_root().expect("Could not get viewport").get_size()
            });
        let window_position = DisplayServer::singleton().window_get_position_ex().window_id(self.window_id).done();
        let content_scale_factor = self.base().get_window()
            .map(|w| w.get_content_scale_factor())
            .unwrap_or(1.0);

        let needs_resize = self.base().get_global_position() != self.previous_global_position
            || viewport_size != self.previous_viewport_size
            || window_position != self.previous_window_position
            || content_scale_factor != self.previous_content_scale_factor;

        if needs_resize {
            self.previous_global_position = self.base().get_global_position();
            self.previous_viewport_size = viewport_size;
            self.previous_window_position = window_position;
            self.previous_content_scale_factor = content_scale_factor;
            self.resize();
        }

        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }

    fn build_webview(&mut self) {
        let display_server = DisplayServer::singleton();
        if display_server.get_name() == "headless"
        {
            godot_warn!("Godot WRY: Headless mode detected. webview will not be created.");
            return;
        }

        #[cfg(target_os = "linux")]
        gtk::init().expect("Failed to initialize GTK");

        let window_id = self.base().get_window()
            .map(|w| w.get_window_id())
            .unwrap_or(0);
        self.window_id = window_id;

        // WebView2 (and other backends) can't construct a controller against
        // a minimized window -- the client rect is 0x0 and creation fails
        // with E_INVALIDARG. Rather than attempt it (and previously panic),
        // bail out quietly and let update_webview() retry once the window
        // is restored/maximized.
        let window_mode = display_server.window_get_mode_ex().window_id(window_id).done();
        if window_mode == WindowMode::MINIMIZED {
            debug_print!("[Godot WRY] build_webview(): window_id={} is minimized (mode={:?}), deferring creation", window_id, window_mode);
            self.webview_creation_pending = true;
            return;
        }

        let window = GodotWindow::new(window_id);

        #[cfg(target_os = "windows")]
        {
            let handle = window.window_handle().unwrap().as_raw();
            let raw_handle: HWND = match handle {
                RawWindowHandle::Win32(win32) => HWND(win32.hwnd.get() as _),
                _ => {
                    panic!("Unsupported window handle type");
                }
            };

            unsafe {
                let current_style = GetWindowLongPtrA(raw_handle, GWL_STYLE);
                SetWindowLongPtrA(raw_handle, GWL_STYLE, current_style & !0x02000000);
            };
        }

        let base = Arc::new(Mutex::new(self.base().clone()));

        // WebView2's user data folder used to be left unset, which makes the
        // WebView2 loader fall back to creating a "<exe_name>.WebView2"
        // folder right next to the executable. Pin it under the project's
        // user:// directory instead (user://webview) so it lands in a
        // predictable, writable spot regardless of where the game is installed.
        let project_settings = ProjectSettings::singleton();
        let base_path = project_settings.globalize_path("user://").to_string();
        let mut resolved_data_directory = PathBuf::from(base_path);
        resolved_data_directory.push("webview");
        std::fs::create_dir_all(&resolved_data_directory).ok();

        let mut context = WebContext::new(Some(resolved_data_directory));
        let mut webview_builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_transparent(self.transparent)
            .with_devtools(self.devtools)
            .with_user_agent(String::from(&self.user_agent))
            .with_hotkeys_zoom(self.zoom_hotkeys)
            .with_clipboard(self.clipboard)
            .with_incognito(self.incognito)
            .with_focused(self.focused_when_created)
            .with_autoplay(self.autoplay)
            .with_accept_first_mouse(true);

        if self.html.is_empty() {
            webview_builder = webview_builder.with_url(String::from(&self.url));
        }
        if self.url.is_empty() {
            webview_builder = webview_builder.with_html(String::from(&self.html));
        }

        let webview_builder = webview_builder
            .with_ipc_handler({
                let base = Arc::clone(&base);
                move |req: Request<String>| {
                    let mut base = base.lock().unwrap();
                    let body = req.body().as_str();
                    
                    if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(body) {
                        if let Some(event_type) = json_value.get("type").and_then(|t| t.as_str()) {
                            let global_pos = base.get_global_position();

                            let x = json_value.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                            let y = json_value.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                            let vp_x = global_pos.x + x;
                            let vp_y = global_pos.y + y;

                            match event_type {
                                "_mouse_move" => {
                                    let movement_x = json_value.get("movementX").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                                    let movement_y = json_value.get("movementY").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                                    
                                    let mut event = InputEventMouseMotion::new_gd();
                                    event.set_position(Vector2::new(vp_x, vp_y));
                                    event.set_global_position(Vector2::new(vp_x, vp_y));
                                    
                                    let button_mask = CURRENT_BUTTON_MASK.lock().unwrap();
                                    event.set_button_mask(*button_mask);

                                    event.set_relative(Vector2::new(movement_x, movement_y));
                                    
                                    if let Some(mut viewport) = base.get_viewport() {
                                        viewport.push_input(&event);
                                    }
                                    return;
                                },
                                
                                "_mouse_down" | "_mouse_up" => {
                                    let button = json_value.get("button").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                    
                                    let godot_button = match button {
                                        0 => MouseButton::LEFT,
                                        1 => MouseButton::MIDDLE,
                                        2 => MouseButton::RIGHT,
                                        3 => MouseButton::WHEEL_UP,
                                        4 => MouseButton::WHEEL_DOWN,
                                        _ => MouseButton::LEFT,
                                    };
                                    
                                    let pressed = event_type == "_mouse_down";
                                    let mask = match godot_button {
                                        MouseButton::LEFT => MouseButtonMask::LEFT,
                                        MouseButton::RIGHT => MouseButtonMask::RIGHT,
                                        MouseButton::MIDDLE => MouseButtonMask::MIDDLE,
                                        _ => MouseButtonMask::default(),
                                    };
                                    
                                    if godot_button != MouseButton::WHEEL_UP && godot_button != MouseButton::WHEEL_DOWN {
                                        let mut button_mask = CURRENT_BUTTON_MASK.lock().unwrap();
                                        if pressed {
                                            *button_mask = *button_mask | mask;
                                        } else {
                                            match godot_button {
                                                MouseButton::LEFT => {
                                                    if button_mask.is_set(MouseButtonMask::LEFT) {
                                                        *button_mask = MouseButtonMask::from_ord(button_mask.ord() & !MouseButtonMask::LEFT.ord());
                                                    }
                                                },
                                                MouseButton::RIGHT => {
                                                    if button_mask.is_set(MouseButtonMask::RIGHT) {
                                                        *button_mask = MouseButtonMask::from_ord(button_mask.ord() & !MouseButtonMask::RIGHT.ord());
                                                    }
                                                },
                                                MouseButton::MIDDLE => {
                                                    if button_mask.is_set(MouseButtonMask::MIDDLE) {
                                                        *button_mask = MouseButtonMask::from_ord(button_mask.ord() & !MouseButtonMask::MIDDLE.ord());
                                                    }
                                                },
                                                _ => {}
                                            }
                                        }
                                    }
                                    
                                    let mut event = InputEventMouseButton::new_gd();
                                    event.set_button_index(godot_button);
                                    event.set_position(Vector2::new(vp_x, vp_y));
                                    event.set_global_position(Vector2::new(vp_x, vp_y));
                                    event.set_pressed(pressed);
                                    
                                    let button_mask = CURRENT_BUTTON_MASK.lock().unwrap();
                                    event.set_button_mask(*button_mask);
                                    
                                    if let Some(mut viewport) = base.get_viewport() {
                                        viewport.push_input(&event);
                                    }
                                    return;
                                },

                                "_mouse_wheel" => {
                                    let delta_x = json_value.get("deltaX").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                                    let delta_y = json_value.get("deltaY").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;

                                    let position = Vector2::new(vp_x, vp_y);
                                    let button_mask = *CURRENT_BUTTON_MASK.lock().unwrap();
                                    let modifiers = (
                                        json_value.get("shift").and_then(|v| v.as_bool()).unwrap_or(false),
                                        json_value.get("ctrl").and_then(|v| v.as_bool()).unwrap_or(false),
                                        json_value.get("alt").and_then(|v| v.as_bool()).unwrap_or(false),
                                        json_value.get("meta").and_then(|v| v.as_bool()).unwrap_or(false),
                                    );

                                    let viewport = base.get_viewport();

                                    if delta_y != 0.0 {
                                        let button = if delta_y < 0.0 { MouseButton::WHEEL_UP } else { MouseButton::WHEEL_DOWN };
                                        let factor = (delta_y.abs() / 100.0).max(1.0);
                                        send_wheel_event(button, position, factor, button_mask, modifiers, &viewport);
                                    }

                                    if delta_x != 0.0 {
                                        let button = if delta_x < 0.0 { MouseButton::WHEEL_LEFT } else { MouseButton::WHEEL_RIGHT };
                                        let factor = (delta_x.abs() / 100.0).max(1.0);
                                        send_wheel_event(button, position, factor, button_mask, modifiers, &viewport);
                                    }

                                    return;
                                },

                                "_key_down" | "_key_up" => {
                                    let key_str = json_value.get("key").and_then(|v| v.as_str()).unwrap_or("");
                                    let mut event = InputEventKey::new_gd();
                                    
                                    let godot_key = GODOT_KEYS.get(key_str).copied().unwrap_or(Key::NONE);
                                    
                                    event.set_keycode(godot_key);
                                    event.set_pressed(event_type == "_key_down");
                                    event.set_shift_pressed(json_value.get("shift").and_then(|v| v.as_bool()).unwrap_or(false));
                                    event.set_ctrl_pressed(json_value.get("ctrl").and_then(|v| v.as_bool()).unwrap_or(false));
                                    event.set_alt_pressed(json_value.get("alt").and_then(|v| v.as_bool()).unwrap_or(false));
                                    event.set_meta_pressed(json_value.get("meta").and_then(|v| v.as_bool()).unwrap_or(false));
                                    
                                    if let Some(mut viewport) = base.get_viewport() {
                                        viewport.push_input(&event);
                                    }
                                    return;
                                },
                                
                                _ => {}
                            }
                        }
                    }
                    
                    base.call_deferred("emit_signal", &["ipc_message".to_variant(), body.to_variant()]); 
                }
            })
            .with_on_page_load_handler({
                let base = Arc::clone(&base);
                move | event: PageLoadEvent, url: String | {
                    let mut base = base.lock().unwrap();

                    match event {
                        PageLoadEvent::Started => base.call_deferred("emit_signal", &["page_load_started".to_variant(), url.to_variant()]),
                        PageLoadEvent::Finished => base.call_deferred("emit_signal", &["page_load_finished".to_variant(), url.to_variant()]),
                    };
                }
            })
            .with_custom_protocol(
                "res".into(), move |_webview_id, request| get_res_response(request),
            );

        if !self.url.is_empty() && !self.html.is_empty() {
            godot_error!("[Godot WRY] You have entered both a URL and HTML code. You may only enter one at a time.")
        }

        let webview = match webview_builder.build_as_child(&window) {
            Ok(webview) => webview,
            Err(e) => {
                // Fail completely and cleanly -- nothing below this point has
                // run yet, so no HWND styles/overlay flags have been touched
                // and no input is left silently blocked. Just mark pending
                // so update_webview() retries next frame the window isn't
                // minimized, instead of leaving a half-initialized node.
                if !self.webview_creation_failed_logged {
                    godot_warn!("[Godot WRY] Failed to create webview, will retry: {:?}", e);
                    self.webview_creation_failed_logged = true;
                }
                self.webview_creation_pending = true;
                return;
            }
        };

        self.webview_creation_pending = false;
        self.webview_creation_failed_logged = false;
        debug_print!("[Godot WRY] build_webview(): native webview constructed for window_id={}", window_id);

        #[cfg(target_os = "windows")]
        {
            self.webview_hwnd = Some(webview.hwnd().0 as isize);

            if self.overlay {
                use windows::Win32::UI::WindowsAndMessaging::{GWL_STYLE, WS_DISABLED};

                unsafe extern "system" fn disable_child(
                    hwnd: HWND,
                    _lparam: windows::Win32::Foundation::LPARAM,
                ) -> windows::core::BOOL {
                    use windows::Win32::UI::WindowsAndMessaging::{GWL_STYLE, WS_DISABLED};
                    let style = GetWindowLongPtrA(hwnd, GWL_STYLE);
                    SetWindowLongPtrA(hwnd, GWL_STYLE, style | WS_DISABLED.0 as isize);
                    windows::core::BOOL(1)
                }

                let root_hwnd = HWND(self.webview_hwnd.unwrap() as _);
                unsafe {
                    let style = GetWindowLongPtrA(root_hwnd, GWL_STYLE);
                    SetWindowLongPtrA(root_hwnd, GWL_STYLE, style | WS_DISABLED.0 as isize);
                    let _ = EnumChildWindows(Some(root_hwnd), Some(disable_child), windows::Win32::Foundation::LPARAM(0));
                }
            }
        }

        self.webview.replace(webview);
        // Don't trust desired_visible here -- if creation was deferred
        // (window was minimized), any set_visible()/hide()/show() call
        // that happened in the meantime fired "visibility_changed" before
        // create_webview() below ever connected to it, so that signal is
        // permanently lost. is_visible_in_tree() reflects the CURRENT,
        // engine-tracked state regardless of when it last changed, so
        // sync from that instead the moment the webview actually exists.
        let should_be_visible = self.base().is_visible_in_tree();
        if let Some(webview) = &self.webview {
            match webview.set_visible(should_be_visible) {
                Ok(_) => debug_print!("[Godot WRY] build_webview(): synced visibility={} from Control.is_visible_in_tree()", should_be_visible),
                Err(e) => godot_warn!("[Godot WRY] build_webview(): failed to sync visibility={}: {e}", should_be_visible),
            }

            // WebView2's controller construction can silently steal OS
            // input focus for its own HWND, even when we just set it
            // invisible above -- hiding a window doesn't hand focus back.
            // Without this, the game window is left unfocused until the
            // user manually alt-tabs away and back. If the webview isn't
            // supposed to be visible/focused right now, explicitly return
            // focus to the parent (game) window immediately.
            if !should_be_visible || !self.focused_when_created {
                // Defer to avoid reentrant bind_mut() panic: build_webview() holds &mut self,
                // and focus_parent() can trigger a Godot callback that tries to borrow self again.
                self.base_mut().call_deferred("focus_parent", &[]);
                debug_print!("[Godot WRY] build_webview(): deferred OS focus return to parent window (should_be_visible={})", should_be_visible);
            }
        }
        self.resize();
        self.apply_z_order();

        // Replay a load_url()/load_html() that arrived while construction
        // was still pending -- otherwise the webview comes up visible but
        // stuck on the default blank page forever, since that call was
        // silently dropped rather than queued.
        if let Some(pending) = self.pending_load.take() {
            if let Some(webview) = &self.webview {
                match pending {
                    PendingLoad::Url(url) => {
                        debug_print!("[Godot WRY] build_webview(): replaying deferred load_url({})", url);
                        let _ = webview.load_url(&url);
                    }
                    PendingLoad::Html(html) => {
                        debug_print!("[Godot WRY] build_webview(): replaying deferred load_html(...)");
                        let _ = webview.load_html(&html);
                    }
                }
            }
        }
    }

    fn create_webview(&mut self) {
        self.build_webview();
        if self.webview.is_none() {
            debug_print!("[Godot WRY] create_webview(): still no webview after build_webview() (pending={})", self.webview_creation_pending);
            return;
        }

        debug_print!("[Godot WRY] create_webview(): webview exists, wiring resize/visibility signals");
        let mut viewport = self.base().get_tree().get_root().expect("Could not get viewport");
        viewport.connect("size_changed", &Callable::from_object_method(&*self.base(), "resize"));

        self.base().clone().connect("resized", &Callable::from_object_method(&*self.base(), "resize"));
        self.base().clone().connect("visibility_changed", &Callable::from_object_method(&*self.base(), "update_visibility"));
    }

    fn reparent_webview(&mut self, new_window_id: i32) {
        if self.webview.is_none() { return; }

        #[cfg(target_os = "windows")]
        {
            let window = GodotWindow::new(new_window_id);
            if let Ok(wh) = window.window_handle() {
                if let RawWindowHandle::Win32(win32) = wh.as_raw() {
                    let hwnd = win32.hwnd.get() as isize;

                    unsafe {
                        let raw_hwnd = HWND(hwnd as _);
                        let current_style = GetWindowLongPtrA(raw_hwnd, GWL_STYLE);
                        SetWindowLongPtrA(raw_hwnd, GWL_STYLE, current_style & !0x02000000);
                    };

                    if self.webview.as_ref().unwrap().reparent(hwnd).is_ok() {
                        self.window_id = new_window_id;
                        self.resize();
                        return;
                    }
                }
            }
            godot_warn!("[Godot WRY] Native reparent failed, falling back to rebuild");
        }

        self.webview.take();
        self.build_webview();
    }

    #[func]
    fn post_message(&self, message: GString) {
        if let Some(webview) = &self.webview {
            let data = serde_json::json!({ "detail": String::from(message) });
            let script = format!("document.dispatchEvent(new CustomEvent('message', {}))", data);
            let _ = webview.evaluate_script(&script);
        }
    }

    #[func]
    fn resize(&self) {
        if let Some(webview) = &self.webview {
            let rect = if self.full_window_size {
                let window_size = self.base().get_window()
                    .map(|w| w.get_size())
                    .unwrap_or_else(|| {
                        self.base().get_tree()
                            .get_root().expect("Could not get viewport").get_size()
                    });
                Rect {
                    position: PhysicalPosition::new(0, 0).into(),
                    size: PhysicalSize::new(window_size.x, window_size.y).into(),
                }
            } else {
                let pos = self.base().get_global_position();
                let size = self.base().get_size();
                let (scale_x, scale_y) = self.get_content_scale();
                let phys_x = (pos.x * scale_x).round();
                let phys_y = (pos.y * scale_y).round();
                Rect {
                    position: PhysicalPosition::new(phys_x, phys_y).into(),
                    size: PhysicalSize::new(size.x * scale_x, size.y * scale_y).into(),
                }
            };
            let _ = webview.set_bounds(rect);
        }
    }

    fn get_content_scale(&self) -> (f32, f32) {
        if let Some(window) = self.base().get_window() {
            let window_size = window.get_size();
            if let Some(viewport) = self.base().get_viewport() {
                let vp_size = viewport.get_visible_rect().size;
                if vp_size.x > 0.0 && vp_size.y > 0.0 {
                    return (
                        window_size.x as f32 / vp_size.x,
                        window_size.y as f32 / vp_size.y,
                    );
                }
            }
        }
        (1.0, 1.0)
    }

    #[func]
    fn eval(&self, script: GString) {
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script(&*String::from(script));
        }
    }

    #[func]
    fn update_visibility(&self) {
        if let Some(webview) = &self.webview {
            let visibility = self.base().is_visible_in_tree();
            match webview.set_visible(visibility) {
                Ok(_) => {
                    debug_print!("[Godot WRY] update_visibility(): visibility_changed fired, synced to {}", visibility);
                    if !visibility {
                        // Defer to avoid reentrant bind_mut() panic when
                        // focus_parent() triggers a Godot callback during
                        // visibility_changed signal dispatch.
                        self.base().clone().call_deferred("focus_parent", &[]);
                    }
                    self.resize();
                }
                Err(e) => {
                    godot_warn!("[Godot WRY] Could not set webview visibility: {e}. \
                        If you are using Window.hide()/show(), reparent the WebView \
                        node out of the Window before hide() and back after show() \
                        so the native handle can survive the window destruction.");
                }
            }
        }
    }

    #[func]
    fn set_visible(&mut self, visibility: bool) {
        // Always record intent, even if the native webview doesn't exist
        // yet (creation pending/minimized) -- it gets applied as soon as
        // build_webview() finishes constructing it.
        self.desired_visible = visibility;
        if let Some(webview) = &self.webview {
            match webview.set_visible(visibility) {
                Ok(_) => debug_print!("[Godot WRY] set_visible({}) applied immediately (webview exists)", visibility),
                Err(e) => godot_warn!("[Godot WRY] set_visible({}) failed on existing webview: {e}", visibility),
            }
        } else {
            debug_print!("[Godot WRY] set_visible({}) recorded as desired_visible, but no webview exists yet -- will apply once constructed", visibility);
        }
    }

    // client.lua toggles via `set_visible(not is_visible())`. This must
    // mirror desired_visible (not Godot's own Control::is_visible(), which
    // set_visible() above never touches) or the toggle desyncs from the
    // webview's actual shown/hidden state.
    #[func]
    fn is_visible(&self) -> bool {
        self.desired_visible
    }

    #[func]
    fn load_html(&mut self, html: GString) {
        let html_str = String::from(html);
        if let Some(webview) = &self.webview {
            let _ = webview.load_html(&html_str);
        } else {
            debug_print!("[Godot WRY] load_html() called before webview exists -- deferring until construction completes");
            self.pending_load = Some(PendingLoad::Html(html_str));
        }
    }

    #[func]
    fn load_url(&mut self, url: GString) {
        let mut url_str = String::from(url);

        if let Some(stripped) = url_str.strip_prefix("res://") {
            let path = stripped.replace("\\", "/");
            
            #[cfg(target_os = "linux")]
            {
                url_str = format!("res://{}", path);
            }

            #[cfg(not(target_os = "linux"))]
            {
                url_str = format!("http://res.{}", path);
            }
        }

        if let Some(webview) = &self.webview {
            let _ = webview.load_url(&url_str);
        } else {
            debug_print!("[Godot WRY] load_url() called before webview exists -- deferring until construction completes");
            self.pending_load = Some(PendingLoad::Url(url_str));
        }
    }

    #[func]
    fn clear_all_browsing_data(&self) {
        if let Some(webview) = &self.webview {
            let _ = webview.clear_all_browsing_data();
        }
    }

    #[func]
    fn close_devtools(&self) {
        if let Some(webview) = &self.webview {
            let _ = webview.close_devtools();
        }
    }

    #[func]
    fn open_devtools(&self) {
        if let Some(webview) = &self.webview {
            let _ = webview.open_devtools();
        }
    }

    #[func]
    fn is_devtools_open(&self) -> bool {
        if let Some(webview) = &self.webview {
            return webview.is_devtools_open();
        }
        false
    }

    #[func]
    fn focus(&self) {
        if let Some(webview) = &self.webview {
            let _ = webview.focus();
        }
    }

    #[func]
    fn focus_parent(&self) {
        if let Some(webview) = &self.webview {
            let _ = webview.focus_parent();
        }
    }

    #[func]
    fn print(&self) {
        if let Some(webview) = &self.webview {
            let _ = webview.print();
        }
    }

    #[func]
    fn reload(&self) {
        if let Some(webview) = &self.webview {
            let _ = webview.reload();
        }
    }

    #[func]
    fn zoom(&self, scale_factor: f64) {
        if let Some(webview) = &self.webview {
            let _ = webview.zoom(scale_factor);
        }
    }

    #[func]
    fn get_window_z_index(&self) -> i32 {
        self.window_z_index
    }

    #[func]
    fn set_window_z_index(&mut self, value: i32) {
        self.window_z_index = value;
        self.apply_z_order();
    }

    fn get_sibling_webviews(&self) -> Vec<Gd<WebView>> {
        let mut siblings: Vec<Gd<WebView>> = Vec::new();
        if let Some(tree) = self.base().get_tree_or_null() {
            if let Some(root) = tree.get_root() {
                let all_webviews = root.find_children_ex("*").type_("WebView").owned(false).done();
                for i in 0..all_webviews.len() {
                    let node = all_webviews.get(i);
                    if let Some(node) = node {
                    if let Ok(wv) = node.try_cast::<WebView>() {
                        if wv.instance_id() != self.base().instance_id() {
                            siblings.push(wv);
                        }
                    }
                    }
                }
            }
        }
        siblings.sort_by_key(|wv| wv.bind().window_z_index);
        siblings
    }

    fn apply_z_order(&self) {
        let mut all: Vec<(i32, Option<Gd<WebView>>)> = Vec::new();

        for wv in self.get_sibling_webviews() {
            let z = wv.bind().window_z_index;
            all.push((z, Some(wv)));
        }
        all.push((self.window_z_index, None));
        all.sort_by_key(|(z, _)| *z);

        #[cfg(target_os = "windows")]
        {
            for (_, maybe_wv) in &all {
                let hwnd_opt: Option<HWND> = if let Some(wv) = maybe_wv {
                    wv.bind().webview_hwnd.map(|h| HWND(h as _))
                } else {
                    self.webview_hwnd.map(|h| HWND(h as _))
                };

                if let Some(hwnd) = hwnd_opt {
                    unsafe {
                        let _ = SetWindowPos(
                            hwnd,
                            Some(HWND_TOP),
                            0, 0, 0, 0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = all;
        }
    }
}

fn send_wheel_event(
    button: MouseButton,
    position: Vector2,
    factor: f32,
    button_mask: MouseButtonMask,
    modifiers: (bool, bool, bool, bool),
    viewport: &Option<Gd<Viewport>>,
) {
    let (shift, ctrl, alt, meta) = modifiers;
    for pressed in [true, false] {
        let mut event = InputEventMouseButton::new_gd();
        event.set_button_index(button);
        event.set_position(position);
        event.set_global_position(position);
        event.set_pressed(pressed);
        event.set_factor(factor);
        event.set_button_mask(button_mask);
        event.set_shift_pressed(shift);
        event.set_ctrl_pressed(ctrl);
        event.set_alt_pressed(alt);
        event.set_meta_pressed(meta);
        if let Some(vp) = viewport {
            vp.clone().push_input(&event);
        }
    }
}