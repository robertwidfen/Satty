use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use femtovg::{ImageFilter, ImageFlags, ImageId, Paint, Path, imgref::ImgVec, rgb::RGBA8};

use relm4::Sender;

use crate::{
    math::{self, Vec2D},
    sketch_board::{MouseButton, MouseEventMsg, MouseEventType, SketchBoardInput},
    style::Style,
    tools::{RenderingMode, drag_box::draw_rect_marker, hit_test_rectangle},
};

use super::{
    Drawable, DrawableClone, Tool, ToolUpdateResult, Tools,
    drag_box::{DragBox, draw_center_marker},
};

#[derive(Clone, Debug)]
pub struct Blur {
    is_spotlight: bool,
    shared_lensblur_sigma: Arc<Mutex<f32>>,
    cached_sigma: Cell<f32>,
    origin: Vec2D,
    top_left: Vec2D,
    size: Vec2D,
    style: Style,
    centered: bool,
    editing: bool,
    cached_image: RefCell<Option<ImageId>>,
}

impl Blur {
    fn get_lensblur_sigma(&self) -> f32 {
        let shared = self.shared_lensblur_sigma.lock().unwrap();
        *shared
    }

    fn calculate_shape(&mut self, sender: &Sender<SketchBoardInput>, event: &MouseEventMsg) {
        let drag_box = DragBox::from_origin_delta(self.origin, self.size, event, sender);
        self.centered = drag_box.centered;
        self.top_left = drag_box.top_left;
        self.size = drag_box.size;
    }

    fn blur(
        canvas: &mut femtovg::Canvas<femtovg::renderer::OpenGl>,
        source_size: Vec2D,
        sigma: f32,
        source_image: ImageId,
    ) -> Result<ImageId> {
        let dst_image_id = canvas.create_image_empty(
            (source_size.x as usize).max(1),
            (source_size.y as usize).max(1),
            femtovg::PixelFormat::Rgba8,
            ImageFlags::empty(),
        )?;

        canvas.filter_image(
            dst_image_id,
            ImageFilter::GaussianBlur { sigma },
            source_image,
        );

        Ok(dst_image_id)
    }
}

impl Drawable for Blur {
    fn get_rendering_mode(&self) -> RenderingMode {
        match self.is_spotlight {
            true => RenderingMode::SpotlightBlur,
            false => RenderingMode::BlurOrPixelate,
        }
    }

    fn bounds(&self) -> Option<(Vec2D, Vec2D)> {
        Some(math::ensure_bounding_box(
            self.top_left,
            self.top_left + self.size,
        ))
    }

    fn hit_test(&self, pos: Vec2D, tolerance: f32) -> bool {
        hit_test_rectangle(pos, self.top_left, self.size, tolerance, !self.is_spotlight)
    }

    fn translate(&mut self, delta: Vec2D) {
        self.top_left += delta;
        // invalidate cached blur image since position changed
        *self.cached_image.borrow_mut() = None;
    }

    fn resize_bounds(&mut self, tl: Vec2D, br: Vec2D, _delta: Vec2D, _keep_aspect: bool) {
        let (tl, br) = math::ensure_bounding_box(tl, br);
        self.top_left = tl;
        self.size = br - tl;
        *self.cached_image.borrow_mut() = None;
    }

    fn get_style(&self) -> Option<&Style> {
        Some(&self.style)
    }

    fn set_style(&mut self, style: Style) {
        *self.cached_image.borrow_mut() = None;
        self.style = style;
        if self.is_spotlight {
            *self.shared_lensblur_sigma.lock().unwrap() = style.blur_factor();
        }
    }

    fn draw(
        &self,
        _canvas: &mut femtovg::Canvas<femtovg::renderer::OpenGl>,
        _font: femtovg::FontId,
        _bounds: (Vec2D, Vec2D),
    ) -> Result<()> {
        Ok(())
    }

    fn draw_baselayer(
        &self,
        canvas: &mut femtovg::Canvas<femtovg::renderer::OpenGl>,
        _source_image: &ImgVec<RGBA8>,
        background_image_id: ImageId,
        _font: femtovg::FontId,
        bounds: (Vec2D, Vec2D),
    ) -> Result<()> {
        let size = self.size;
        let (pos, size) = math::rect_ensure_in_bounds(
            math::rect_ensure_positive_size(self.top_left, size),
            bounds,
        );
        let source_pos = bounds.0;
        let source_size = bounds.1 - bounds.0;

        if size.x <= 0.0 || size.y <= 0.0 {
            return Ok(());
        }

        // create new cached image
        if self.cached_image.borrow().is_none() {
            self.cached_image.borrow_mut().replace(Self::blur(
                canvas,
                source_size,
                self.style.blur_factor(),
                background_image_id,
            )?);
        }

        let mut path = Path::new();
        path.rounded_rect(pos.x, pos.y, size.x, size.y, self.style.corner_radius());

        canvas.fill_path(
            &path,
            &Paint::image(
                self.cached_image.borrow().unwrap(),
                source_pos.x,
                source_pos.y,
                source_size.x,
                source_size.y,
                0f32,
                1f32,
            ),
        );

        if self.editing && self.centered {
            draw_center_marker(canvas, self.origin);
        }

        Ok(())
    }

    fn set_centered(&mut self, centered: bool) {
        self.centered = centered;
        self.origin = self.top_left + self.size / 2.0;
    }

    fn set_editing(&mut self, editing: bool) {
        self.editing = editing;
    }

