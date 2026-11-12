use gtk::{atk::prelude::*, prelude::*};
#[derive(Clone, Copy)]
pub enum Icon {
    Expand,
    Restore,
    Volume,
    Close,
    Watch,
    People,
    Settings,
    Leave,
    Cast,
    Stop,
}
pub fn image(icon: Icon) -> gtk::DrawingArea {
    let image = gtk::DrawingArea::new();
    image.set_size_request(18, 18);
    image.set_valign(gtk::Align::Center);
    image.connect_draw(move |area, cr| {
        let color = area.style_context().color(area.state_flags());
        cr.set_source_rgba(color.red(), color.green(), color.blue(), color.alpha());
        let side = f64::from(area.allocated_width().min(area.allocated_height()));
        cr.translate(
            (f64::from(area.allocated_width()) - side) / 2.0,
            (f64::from(area.allocated_height()) - side) / 2.0,
        );
        cr.scale(side / 18.0, side / 18.0);
        cr.set_line_width(1.7);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        let line = |x, y, a, b| {
            cr.move_to(x, y);
            cr.line_to(a, b);
        };
        match icon {
            Icon::Close => {
                line(4., 4., 14., 14.);
                line(14., 4., 4., 14.);
            }
            Icon::Stop => {
                cr.rectangle(4., 4., 10., 10.);
            }
            Icon::Expand | Icon::Restore => {
                for (x, y, dx, dy) in [
                    (3., 3., 1., 1.),
                    (15., 3., -1., 1.),
                    (3., 15., 1., -1.),
                    (15., 15., -1., -1.),
                ] {
                    let (x, y, dx, dy) = if matches!(icon, Icon::Restore) {
                        (x + dx * 4., y + dy * 4., -dx, -dy)
                    } else {
                        (x, y, dx, dy)
                    };
                    cr.move_to(x, y + dy * 4.);
                    cr.line_to(x, y);
                    cr.line_to(x + dx * 4., y);
                }
            }
            Icon::Watch => {
                cr.move_to(5., 3.);
                cr.line_to(15., 9.);
                cr.line_to(5., 15.);
                cr.close_path();
            }
            Icon::People => {
                cr.arc(7., 5., 3., 0., std::f64::consts::TAU);
                cr.new_sub_path();
                cr.arc(7., 15., 5., std::f64::consts::PI, std::f64::consts::TAU);
                cr.new_sub_path();
                cr.arc(13., 5., 2., -1.2, 1.2);
                cr.new_sub_path();
                cr.arc(13., 15., 3., -std::f64::consts::FRAC_PI_2, 0.);
            }
            Icon::Volume => {
                cr.move_to(3., 7.);
                cr.line_to(6., 7.);
                cr.line_to(10., 3.);
                cr.line_to(10., 15.);
                cr.line_to(6., 11.);
                cr.line_to(3., 11.);
                cr.close_path();
                cr.new_sub_path();
                cr.arc(10., 9., 6., -0.8, 0.8);
            }
            Icon::Settings => {
                cr.arc(9., 9., 3., 0., std::f64::consts::TAU);
                for n in 0..8 {
                    let a = n as f64 * std::f64::consts::FRAC_PI_4;
                    line(
                        9. + a.cos() * 6.,
                        9. + a.sin() * 6.,
                        9. + a.cos() * 8.,
                        9. + a.sin() * 8.,
                    );
                }
            }
            Icon::Leave => {
                line(8., 3., 3., 3.);
                line(3., 3., 3., 15.);
                line(3., 15., 8., 15.);
                line(7., 9., 16., 9.);
                line(12., 5., 16., 9.);
                line(16., 9., 12., 13.);
            }
            Icon::Cast => {
                cr.rectangle(2., 3., 14., 10.);
                line(6., 16., 12., 16.);
                line(9., 13., 9., 16.);
            }
        }
        let _ = cr.stroke();
        gtk::glib::Propagation::Proceed
    });
    image
}
pub fn set(button: &gtk::Button, icon: Icon, text: &str, show_text: bool) {
    if let Some(child) = button.child() {
        button.remove(&child);
    }
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 7);
    content.set_halign(gtk::Align::Center);
    content.add(&image(icon));
    if show_text {
        content.add(&gtk::Label::new(Some(text)));
    }
    button.add(&content);
    button.set_tooltip_text(Some(text));
    if let Some(accessible) = button.accessible() {
        accessible.set_name(text);
    }
    button.style_context().add_class("icon-button");
    content.show_all();
}
pub fn button(icon: Icon, text: &str) -> gtk::Button {
    let button = gtk::Button::new();
    set(&button, icon, text, false);
    button
}
