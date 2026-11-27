//! The Checklist printed at the end of every setup run (spec §7, "The checklist").

const STANDING: [&str; 1] = ["For private dictation, use Firefox or the Rakazo desktop app."];

#[derive(Debug, Default)]
pub struct Checklist {
    items: Vec<String>,
}

impl Checklist {
    pub fn add(&mut self, item: impl Into<String>) {
        self.items.push(item.into());
    }

    pub fn items(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(String::as_str).chain(STANDING)
    }

    pub fn render(&self) -> String {
        let mut text = String::from("Checklist:\n");
        for item in self.items() {
            text.push_str(&format!("  - {item}\n"));
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::Checklist;

    #[test]
    fn an_empty_checklist_still_has_the_standing_items() {
        assert_eq!(
            Checklist::default().render(),
            "Checklist:\n  - For private dictation, use Firefox or the Rakazo desktop app.\n"
        );
    }

    #[test]
    fn items_come_before_the_standing_ones() {
        let mut checklist = Checklist::default();
        checklist.add("Do this by hand.");
        let items: Vec<_> = checklist.items().collect();
        assert_eq!(items[0], "Do this by hand.");
        assert_eq!(items.len(), 2);
    }
}
