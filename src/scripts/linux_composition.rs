//! Transparent X11 children keep native input; Godot presents their RGBA pixels.
use godot::classes::canvas_item_material::BlendMode;
use godot::classes::control::MouseFilter;
use godot::classes::texture_rect::{ExpandMode, StretchMode};
use godot::classes::{CanvasItemMaterial, CanvasLayer, Image, ImageTexture, TextureRect};
use godot::prelude::*;
use gtk::prelude::*;
use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt, WebViewExt};
use wry::{WebViewBuilderExtUnix, WebViewExtUnix};

use crate::linux_snapshot::Source;

#[link(name = "Xcomposite")]
extern "C" {
    fn XCompositeQueryVersion(
        display: *mut std::ffi::c_void,
        major: *mut i32,
        minor: *mut i32,
    ) -> i32;
    fn XCompositeRedirectWindow(
        display: *mut std::ffi::c_void,
        window: std::ffi::c_ulong,
        update: i32,
    );
}

pub struct Composition {
    child: NativeChild,
    source: Source,
    view: webkit2gtk::WebView,
    pub canvas: Gd<CanvasLayer>,
    rect: Gd<TextureRect>,
    texture: Option<Gd<ImageTexture>>,
    size: Vector2i,
    visible: bool,
}

// An owned GTK window avoids WRY's foreign-GDK-window hide/show lifetime.
// It is an override-redirect X11 child, never a desktop toplevel.
struct NativeChild {
    window: gtk::Window,
    display: gdkx11::X11Display,
    xid: std::ffi::c_ulong,
    parent: std::ffi::c_ulong,
    xlib: x11_dl::xlib::Xlib,
    bounds: std::cell::Cell<Option<(i32, i32, i32, i32)>>,
    visible: std::cell::Cell<bool>,
}
impl NativeChild {
    fn new(parent: u64) -> Result<Self, String> {
        let display = gtk::gdk::Display::default()
            .ok_or("No GDK display")?
            .downcast::<gdkx11::X11Display>()
            .map_err(|_| "X11 required")?;
        let xlib = x11_dl::xlib::Xlib::open().map_err(|e| e.to_string())?;
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_decorated(false);
        window.realize();
        let gdk_window = window.window().ok_or("Missing GTK window")?;
        gdk_window.set_override_redirect(true);
        let xid = unsafe { gdkx11::ffi::gdk_x11_window_get_xid(gdk_window.as_ptr() as _) };
        let child = Self {
            window,
            display,
            xid,
            parent: parent as _,
            xlib,
            bounds: std::cell::Cell::new(None),
            visible: std::cell::Cell::new(false),
        };
        unsafe {
            let raw = child.raw();
            let (mut major, mut minor) = (0, 0);
            if XCompositeQueryVersion(raw as _, &mut major, &mut minor) == 0
                || (major == 0 && minor < 4)
            {
                return Err("Transparent composition requires XComposite >= 0.4".into());
            }
            gdkx11::ffi::gdk_x11_display_error_trap_push(child.display.as_ptr());
            (child.xlib.XReparentWindow)(raw, xid, parent as _, 0, 0);
            XCompositeRedirectWindow(raw as _, xid, 1);
            let error = gdkx11::ffi::gdk_x11_display_error_trap_pop(child.display.as_ptr());
            if error != 0 {
                return Err(format!("X11 child creation failed: {error}"));
            }
        }
        Ok(child)
    }
    fn raw(&self) -> *mut x11_dl::xlib::Display {
        unsafe { gdkx11::ffi::gdk_x11_display_get_xdisplay(self.display.as_ptr()) as _ }
    }
    fn configure(&self, position: Vector2, size: Vector2, scale: (f32, f32), visible: bool) {
        let width = (size.x * scale.0).round().max(1.0) as i32;
        let height = (size.y * scale.1).round().max(1.0) as i32;
        let bounds = (
            (position.x * scale.0).round() as i32,
            (position.y * scale.1).round() as i32,
            width,
            height,
        );
        if self.bounds.replace(Some(bounds)) != Some(bounds) {
            // GTK allocates in logical pixels; X11 positions in physical pixels.
            let gtk_scale = self.window.scale_factor().max(1);
            if let Some(window) = self.window.window() {
                window.resize(width / gtk_scale, height / gtk_scale);
            }
            self.window.size_allocate(&gtk::Allocation::new(
                0,
                0,
                (width / gtk_scale).max(1),
                (height / gtk_scale).max(1),
            ));
            unsafe {
                (self.xlib.XMoveResizeWindow)(
                    self.raw(),
                    self.xid,
                    bounds.0,
                    bounds.1,
                    width as u32,
                    height as u32,
                );
            }
        }
        if self.visible.replace(visible) != visible {
            if visible {
                self.window.show_all();
            } else {
                self.window.hide();
            }
        }
    }
}
impl Drop for NativeChild {
    fn drop(&mut self) {
        unsafe {
            self.window.destroy();
        }
    }
}

