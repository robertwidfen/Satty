use std::cell::Cell;
use std::f32::consts::{FRAC_PI_2, PI};

use anyhow::Result;
use femtovg::{FontId, Paint, Path};
use relm4::Sender;

use crate::{
    math::Vec2D,
    sketch_board::{MouseButton, MouseEventMsg, MouseEventType, SketchBoardInput},
    style::Style,
};

use super::{Drawable, DrawableClone, Straight, Tool, ToolUpdateResult, Tools};

/// Length and angle of a line from its start to its end. The angle is
/// counterclockwise from the right, from -180° to 180°.
pub fn format_line_dimensions(delta: Vec2D) -> (String, String) {
    let length = format!("{:.0}px", delta.norm());
    if delta.norm() < f32::EPSILON {
        return (length, String::new());
    }
    // y points down on screen; 0.0 - y instead of -y gives no negative zero,
    // which would turn 180° into -180°, and + 0.0 avoids showing -0
    let angle = (0.0 - delta.y).atan2(delta.x).to_degrees().round() + 0.0;
    (length, format!("{angle}°"))
}

/// The label of a measurement: the length, and the angle unless the line is
/// horizontal or vertical.
fn measurement_label(delta: Vec2D) -> String {
    let (length, angle) = format_line_dimensions(delta);
    let degrees = (0.0 - delta.y).atan2(delta.x).to_degrees().round();
    if angle.is_empty() || degrees % 90.0 == 0.0 {
        length
    } else {
        format!("{length} {angle}")
    }
}

/// Angle in radians to rotate the label by so it runs along the line and is
/// never upside down; vertical lines read from bottom to top.
fn label_rotation(delta: Vec2D) -> f32 {
    let theta = delta.y.atan2(delta.x);
    if theta >= FRAC_PI_2 {
        theta - PI
    } else if theta < -FRAC_PI_2 {
        theta + PI
    } else {
        theta
    }
}

#[derive(Default)]
pub struct MeasureTool {
    measure: Option<Measure>,
    style: Style,
    input_enabled: bool,
    sender: Option<Sender<SketchBoardInput>>,
}

/// The pixel `pos` is on, as the coordinates of its top left corner.
fn pixel_at(pos: Vec2D) -> Vec2D {
    Vec2D::new(pos.x.floor(), pos.y.floor())
}

/// Moves `pixel` by `delta` rounded to whole pixels.
fn move_pixel(pixel: Vec2D, delta: Vec2D) -> Vec2D {
    pixel + pixel_at(delta + 0.5)
}

/// The start and end of a line from the pixel `start` to the pixel `end`,
/// both measured: along the line on the outer edges of the pixels, where the
/// ticks end, across it on their centers. The line counts as horizontal when
/// it is at least as wide as high.
fn pixel_line(start: Vec2D, end: Vec2D) -> (Vec2D, Vec2D) {
    // the outer edges along one axis, from the lower to the higher pixel
    let edges = |a: f32, b: f32| if a <= b { (a, b + 1.0) } else { (a + 1.0, b) };
    let delta = end - start;
    if delta.x.abs() >= delta.y.abs() {
        let (x0, x1) = edges(start.x, end.x);
        (Vec2D::new(x0, start.y + 0.5), Vec2D::new(x1, end.y + 0.5))
    } else {
        let (y0, y1) = edges(start.y, end.y);
        (Vec2D::new(start.x + 0.5, y0), Vec2D::new(end.x + 0.5, y1))
    }
}

/// A line with ticks at its ends, |------|, labelled with its length and angle.
/// It measures from the pixel `start_pixel` to the pixel `end_pixel`, both
/// included; `start` and `end` are its ends, see pixel_line.
#[derive(Clone, Debug)]
pub struct Measure {
    /// where the drawing started, not snapped
    origin: Vec2D,
    start_pixel: Vec2D,
    end_pixel: Vec2D,
    start: Vec2D,
    end: Vec2D,
    style: Style,
    /// width of the label, known after drawing, for hit tests
    label_width: Cell<f32>,
}

impl Measure {
    fn new(origin: Vec2D, style: Style) -> Self {
        let pixel = pixel_at(origin);
        let mut measure = Self {
            origin,
            start_pixel: pixel,
            end_pixel: pixel,
            start: origin,
            end: origin,
            style,
            label_width: Cell::new(0.0),
        };
        measure.set_pixels(pixel, pixel);
        measure
    }

    fn set_pixels(&mut self, start_pixel: Vec2D, end_pixel: Vec2D) {
        self.start_pixel = start_pixel;
        self.end_pixel = end_pixel;
        (self.start, self.end) = pixel_line(start_pixel, end_pixel);
    }