    fn draw_spotlight(
        &self,
        canvas: &mut femtovg::Canvas<femtovg::renderer::OpenGl>,
        bounds: (Vec2D, Vec2D),
        boxes: &Vec<(Vec2D, Vec2D)>,
        spotlight_preview: bool,
        background_image_id: femtovg::ImageId,
    ) {
        let canvas_tl = bounds.0;
        let canvas_size = bounds.1 - bounds.0;

        let sigma = self.get_lensblur_sigma();
        if self.cached_sigma.get() != sigma {
            *self.cached_image.borrow_mut() = None;
        }

        if self.cached_image.borrow().is_none() {
            // create new cached image
            self.cached_sigma.set(sigma);
            self.cached_image
                .borrow_mut()
                .replace(Self::blur(canvas, canvas_size, sigma, background_image_id).unwrap());
        }

        if spotlight_preview {
            // do not blur in preview, just show boxes
            for (tl, br) in boxes {
                let (pos, size) = math::rect_ensure_in_bounds(
                    math::rect_ensure_positive_size(*tl, *br - *tl),
                    bounds,
                );
                draw_rect_marker(canvas, pos, size, false);
            }
        } else {
            // normal blur
            let mut path = Path::new();
            path.rect(canvas_tl.x, canvas_tl.y, canvas_size.x, canvas_size.y);
            for (tl, br) in boxes {
                let (pos, size) = math::rect_ensure_in_bounds(
                    math::rect_ensure_positive_size(*tl, *br - *tl),
                    bounds,
                );

                path.rounded_rect(pos.x, pos.y, size.x, size.y, self.style.corner_radius());
            }

            canvas.fill_path(
                &path,
                &Paint::image(
                    self.cached_image.borrow().unwrap(),
                    canvas_tl.x,
                    canvas_tl.y,
                    canvas_size.x,
                    canvas_size.y,
                    0f32,
                    1f32,
                )
                .with_fill_rule(femtovg::FillRule::EvenOdd),
            );

            if self.editing && self.centered {
                draw_center_marker(canvas, self.origin);
            }
        }
    }
}

#[derive(Default)]
pub struct BlurTool {
    is_spotlight: bool,
    shared_lens_blur_sigma: Arc<Mutex<f32>>,
    blur: Option<Blur>,
    style: Style,
    input_enabled: bool,
    sender: Option<Sender<SketchBoardInput>>,
}

impl BlurTool {
    pub(crate) fn spotlight() -> Self {
        let style = Style::default();
        let sigma = style.blur_factor();
        BlurTool {
            is_spotlight: true,
            shared_lens_blur_sigma: Arc::new(Mutex::new(sigma)),
            blur: None,
            style,
            input_enabled: true,
            sender: None,
        }
    }
}

impl Tool for BlurTool {
    fn input_enabled(&self) -> bool {
        self.input_enabled
    }

    fn set_input_enabled(&mut self, value: bool) {
        self.input_enabled = value;
    }

    fn active(&self) -> bool {
        self.blur.is_some()
    }

    fn get_tool_type(&self) -> super::Tools {
        if self.is_spotlight {
            Tools::Lensblur
        } else {
            Tools::Blur
        }
    }

    fn handle_mouse_event(&mut self, event: MouseEventMsg) -> ToolUpdateResult {
        match event.type_ {
            MouseEventType::BeginDrag => {
                if event.button == MouseButton::Middle {
                    return ToolUpdateResult::Unmodified;
                }

                // start new
                self.blur = Some(Blur {
                    is_spotlight: self.is_spotlight,
                    shared_lensblur_sigma: self.shared_lens_blur_sigma.clone(),
                    cached_sigma: Cell::new(f32::NAN),
                    origin: event.pos,
                    top_left: event.pos,
                    size: Vec2D::zero(),
                    style: self.style,
                    centered: false,
                    editing: true,
                    cached_image: RefCell::new(None),
                });

                ToolUpdateResult::Redraw
            }
            MouseEventType::EndDrag => {
                if event.button == MouseButton::Middle {
                    return ToolUpdateResult::Unmodified;
                }

                if let Some(a) = &mut self.blur {
                    a.editing = false;
                    if event.pos == Vec2D::zero() {
                        self.blur = None;

                        ToolUpdateResult::Redraw
                    } else {
                        a.calculate_shape(self.sender.as_ref().unwrap(), &event);

                        let result = a.clone_box();
                        self.blur = None;

                        if event.pos.x.abs() <= 0.0 && event.pos.y.abs() <= 0.0 {
                            return ToolUpdateResult::Unmodified;
                        }

                        ToolUpdateResult::Commit(result)
                    }
                } else {
                    ToolUpdateResult::Unmodified
                }
            }

            MouseEventType::UpdateDrag => {
                if event.button == MouseButton::Middle {
                    return ToolUpdateResult::Unmodified;
                }

                if let Some(a) = &mut self.blur {
                    if event.pos == Vec2D::zero() {
                        return ToolUpdateResult::Unmodified;
                    }
                    a.calculate_shape(self.sender.as_ref().unwrap(), &event);

                    ToolUpdateResult::Redraw
                } else {
                    ToolUpdateResult::Unmodified
                }
            }
            _ => ToolUpdateResult::Unmodified,
        }
    }

    fn handle_style_event(&mut self, style: Style) -> ToolUpdateResult {
        self.style = style;
        if self.is_spotlight {
            let mut shared_sigma = self.shared_lens_blur_sigma.lock().unwrap();
            *shared_sigma = self.style.blur_factor();
        }
        ToolUpdateResult::Unmodified
    }

    fn get_drawable(&self) -> Option<&dyn Drawable> {
        match &self.blur {
            Some(d) => Some(d),
            None => None,
        }
    }

    fn set_sender(&mut self, sender: Sender<SketchBoardInput>) {
        self.sender = Some(sender);
    }
}
