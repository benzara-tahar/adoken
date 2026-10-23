//! `[tours]` in `settings.toml` (spec 0020 FR-15, 0009 FR-1): the
//! first-use switch and the ids of tours already shown.

use toml::{Table, Value};

pub const TABLE: &str = "tours";
const AUTO_START: &str = "auto_start";
const SEEN: &str = "seen";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TourSettings {
    /// Show each tour the first time its view opens.
    pub auto_start: bool,
    /// Ids shown so far, unknown ones included (a newer build may know
    /// them; they are kept on every write).
    pub seen: Vec<String>,
}

impl Default for TourSettings {
    fn default() -> Self {
        Self {
            auto_start: true,
            seen: Vec::new(),
        }
    }
}

impl TourSettings {
    /// Reads `[tours]` from the whole document, with the problems Settings
    /// reports (0009 FR-5): a bad value falls back to its default.
    pub fn parse(document: &Table) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut issues = Vec::new();
        let Some(table) = document.get(TABLE) else {
            return (settings, issues);
        };
        let Some(table) = table.as_table() else {
            issues.push(format!("{TABLE}: not a table; using the defaults"));
            return (settings, issues);
        };
        match table.get(AUTO_START) {
            None => {}
            Some(Value::Boolean(on)) => settings.auto_start = *on,
            Some(_) => issues.push(format!(
                "{TABLE}.{AUTO_START}: expected true or false; using true"
            )),
        }
        match table.get(SEEN) {
            None => {}
            Some(Value::Array(ids)) => {
                let strings: Option<Vec<String>> = ids
                    .iter()
                    .map(|id| id.as_str().map(str::to_owned))
                    .collect();
                match strings {
                    Some(ids) => settings.seen = ids,
                    None => issues.push(format!(
                        "{TABLE}.{SEEN}: every entry must be a tour id in quotes; treating it as empty"
                    )),
                }
            }
            Some(_) => issues.push(format!(
                "{TABLE}.{SEEN}: expected a list of tour ids; treating it as empty"
            )),
        }
        (settings, issues)
    }

    /// Parse the file's text; a broken file reads as the defaults.
    pub fn from_toml(text: &str) -> Self {
        toml::from_str::<Table>(text)
            .map(|document| Self::parse(&document).0)
            .unwrap_or_default()
    }

    pub fn is_seen(&self, id: &str) -> bool {
        self.seen.iter().any(|seen| seen == id)
    }

    /// Adds `id` once; whether it was new.
    pub fn mark_seen(&mut self, id: &str) -> bool {
        if self.is_seen(id) {
            return false;
        }
        self.seen.push(id.to_owned());
        true
    }

    pub fn to_table(&self) -> Table {
        let mut table = Table::new();
        table.insert(AUTO_START.into(), self.auto_start.into());
        table.insert(
            SEEN.into(),
            Value::Array(self.seen.iter().cloned().map(Value::from).collect()),
        );
        table
    }
}

/// `[tours]` problems for Settings' report (0009 FR-5).
pub fn settings_issues(document: &Table) -> Vec<String> {
    TourSettings::parse(document).1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (TourSettings, Vec<String>) {
        TourSettings::parse(&toml::from_str(text).unwrap())
    }

    #[test]
    fn missing_table_is_on_and_nothing_seen() {
        assert_eq!(parse(""), (TourSettings::default(), Vec::new()));
    }

    #[test]
    fn non_array_seen_is_empty_and_reported() {
        let (settings, issues) = parse("[tours]\nseen = 3\n");
        assert!(settings.seen.is_empty());
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("tours.seen"));
    }

    #[test]
    fn non_string_entries_empty_the_list_and_are_reported() {
        let (settings, issues) = parse("[tours]\nseen = [\"review\", 4]\n");
        assert!(settings.seen.is_empty());
        assert_eq!(issues.len(), 1);
    }

    #[test]
    fn unknown_ids_are_kept_on_the_next_write() {
        let (mut settings, issues) = parse("[tours]\nseen = [\"review\", \"future-tour\"]\n");
        assert!(issues.is_empty());
        assert!(settings.is_seen("review"));
        assert!(settings.mark_seen("inbox"));
        assert!(!settings.mark_seen("inbox"));
        let written = settings.to_table();
        let seen: Vec<&str> = written["seen"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(seen, ["review", "future-tour", "inbox"]);
    }

    #[test]
    fn bad_auto_start_keeps_tours_on() {
        let (settings, issues) = parse("[tours]\nauto_start = \"no\"\n");
        assert!(settings.auto_start);
        assert_eq!(issues.len(), 1);
        assert!(!parse("[tours]\nauto_start = false\n").0.auto_start);
    }
}