    /// Sets the end `delta` from where the drawing started, on the pixels
    /// under the mouse.
    fn set_delta(&mut self, delta: Vec2D) {
        self.set_pixels(pixel_at(self.origin), pixel_at(self.origin + delta));
    }

    fn line_width(&self) -> f32 {
        self.style.line_width()
    }

    /// Half the length of the ticks at the ends.
    fn tick_half_length(&self) -> f32 {
        3.0 * self.line_width() + 4.0
    }

    fn font_size(&self) -> f32 {
        12.0 + 3.0 * self.line_width()
    }

    /// Distance between the line and the bottom of the label.
    fn label_gap(&self) -> f32 {
        self.line_width() / 2.0 + self.font_size() * 0.2
    }

    /// Shift across a horizontal or vertical line with an even line width.
    /// The line runs through pixel centers, so its edges are on whole pixels
    /// for odd widths, but half cover pixels for even ones without the shift.
    fn pixel_offset(&self) -> Vec2D {
        let width = self.line_width();
        if width.fract() != 0.0 || width % 2.0 != 0.0 {
            return Vec2D::zero();
        }
        let delta = self.end - self.start;
        if delta.y == 0.0 {
            Vec2D::new(0.0, 0.5)
        } else if delta.x == 0.0 {
            Vec2D::new(0.5, 0.0)
        } else {
            Vec2D::zero()
        }
    }

    /// Center of the line, where the label is.
    fn mid(&self) -> Vec2D {
        (self.start + self.end) * 0.5 + self.pixel_offset()
    }

    /// Half the visible length of the ticks, including round caps. Shortened
    /// below the label when the line is too short for the label to fit
    /// between the ticks.
    fn tick_reach(&self, length: f32) -> f32 {
        let width = self.line_width();
        let clearance = self.font_size() * 0.1;
        let between_ticks = length / 2.0 - width;
        let reach = if self.label_width.get() / 2.0 + clearance > between_ticks {
            self.tick_half_length()
                .min(self.label_gap() - clearance)
                .max(width / 2.0)
        } else {
            self.tick_half_length()
        };
        // the ends on whole pixels: the line is on pixel centers, unless
        // shifted by half a pixel onto their edges
        let center = 0.5 - self.pixel_offset().norm();
        ((reach - center).round() + center).max(width / 2.0)
    }

    /// The line and the ticks at the start and the end as drawn, None for a
    /// point. femtovg centers the stroke on the path and round caps add half
    /// the line width at the ends, so all is moved inwards by half the line
    /// width to end flush with the start and end.
    fn segments(&self) -> Option<[(Vec2D, Vec2D); 3]> {
        let delta = self.end - self.start;
        let length = delta.norm();
        if length < f32::EPSILON {
            return None;
        }
        let half_width = self.line_width() / 2.0;
        let offset = self.pixel_offset();
        // the outer edges of the ticks are on the start and the end
        let along = delta / length * half_width.min(length / 2.0);
        let tick_start = self.start + offset + along;
        let tick_end = self.end + offset - along;
        let cap = if self.style.round_caps {
            half_width
        } else {
            0.0
        };
        let tick_half = (self.tick_reach(length) - cap).max(0.0);
        // the unit vector first, exact for horizontal and vertical lines
        let normal = Vec2D::new(-delta.y, delta.x) / length * tick_half;
        // between the centers of the ticks, round caps reach the start and end
        Some([
            (tick_start, tick_end),
            (tick_start - normal, tick_start + normal),
            (tick_end - normal, tick_end + normal),
        ])
    }

    /// Whether `pos` is on the label, in the coordinates rotated with it.
    fn label_hit(&self, pos: Vec2D, tolerance: f32) -> bool {
        // horizontal without a direction, at length 0
        let rotation = label_rotation(self.end - self.start);
        let (sin, cos) = rotation.sin_cos();
        let p = pos - self.mid();
        let x = p.x * cos + p.y * sin;
        let y = -p.x * sin + p.y * cos;
        let half_width = self.label_width.get() / 2.0;
        let bottom = -self.label_gap();
        let top = bottom - self.font_size();
        x.abs() <= half_width + tolerance && y >= top - tolerance && y <= bottom + tolerance
    }
}

impl Drawable for Measure {
    fn anchors(&self) -> Option<(Vec2D, Vec2D)> {
        Some((self.start, self.end))
    }

    fn get_bounding_box(&self) -> Option<(Vec2D, Vec2D)> {
        // the ticks and the label stick out of the line
        let margin = (self.tick_half_length() + self.line_width() / 2.0)
            .max(self.label_gap() + self.font_size())
            .max(self.label_width.get() / 2.0);
        Some(super::bounding_box_with_margin(
            self.start, self.end, margin,
        ))
    }

