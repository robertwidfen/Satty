use std::cell::RefCell;

use anyhow::Result;
use femtovg::{Color, ImageFilter, ImageFlags, ImageId, Paint, Path, imgref::Img};

use relm4::{Sender, gtk::gdk::ModifierType};

use crate::{
    configuration::APP_CONFIG,
    math::{self, Vec2D},
    sketch_board::{MouseButton, MouseEventMsg, MouseEventType, SketchBoardInput},
    style::Style,
    tools::hit_test_rectangle,
};

use super::{
    Drawable, DrawableClone, SpotlightKind, Tool, ToolUpdateResult, Tools,
    drag_box::{DragBox, draw_center_marker},
};

#[derive(Clone, Debug)]
pub struct Blur {
    origin: Vec2D,
    top_left: Vec2D,
    size: Option<Vec2D>,
    style: Style,
    centered: bool,
    editing: bool,
    cached_image: RefCell<Option<ImageId>>,
}

impl Blur {
    fn calculate_shape(&mut self, pos: Vec2D, modifier: ModifierType) {
        let drag_box = DragBox::from_origin_delta(self.origin, pos, modifier);
        self.centered = drag_box.centered;
        self.top_left = drag_box.top_left;
        self.size = Some(drag_box.size);
    }

    fn blur(
        canvas: &mut femtovg::Canvas<femtovg::renderer::OpenGl>,
        pos: Vec2D,
        size: Vec2D,
        sigma: f32,
        background_image_id: Option<ImageId>,
    ) -> Result<ImageId> {
        let (src_image_id, width, height) = if let Some(background_image_id) = background_image_id {
            (background_image_id, size.x as usize, size.y as usize)
        } else {
            let img = canvas.screenshot()?;

            let transformed_pos = canvas.transform().transform_point(pos.x, pos.y);
            let transformed_size = size * canvas.transform().average_scale();

            let (buf, width, height) = img
                .sub_image(
                    transformed_pos.0 as usize,
                    transformed_pos.1 as usize,
                    (transformed_size.x as usize).max(1),
                    (transformed_size.y as usize).max(1),
                )
                .to_contiguous_buf();
            let sub = Img::new(buf.into_owned(), width, height);
            (
                canvas.create_image(sub.as_ref(), ImageFlags::empty())?,
                sub.width(),
                sub.height(),
            )
        };

        let dst_image_id = canvas.create_image_empty(
            width,
            height,
            femtovg::PixelFormat::Rgba8,
            ImageFlags::empty(),
        )?;

        canvas.filter_image(
            dst_image_id,
            ImageFilter::GaussianBlur { sigma },
            src_image_id,
        );

        Ok(dst_image_id)
    }
}

impl Drawable for Blur {
    fn bounds(&self) -> Option<(Vec2D, Vec2D)> {
        let size = self.size?;
        Some(math::ensure_bounding_box(
            self.top_left,
            self.top_left + size,
        ))
    }

    fn hit_test(&self, pos: Vec2D, tolerance: f32) -> bool {
        hit_test_rectangle(pos, self.top_left, self.size, tolerance, !self.style.fill)
    }

    fn translate(&mut self, delta: Vec2D) {
        self.top_left += delta;
        // invalidate cached blur image since position changed
        *self.cached_image.borrow_mut() = None;
    }

    fn resize_bounds(&mut self, tl: Vec2D, br: Vec2D) {
        let (tl, br) = math::ensure_bounding_box(tl, br);
        self.top_left = tl;
        self.size = Some(br - tl);
        *self.cached_image.borrow_mut() = None;
    }

    fn get_style(&self) -> Option<&Style> {
        Some(&self.style)
    }

    fn get_style_mut(&mut self) -> Option<&mut Style> {
        *self.cached_image.borrow_mut() = None;
        Some(&mut self.style)
    }

    fn get_spotlight(&self) -> SpotlightKind {
        if self.style.fill {
            SpotlightKind::Blur
        } else {
            SpotlightKind::None
        }
    }

