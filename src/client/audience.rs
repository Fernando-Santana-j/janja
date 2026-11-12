use super::{
    Person,
    icons::{self, Icon},
};
use gtk::{Orientation, prelude::*};

pub struct Audience {
    pub button: gtk::Button,
    details: gtk::Label,
}

fn names<'a>(ids: &[u64], people: &'a [Person]) -> Vec<&'a str> {
    people
        .iter()
        .filter(|person| ids.contains(&person.id))
        .map(|person| person.name.as_str())
        .collect()
}

impl Audience {
    pub fn new() -> Self {
        let button = gtk::Button::new();
        button.set_no_show_all(true);
        button.set_valign(gtk::Align::Center);
        button.style_context().add_class("audience");
        icons::set(&button, Icon::People, "0 assistindo", true);
        let content = gtk::Box::new(Orientation::Vertical, 10);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.pack_start(
            &super::ui::label("Quem está assistindo", "video-name"),
            false,
            false,
            0,
        );
        let details = super::ui::label("Ninguém assistindo ainda.", "muted");
        details.set_line_wrap(true);
        details.set_max_width_chars(32);
        content.pack_start(&details, false, false, 0);
        let popover = gtk::Popover::new(Some(&button));
        popover.add(&content);
        button.connect_clicked(move |_| {
            popover.show_all();
            popover.popup();
        });
        Self { button, details }
    }

    pub fn update(&self, ids: &[u64], people: &[Person]) {
        let people = names(ids, people);
        let text = if people.is_empty() {
            "Ninguém assistindo ainda.".into()
        } else {
            people.join("\n")
        };
        if self.details.text() == text {
            return;
        }
        self.details.set_text(&text);
        icons::set(
            &self.button,
            Icon::People,
            &format!("{} assistindo", people.len()),
            true,
        );
        self.button.set_tooltip_text(Some(&text));
        if people.is_empty() {
            self.button.style_context().remove_class("has-viewers");
        } else {
            self.button.style_context().add_class("has-viewers");
        }
    }

    pub fn set_streaming(&self, active: bool) {
        if !active {
            self.update(&[], &[]);
        }
        self.button.set_visible(active);
    }
}

#[cfg(test)]
#[test]
fn audience_names_use_only_room_members_and_ignore_duplicates() {
    let people = vec![
        Person {
            id: 1,
            name: "Ana".into(),
            streaming: false,
        },
        Person {
            id: 2,
            name: "Bruno".into(),
            streaming: true,
        },
    ];
    assert_eq!(names(&[2, 2, 99], &people), ["Bruno"]);
    assert_eq!(names(&[2, 1], &people), ["Ana", "Bruno"]);
    assert!(names(&[], &people).is_empty());
}