    fn get_selection_type(&self) -> super::SelectionType {
        super::SelectionType::Line
    }

    fn hit_test(&self, pos: Vec2D, tolerance: f32) -> bool {
        let reach = tolerance + self.line_width() / 2.0;
        pos.distance_to_segment(self.start, self.end) <= reach
            || self.segments().is_some_and(|segments| {
                segments
                    .iter()
                    .any(|(a, b)| pos.distance_to_segment(*a, *b) <= reach)
            })
            || self.label_hit(pos, tolerance)
    }

    fn translate(&mut self, delta: Vec2D) {
        self.set_pixels(
            move_pixel(self.start_pixel, delta),
            move_pixel(self.end_pixel, delta),
        );
    }

    /// Moves the pixels of the ends by how far they were dragged, so an end
    /// not dragged keeps its pixel, even if the line turns from horizontal to
    /// vertical and its end moves from a pixel edge to a pixel center.
    fn resize(&mut self, tl: Vec2D, br: Vec2D, _keep_aspect: bool) {
        self.set_pixels(
            move_pixel(self.start_pixel, tl - self.start),
            move_pixel(self.end_pixel, br - self.end),
        );
    }

    fn get_style(&self) -> Option<&Style> {
        Some(&self.style)
    }

    fn set_style(&mut self, style: Style) {
        self.style = style;
    }

    fn draw(
        &self,
        canvas: &mut femtovg::Canvas<femtovg::renderer::OpenGl>,
        font: FontId,
        _bounds: (Vec2D, Vec2D),
    ) -> Result<()> {
        let paint: Paint = self.style.into();
        let mut path = Path::new();

        let delta = self.end - self.start;
        let label = measurement_label(delta);
        let mut text_paint = Paint::color(self.style.paint_color());
        text_paint.set_font(&[font]);
        text_paint.set_font_size(self.font_size());
        text_paint.set_text_align(femtovg::Align::Center);
        text_paint.set_text_baseline(femtovg::Baseline::Bottom);
        // before the ticks, they are shortened when the label does not fit
        let metrics = canvas.measure_text(0.0, 0.0, &label, &text_paint)?;
        self.label_width.set(metrics.width());

        if let Some(segments) = self.segments() {
            for (a, b) in segments {
                path.move_to(a.x, a.y);
                path.line_to(b.x, b.y);
            }
        } else {
            // also before the first move: draw a point at start, like the line
            path.move_to(self.start.x, self.start.y);
            path.line_to(self.start.x + 0.01, self.start.y);
        }
        canvas.stroke_path(&path, &paint);

        let mid = self.mid();
        canvas.save();
        canvas.translate(mid.x, mid.y);
        canvas.rotate(label_rotation(delta));
        canvas.fill_text(0.0, -self.label_gap(), &label, &text_paint)?;
        canvas.restore();

        Ok(())
    }
}

impl Tool for MeasureTool {
    fn input_enabled(&self) -> bool {
        self.input_enabled
    }

    fn set_input_enabled(&mut self, value: bool) {
        self.input_enabled = value;
    }

    fn active(&self) -> bool {
        self.measure.is_some()
    }

    fn handle_mouse_event(&mut self, event: MouseEventMsg) -> ToolUpdateResult {
        if event.button == MouseButton::Middle {
            return ToolUpdateResult::Unmodified;
        }
        match event.type_ {
            MouseEventType::BeginDrag => {
                self.measure = Some(Measure::new(event.pos, self.style));
                ToolUpdateResult::Redraw
            }
            MouseEventType::EndDrag => {
                let Some(mut measure) = self.measure.take() else {
                    return ToolUpdateResult::Unmodified;
                };
                if event.pos == Vec2D::zero() {
                    ToolUpdateResult::Redraw
                } else {
                    measure.set_delta(Straight::apply(event.modifier, event.pos));
                    ToolUpdateResult::Commit(measure.clone_box())
                }
            }
            MouseEventType::UpdateDrag => {
                let Some(measure) = &mut self.measure else {
                    return ToolUpdateResult::Unmodified;
                };
                measure.set_delta(Straight::apply(event.modifier, event.pos));
                if let Some(sender) = &self.sender {
                    sender
                        .send(SketchBoardInput::LineDimensionsUpdate(
                            measure.end - measure.start,
                        ))
                        .ok();
                }
                ToolUpdateResult::Redraw
            }
            _ => ToolUpdateResult::Unmodified,
        }
    }