impl Composition {
    pub fn build(
        builder: wry::WebViewBuilder<'_>,
        parent: u64,
    ) -> Result<(wry::WebView, Self), String> {
        let child = NativeChild::new(parent)?;
        let webview = builder
            .build_gtk(&child.window)
            .map_err(|e| e.to_string())?;
        let view = webview.webview();
        // Snapshots use WebKit's software painter. This is a
        // CPU-copy path, independent of Godot's OpenGL or Vulkan renderer.
        if let Some(settings) = WebViewExt::settings(&view) {
            settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
        }
        let source = Source::new(&view, true);
        let mut canvas = CanvasLayer::new_alloc();
        canvas.set_visible(false);
        let mut rect = TextureRect::new_alloc();
        rect.set_mouse_filter(MouseFilter::IGNORE);
        rect.set_expand_mode(ExpandMode::IGNORE_SIZE);
        rect.set_stretch_mode(StretchMode::SCALE);
        let mut material = CanvasItemMaterial::new_gd();
        material.set_blend_mode(BlendMode::PREMULT_ALPHA);
        rect.set_material(&material);
        canvas.add_child(&rect);
        Ok((
            webview,
            Self {
                child,
                source,
                view,
                canvas,
                rect,
                texture: None,
                size: Vector2i::ZERO,
                visible: false,
            },
        ))
    }

    // WRY's build_gtk() manages widget focus. The owned X11 child additionally
    // needs OS focus routing to preserve focus()/focus_parent() semantics.
    pub fn focus(&self, parent: bool) {
        let child = &self.child;
        let xid = if parent { child.parent } else { child.xid };
        unsafe {
            gdkx11::ffi::gdk_x11_display_error_trap_push(child.display.as_ptr());
            let mut attributes = std::mem::zeroed();
            if (child.xlib.XGetWindowAttributes)(child.raw(), xid, &mut attributes) != 0
                && attributes.map_state == x11_dl::xlib::IsViewable
            {
                (child.xlib.XSetInputFocus)(
                    child.raw(),
                    xid,
                    x11_dl::xlib::RevertToParent,
                    x11_dl::xlib::CurrentTime,
                );
            }
            gdkx11::ffi::gdk_x11_display_error_trap_pop_ignored(child.display.as_ptr());
        }
    }

    pub fn update(
        &mut self,
        position: Vector2,
        size: Vector2,
        z: i32,
        visible: bool,
        scale: (f32, f32),
    ) {
        self.child.configure(position, size, scale, visible);
        if self.visible != visible {
            self.source.invalidate();
            self.visible = visible;
        }
        self.canvas.set_layer(z.saturating_add(1));
        self.rect.set_position(position);
        self.rect.set_size(size);
        self.canvas.set_visible(visible && self.texture.is_some());
        if !visible {
            return;
        }
        match self.source.capture(&self.view) {
            Ok(Some((width, height, rgba))) => {
                let size = Vector2i::new(width as i32, height as i32);
                let bytes = PackedByteArray::from(rgba.as_slice());
                if let Some(image) = Image::create_from_data(
                    size.x,
                    size.y,
                    false,
                    godot::classes::image::Format::RGBA8,
                    &bytes,
                ) {
                    if size == self.size {
                        if let Some(texture) = &mut self.texture {
                            texture.update(&image);
                        }
                    } else if let Some(texture) = ImageTexture::create_from_image(&image) {
                        self.rect.set_texture(&texture);
                        self.texture = Some(texture);
                        self.size = size;
                    }
                    self.canvas.set_visible(true);
                }
            }
            Ok(None) => {}
            Err(error) => godot_warn!("WebView snapshot: {}", error),
        }
    }
}

impl Drop for Composition {
    fn drop(&mut self) {
        self.source.dispose();
        if self.canvas.is_instance_valid() {
            self.canvas.queue_free();
        }
    }
}
