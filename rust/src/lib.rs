#[macro_use]
mod macros;
mod godot_window;
mod protocols;

use godot::global::MouseButtonMask;
use godot::init::*;
use godot::prelude::*;
use godot::classes::{Control, DisplayServer, IControl, InputEvent, InputEventMouseButton, InputEventMouseMotion, InputEventKey, ProjectSettings, Viewport};
use godot::global::{Key, MouseButton};
use lazy_static::lazy_static;
use serde_json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use wry::{WebViewBuilder, WebContext, Rect, PageLoadEvent};
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::http::Request;

use crate::godot_window::GodotWindow;
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
    data_directory: GString,
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
            data_directory: "user://".into(),
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
                    if let Some(webview) = &self.webview {
                        let _ = webview.focus_parent();
                    }
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
        let resolved_data_directory: Option<PathBuf> = if !self.data_directory.is_empty() {
            let data_directory = self.data_directory.to_string();

            if data_directory.starts_with("user://") {
                let path_without_prefix = data_directory.trim_start_matches("user://");

                let project_settings = ProjectSettings::singleton();
                let base_path = project_settings.globalize_path("user://").to_string();
                let mut absolute_path = PathBuf::from(base_path);
                absolute_path.push(path_without_prefix);

                std::fs::create_dir_all(&absolute_path).ok();

                Some(absolute_path)
            } else {
                let path = PathBuf::from(&data_directory);
                std::fs::create_dir_all(&path).ok();
                Some(path)
            }
        } else {
            None
        };
        let mut context = WebContext::new(resolved_data_directory);
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

        let webview = webview_builder.build_as_child(&window).unwrap();

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
        self.resize();
        self.apply_z_order();
    }

    fn create_webview(&mut self) {
        self.build_webview();
        if self.webview.is_none() {
            return;
        }

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
                Ok(_) => self.resize(),
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
    fn set_visible(&self, visibility: bool) {
        if let Some(webview) = &self.webview {
            let _ = webview.set_visible(visibility);
        }
    }

    #[func]
    fn load_html(&self, html: GString) {
        if let Some(webview) = &self.webview {
            let _ = webview.load_html(&*String::from(html));
        }
    }

    #[func]
    fn load_url(&self, url: GString) {
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

lazy_static! {
    static ref CURRENT_BUTTON_MASK: Mutex<MouseButtonMask> = Mutex::new(MouseButtonMask::default());

    static ref GODOT_KEYS: HashMap<&'static str, Key> = HashMap::from([

        ("a", Key::A),
        ("A", Key::A),
        ("b", Key::B),
        ("B", Key::B),
        ("c", Key::C),
        ("C", Key::C),
        ("d", Key::D),
        ("D", Key::D),
        ("e", Key::E),
        ("E", Key::E),
        ("f", Key::F),
        ("F", Key::F),
        ("g", Key::G),
        ("G", Key::G),
        ("h", Key::H),
        ("H", Key::H),
        ("i", Key::I),
        ("I", Key::I),
        ("j", Key::J),
        ("J", Key::J),
        ("k", Key::K),
        ("K", Key::K),
        ("l", Key::L),
        ("L", Key::L),
        ("m", Key::M),
        ("M", Key::M),
        ("n", Key::N),
        ("N", Key::N),
        ("o", Key::O),
        ("O", Key::O),
        ("p", Key::P),
        ("P", Key::P),
        ("q", Key::Q),
        ("Q", Key::Q),
        ("r", Key::R),
        ("R", Key::R),
        ("s", Key::S),
        ("S", Key::S),
        ("t", Key::T),
        ("T", Key::T),
        ("u", Key::U),
        ("U", Key::U),
        ("v", Key::V),
        ("V", Key::V),
        ("w", Key::W),
        ("W", Key::W),
        ("x", Key::X),
        ("X", Key::X),
        ("y", Key::Y),
        ("Y", Key::Y),
        ("z", Key::Z),
        ("Z", Key::Z),
        
        ("0", Key::KEY_0),
        ("1", Key::KEY_1),
        ("2", Key::KEY_2),
        ("3", Key::KEY_3),
        ("4", Key::KEY_4),
        ("5", Key::KEY_5),
        ("6", Key::KEY_6),
        ("7", Key::KEY_7),
        ("8", Key::KEY_8),
        ("9", Key::KEY_9),
        ("Numpad0", Key::KP_0),
        ("Numpad1", Key::KP_1),
        ("Numpad2", Key::KP_2),
        ("Numpad3", Key::KP_3),
        ("Numpad4", Key::KP_4),
        ("Numpad5", Key::KP_5),
        ("Numpad6", Key::KP_6),
        ("Numpad7", Key::KP_7),
        ("Numpad8", Key::KP_8),
        ("Numpad9", Key::KP_9),
        
        ("F1", Key::F1),
        ("F2", Key::F2),
        ("F3", Key::F3),
        ("F4", Key::F4),
        ("F5", Key::F5),
        ("F6", Key::F6),
        ("F7", Key::F7),
        ("F8", Key::F8),
        ("F9", Key::F9),
        ("F10", Key::F10),
        ("F11", Key::F11),
        ("F12", Key::F12),
        ("F13", Key::F13),
        ("F14", Key::F14),
        ("F15", Key::F15),
        ("F16", Key::F16),
        ("F17", Key::F17),
        ("F18", Key::F18),
        ("F19", Key::F19),
        ("F20", Key::F20),
        ("F21", Key::F21),
        ("F22", Key::F22),
        ("F23", Key::F23),
        ("F24", Key::F24),
        
        ("ArrowUp", Key::UP),
        ("ArrowDown", Key::DOWN),
        ("ArrowLeft", Key::LEFT),
        ("ArrowRight", Key::RIGHT),
        
        ("Enter", Key::ENTER),
        ("NumpadEnter", Key::KP_ENTER),
        ("Tab", Key::TAB),
        ("Space", Key::SPACE),
        (" ", Key::SPACE),
        ("Backspace", Key::BACKSPACE),
        ("Escape", Key::ESCAPE),
        ("CapsLock", Key::CAPSLOCK),
        ("ScrollLock", Key::SCROLLLOCK),
        ("NumLock", Key::NUMLOCK),
        ("PrintScreen", Key::PRINT),
        ("Pause", Key::PAUSE),
        ("Insert", Key::INSERT),
        ("Home", Key::HOME),
        ("PageUp", Key::PAGEUP),
        ("Delete", Key::DELETE),
        ("End", Key::END),
        ("PageDown", Key::PAGEDOWN),
        
        ("Shift", Key::SHIFT),
        ("Control", Key::CTRL),
        ("Alt", Key::ALT),
        ("AltGraph", Key::ALT),
        ("Meta", Key::META),
        ("ContextMenu", Key::MENU),
        
        ("NumpadMultiply", Key::KP_MULTIPLY),
        ("NumpadDivide", Key::KP_DIVIDE),
        ("NumpadAdd", Key::KP_ADD),
        ("NumpadSubtract", Key::KP_SUBTRACT),
        ("NumpadDecimal", Key::KP_PERIOD),
        
        ("MediaPlayPause", Key::MEDIAPLAY),
        ("MediaStop", Key::MEDIASTOP),
        ("MediaTrackNext", Key::MEDIANEXT),
        ("MediaTrackPrevious", Key::MEDIAPREVIOUS),
        ("VolumeDown", Key::VOLUMEDOWN),
        ("VolumeUp", Key::VOLUMEUP),
        ("VolumeMute", Key::VOLUMEMUTE),
        
        ("BrowserBack", Key::BACK),
        ("BrowserForward", Key::FORWARD),
        ("BrowserRefresh", Key::REFRESH),
        ("BrowserStop", Key::STOP),
        ("BrowserSearch", Key::SEARCH),
        ("BrowserHome", Key::HOMEPAGE),
        
        ("`", Key::QUOTELEFT),
        ("~", Key::ASCIITILDE),
        ("!", Key::EXCLAM),
        ("@", Key::AT),
        ("#", Key::NUMBERSIGN),
        ("$", Key::DOLLAR),
        ("%", Key::PERCENT),
        ("^", Key::ASCIICIRCUM),
        ("&", Key::AMPERSAND),
        ("*", Key::ASTERISK),
        ("(", Key::PARENLEFT),
        (")", Key::PARENRIGHT),
        ("-", Key::MINUS),
        ("_", Key::UNDERSCORE),
        ("=", Key::EQUAL),
        ("+", Key::PLUS),
        ("[", Key::BRACKETLEFT),
        ("{", Key::BRACELEFT),
        ("]", Key::BRACKETRIGHT),
        ("}", Key::BRACERIGHT),
        ("\\", Key::BACKSLASH),
        ("|", Key::BAR),
        (";", Key::SEMICOLON),
        (":", Key::COLON),
        ("'", Key::APOSTROPHE),
        ("\"", Key::QUOTEDBL),
        (",", Key::COMMA),
        ("<", Key::LESS),
        (".", Key::PERIOD),
        (">", Key::GREATER),
        ("/", Key::SLASH),
        ("?", Key::QUESTION),
    ]);
}