    fn handle_style_event(&mut self, style: Style) -> ToolUpdateResult {
        self.style = style;
        ToolUpdateResult::Unmodified
    }

    fn get_drawable(&self) -> Option<&dyn Drawable> {
        self.measure.as_ref().map(|m| m as &dyn Drawable)
    }

    fn get_tool_type(&self) -> super::Tools {
        Tools::Measure
    }

    fn set_sender(&mut self, sender: Sender<SketchBoardInput>) {
        self.sender = Some(sender);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Size;

    #[test]
    fn label_hides_right_angles() {
        assert_eq!(measurement_label(Vec2D::new(100.0, 0.0)), "100px");
        assert_eq!(measurement_label(Vec2D::new(0.0, -50.0)), "50px");
        assert_eq!(measurement_label(Vec2D::new(-30.0, 0.0)), "30px");
        assert_eq!(measurement_label(Vec2D::new(30.0, -30.0)), "42px 45°");
        assert_eq!(measurement_label(Vec2D::new(-30.0, 30.0)), "42px -135°");
    }

    #[test]
    fn ends_on_outer_edges_along_and_centers_across() {
        // horizontal from pixel 10 to 109: x on their outer edges, y centered
        let mut measure = Measure::new(Vec2D::new(10.4, 20.6), Style::default());
        measure.set_delta(Vec2D::new(99.3, 3.0));
        assert_eq!(measure.start, Vec2D::new(10.0, 20.5));
        assert_eq!(measure.end, Vec2D::new(110.0, 23.5));
        // right to left, the outer edges are the other ones
        measure.set_delta(Vec2D::new(-5.0, 0.0));
        assert_eq!(measure.start, Vec2D::new(11.0, 20.5));
        assert_eq!(measure.end, Vec2D::new(5.0, 20.5));
        // vertical: x centered, y on the outer edges
        measure.set_delta(Vec2D::new(3.0, -50.3));
        assert_eq!(measure.start, Vec2D::new(10.5, 21.0));
        assert_eq!(measure.end, Vec2D::new(13.5, -30.0));
        // a single pixel
        measure.set_delta(Vec2D::new(0.3, 0.2));
        assert_eq!(measure.end - measure.start, Vec2D::new(1.0, 0.0));
    }

    #[test]
    fn resizing_keeps_the_pixel_of_the_other_end() {
        // horizontal from pixel (10, 20) to (109, 20)
        let mut measure = Measure::new(Vec2D::new(10.4, 20.6), Style::default());
        measure.set_delta(Vec2D::new(99.3, 0.0));
        let (start, end) = (measure.start, measure.end);
        // drag the end below the start, the line turns vertical
        measure.resize(start, end + Vec2D::new(-99.5, 60.2), false);
        assert_eq!(measure.start_pixel, Vec2D::new(10.0, 20.0));
        assert_eq!(measure.end_pixel, Vec2D::new(10.0, 80.0));
        assert_eq!(measure.start, Vec2D::new(10.5, 20.0));
        assert_eq!(measure.end, Vec2D::new(10.5, 81.0));
    }

    #[test]
    fn ticks_end_flush_with_the_outer_pixel_edges() {
        let style = Style {
            size: Size::new(4.0),
            round_caps: false,
            ..Style::default()
        };
        let mut measure = Measure::new(Vec2D::new(10.0, 20.0), style);
        measure.set_delta(Vec2D::new(99.0, 0.0));
        let [line, start_tick, end_tick] = measure.segments().unwrap();
        // the 4 px wide ticks cover x 10 to 14 and 106 to 110
        assert_eq!(start_tick.0.x, 12.0);
        assert_eq!(end_tick.0.x, 108.0);
        // the even width is shifted by half a pixel to cover y 19 to 23
        assert_eq!(line, (Vec2D::new(12.0, 21.0), Vec2D::new(108.0, 21.0)));
        // and the ends of the ticks are on whole pixels
        assert_eq!(start_tick, (Vec2D::new(12.0, 5.0), Vec2D::new(12.0, 37.0)));

        // odd widths are crisp on the pixel centers without the shift
        measure.style.size = Size::new(3.0);
        assert_eq!(measure.pixel_offset(), Vec2D::zero());
    }

    #[test]
    fn label_is_never_upside_down() {
        // left to right and right to left read the same
        assert_eq!(label_rotation(Vec2D::new(-10.0, 0.0)), 0.0);
        // vertical lines read from bottom to top
        assert_eq!(label_rotation(Vec2D::new(0.0, 10.0)), -FRAC_PI_2);
        assert_eq!(label_rotation(Vec2D::new(0.0, -10.0)), -FRAC_PI_2);
    }
}
