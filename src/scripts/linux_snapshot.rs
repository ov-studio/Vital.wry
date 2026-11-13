// Copyright 2026 DreamCity contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! One asynchronous WebKit CPU snapshot at a time; callbacks never touch Godot.
type Result<T> = std::result::Result<T, String>;
use gtk::{
    cairo::{Format, ImageSurface, Surface},
    gio::Cancellable,
    prelude::*,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};
use webkit2gtk::{LoadEvent, SnapshotOptions, SnapshotRegion, WebView, WebViewExt};
type Frame = (u32, u32, Vec<u8>);
struct State {
    alive: Cell<bool>,
    generation: Cell<u64>,
    serial: Cell<u64>,
    next_capture: Cell<Instant>,
    pending: RefCell<Option<(u64, Cancellable)>>,
    ready: RefCell<Option<Frame>>,
    error: RefCell<Option<String>>,
    size: Cell<(i32, i32)>,
}
impl State {
    fn invalidate(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        let pending = self.pending.borrow_mut().take();
        if let Some((_, cancel)) = pending {
            cancel.cancel();
        }
        self.ready.borrow_mut().take();
        self.error.borrow_mut().take();
        self.next_capture.set(Instant::now());
    }
}
pub struct Source {
    state: Rc<State>,
    transparent: bool,
}
impl Source {
    pub fn new(view: &WebView, transparent: bool) -> Self {
        let state = Rc::new(State {
            alive: Cell::new(true),
            generation: Cell::new(0),
            serial: Cell::new(0),
            next_capture: Cell::new(Instant::now()),
            pending: RefCell::new(None),
            ready: RefCell::new(None),
            error: RefCell::new(None),
            size: Cell::new((0, 0)),
        });
        let weak = Rc::downgrade(&state);
        view.connect_load_changed(move |_, event| {
            if let Some(s) = weak.upgrade() {
                match event {
                    LoadEvent::Started => s.invalidate(),
                    LoadEvent::Committed | LoadEvent::Finished => {
                        s.next_capture.set(Instant::now())
                    }
                    _ => {}
                }
            }
        });
        let weak = Rc::downgrade(&state);
        view.connect_size_allocate(move |_, rect| {
            if let Some(s) = weak.upgrade() {
                let size = (rect.width(), rect.height());
                if s.size.replace(size) != size {
                    s.invalidate();
                }
            }
        });
        let weak = Rc::downgrade(&state);
        view.connect_unmap(move |_| {
            if let Some(s) = weak.upgrade() {
                s.invalidate();
            }
        });
        Self { state, transparent }
    }
    pub fn invalidate(&self) {
        self.state.invalidate();
    }
    pub fn dispose(&self) {
        self.state.alive.set(false);
        self.state.invalidate();
    }
    pub fn capture(&self, view: &WebView) -> Result<Option<Frame>> {
        let s = &self.state;
        if let Some(error) = s.error.borrow_mut().take() {
            return Err(error);
        }
        let frame = s.ready.borrow_mut().take();
        // WebKit can repaint without a GTK draw signal. Poll visible content at
        // most 30 Hz, including static pages, with one async request in flight.
        if view.is_mapped()
            && s.alive.get()
            && Instant::now() >= s.next_capture.get()
            && s.pending.borrow().is_none()
        {
            let scale = i64::from(view.scale_factor().max(1));
            let width = i64::from(view.allocated_width()) * scale;
            let height = i64::from(view.allocated_height()) * scale;
            if width <= 0 || height <= 0 {
                return Ok(frame);
            }
            if width > 8192 || height > 8192 || width * height > 16_777_216 {
                return Err("WebKit snapshot exceeds 64 MiB".into());
            }
            let serial = s.serial.get().wrapping_add(1);
            s.serial.set(serial);
            let generation = s.generation.get();
            let cancel = Cancellable::new();
            *s.pending.borrow_mut() = Some((serial, cancel.clone()));
            s.next_capture
                .set(Instant::now() + Duration::from_millis(34));
            let weak = Rc::downgrade(s);
            let options = if self.transparent {
                SnapshotOptions::TRANSPARENT_BACKGROUND
            } else {
                SnapshotOptions::empty()
            };
            view.snapshot(
                SnapshotRegion::Visible,
                options,
                Some(&cancel),
                move |result| {
                    let Some(s) = weak.upgrade() else {
                        return;
                    };
                    if !s.alive.get()
                        || s.generation.get() != generation
                        || s.pending.borrow().as_ref().map(|p| p.0) != Some(serial)
                    {
                        return;
                    }
                    s.pending.borrow_mut().take();
                    match result.map_err(|e| e.to_string()).and_then(convert) {
                        Ok(frame) => {
                            *s.ready.borrow_mut() = Some(frame);
                        }
                        Err(error) => {
                            *s.error.borrow_mut() = Some(error);
                        }
                    }
                },
            );
        }
        Ok(frame)
    }
}
fn convert(surface: Surface) -> std::result::Result<Frame, String> {
    let image = ImageSurface::try_from(surface)
        .map_err(|_| "WebKit returned a non-image snapshot".to_owned())?;
    let (w, h, stride) = (image.width(), image.height(), image.stride());
    let format = image.format();
    if w <= 0
        || h <= 0
        || w > 8192
        || h > 8192
        || i64::from(w) * i64::from(h) > 16_777_216
        || stride < w * 4
        || !matches!(format, Format::ARgb32 | Format::Rgb24)
    {
        return Err("Unsupported WebKit snapshot layout".into());
    }
    let mut rgba = vec![0; w as usize * h as usize * 4];
    image
        .with_data(|bytes| {
            for y in 0..h as usize {
                let input = &bytes[y * stride as usize..y * stride as usize + w as usize * 4];
                for (src, dst) in input
                    .chunks_exact(4)
                    .zip(rgba[y * w as usize * 4..(y + 1) * w as usize * 4].chunks_exact_mut(4))
                {
                    // Cairo ARGB32 is native-endian and already premultiplied.
                    let argb = u32::from_ne_bytes(src.try_into().unwrap());
                    dst.copy_from_slice(&[
                        (argb >> 16) as u8,
                        (argb >> 8) as u8,
                        argb as u8,
                        if format == Format::ARgb32 {
                            (argb >> 24) as u8
                        } else {
                            255
                        },
                    ]);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok((w as u32, h as u32, rgba))
}

impl Drop for Source {
    fn drop(&mut self) {
        self.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_premultiplied_alpha_and_native_endianness() {
        let pixels = [0x00000000_u32, 0x80808080, 0xffff2020];
        let bytes: Vec<u8> = pixels.into_iter().flat_map(u32::to_ne_bytes).collect();
        let image = ImageSurface::create_for_data(bytes, Format::ARgb32, 3, 1, 12).unwrap();
        let (_, _, rgba) = convert(image.as_ref().clone()).unwrap();
        assert_eq!(rgba, [0, 0, 0, 0, 128, 128, 128, 128, 255, 32, 32, 255]);
    }

    #[test]
    fn ignores_row_padding_and_forces_rgb24_opaque() {
        let mut bytes = Vec::new();
        for pixel in [0x00112233_u32, 0xdeadbeef, 0x00445566, 0xdeadbeef] {
            bytes.extend(pixel.to_ne_bytes());
        }
        let image = ImageSurface::create_for_data(bytes, Format::Rgb24, 1, 2, 8).unwrap();
        let (_, _, rgba) = convert(image.as_ref().clone()).unwrap();
        assert_eq!(rgba, [17, 34, 51, 255, 68, 85, 102, 255]);
    }

    #[test]
    fn rejects_unsupported_formats() {
        let image = ImageSurface::create(Format::A8, 4, 1).unwrap();
        assert!(convert(image.as_ref().clone()).is_err());
    }
}