    fn draw(
        &self,
        canvas: &mut femtovg::Canvas<femtovg::renderer::OpenGl>,
        _font: femtovg::FontId,
        bounds: (Vec2D, Vec2D),
    ) -> Result<()> {
        let size = match self.size {
            Some(s) => s,
            None => return Ok(()), // early exit if none
        };
        let (pos, size) = math::rect_ensure_in_bounds(
            math::rect_ensure_positive_size(self.top_left, size),
            bounds,
        );
        if self.editing {
            if self.centered {
                draw_center_marker(canvas, self.origin);
            }

            // make rect
            let mut path = Path::new();
            path.rounded_rect(
                pos.x,
                pos.y,
                size.x,
                size.y,
                APP_CONFIG.read().corner_roundness(),
            );

            // draw
            if self.get_spotlight() != SpotlightKind::None {
                let paint = Paint::color(femtovg::Color::rgb(
                    self.style.color.r,
                    self.style.color.g,
                    self.style.color.b,
                ));

                canvas.stroke_path(&path, &paint);
            } else {
                // set style
                let mut color = Color::black();
                color.set_alphaf(0.6);
                let paint = Paint::color(color);

                canvas.fill_path(&path, &paint);
            }
        } else {
            if size.x <= 0.0 || size.y <= 0.0 {
                return Ok(());
            }

            canvas.save();
            canvas.flush();

            // create new cached image
            if self.cached_image.borrow().is_none() {
                self.cached_image.borrow_mut().replace(Self::blur(
                    canvas,
                    pos,
                    size,
                    self.style
                        .size
                        .to_blur_factor(self.style.annotation_size_factor),
                    None,
                )?);
            }

            let mut path = Path::new();
            path.rounded_rect(
                pos.x,
                pos.y,
                size.x,
                size.y,
                APP_CONFIG.read().corner_roundness(),
            );

            canvas.fill_path(
                &path,
                &Paint::image(
                    self.cached_image.borrow().unwrap(), // this unwrap is safe because we placed it above
                    pos.x,
                    pos.y,
                    size.x,
                    size.y,
                    0f32,
                    1f32,
                ),
            );
            canvas.restore();
        }
        Ok(())
    }

    fn draw_spotlight(
        &self,
        canvas: &mut femtovg::Canvas<femtovg::renderer::OpenGl>,
        bounds: (Vec2D, Vec2D),
        boxes: &Vec<(Vec2D, Vec2D)>,
        spotlight_preview: bool,
        background_image_id: Option<femtovg::ImageId>,
    ) {
        let canvas_tl = bounds.0;
        let canvas_size = bounds.1 - bounds.0;

        if self.cached_image.borrow().is_none() {
            // create new cached image
            canvas.save();
            canvas.flush();
            self.cached_image.borrow_mut().replace(
                Self::blur(
                    canvas,
                    canvas_tl,
                    canvas_size,
                    self.style
                        .size
                        .to_blur_factor(self.style.annotation_size_factor),
                    background_image_id,
                )
                .unwrap(),
            );
            canvas.restore();
        }

        if spotlight_preview {
            let mut paint = Paint::color(Color::rgba(255, 0, 0, 255));
            paint = paint.with_line_width(1.0);
            for (tl, br) in boxes {
                let (pos, size) = math::rect_ensure_in_bounds(
                    math::rect_ensure_positive_size(*tl, *br - *tl),
                    bounds,
                );

                let mut path = Path::new();
                path.rounded_rect(
                    pos.x,
                    pos.y,
                    size.x,
                    size.y,
                    APP_CONFIG.read().corner_roundness(),
                );
                canvas.stroke_path(&path, &paint);
            }
        } else {
            let mut path = Path::new();
            path.rect(canvas_tl.x, canvas_tl.y, canvas_size.x, canvas_size.y);
            for (tl, br) in boxes {
                let (pos, size) = math::rect_ensure_in_bounds(
                    math::rect_ensure_positive_size(*tl, *br - *tl),
                    bounds,
                );

                path.rounded_rect(
                    pos.x,
                    pos.y,
                    size.x,
                    size.y,
                    APP_CONFIG.read().corner_roundness(),
                );
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
        }
    }
}

#[derive(Default)]
pub struct BlurTool {
    blur: Option<Blur>,
    style: Style,
    input_enabled: bool,
    sender: Option<Sender<SketchBoardInput>>,
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
        Tools::Blur
    }

    fn handle_mouse_event(&mut self, event: MouseEventMsg) -> ToolUpdateResult {
        match event.type_ {
            MouseEventType::BeginDrag => {
                if event.button == MouseButton::Middle {
                    return ToolUpdateResult::Unmodified;
                }

                // start new
                self.blur = Some(Blur {
                    origin: event.pos,
                    top_left: event.pos,
                    size: None,
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
                    if event.pos == Vec2D::zero() {
                        self.blur = None;

                        ToolUpdateResult::Redraw
                    } else {
                        a.calculate_shape(event.pos, event.modifier);
                        a.editing = false;

                        let result = a.clone_box();
                        self.blur = None;

                        if event.pos.x.abs() < 25.0 && event.pos.y.abs() < 25.0 {
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
                    a.calculate_shape(event.pos, event.modifier);

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
