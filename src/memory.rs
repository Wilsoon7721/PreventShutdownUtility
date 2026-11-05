//! Remembers which PCs overrode the shutdown block

use crate::machine::Identity;

pub const FILE_NAME: &str = "PreventShutdownApp.machines.txt";
pub const OVERRIDDEN: &str = "overridden";
pub const RESPECTED: &str = "respected";

const HEADER: &str = "\
# PreventShutdownApp - machines that ignore the shutdown block.
#
# \"machine\" - lines are one PC, keyed by a hash of its firmware ID.
# \"group\" - lines match any PC whose name starts with that prefix. They are written automatically for the narrowest prefix of a PC's name, and you can add them by hand - a line keyed `SIT-` covers every computer named SIT-*.
#
# state=overridden  this PC shut down anyway, so the block does not work here
# state=respected   the block worked here
#
# Delete a line to forget it. Delete the file to forget everything.
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Machine,
    Group,
}

impl Kind {
    fn tag(self) -> &'static str {
        match self {
            Kind::Machine => "machine",
            Kind::Group => "group",
        }
    }

    fn parse(s: &str) -> Option<Kind> {
        match s {
            "machine" => Some(Kind::Machine),
            "group" => Some(Kind::Group),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    pub key: String,
    fields: Vec<(String, String)>,
}

impl Entry {
    fn new(kind: Kind, key: &str) -> Entry {
        Entry {
            kind,
            key: key.to_string(),
            fields: Vec::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, name: &str, value: &str) {
        match self.fields.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => *v = value.to_string(),
            None => self.fields.push((name.to_string(), value.to_string())),
        }
    }

    pub fn state(&self) -> &str {
        self.get("state").unwrap_or("")
    }

    pub fn count(&self) -> u32 {
        self.get("count").and_then(|c| c.parse().ok()).unwrap_or(1)
    }

    pub fn summary(&self) -> String {
        let state = match self.state() {
            OVERRIDDEN => "block ignored",
            RESPECTED => "block worked",
            "" => "no state",
            other => other,
        };
        let who = match self.kind {
            Kind::Machine => self.get("host").unwrap_or("(unnamed)").to_string(),
            Kind::Group => format!("{}*", self.key),
        };
        let seen = match (self.get("first"), self.get("last")) {
            (Some(f), Some(l)) if f != l => format!("{f} to {l}, {} times", self.count()),
            (Some(f), _) => f.to_string(),
            _ => "date unknown".into(),
        };
        format!("{who:<22} {state:<14} {seen}")
    }

    fn render(&self) -> String {
        let mut line = format!("{} {}", self.kind.tag(), self.key);
        for (k, v) in &self.fields {
            line.push_str(&format!(" {k}={}", v.replace(char::is_whitespace, "_")));
        }
        line
    }

    fn parse(line: &str) -> Option<Entry> {
        let mut parts = line.split_whitespace();
        let kind = Kind::parse(parts.next()?)?;
        let key = parts.next()?.to_string();
        let fields = parts
            .filter_map(|p| p.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Some(Entry { kind, key, fields })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Raw(String),
    Entry(Entry),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Memory {
    lines: Vec<Line>,
}

/// How a PC was recognised
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Via {
    /// This exact machine
    Machine,
    /// Prefix-based
    Group(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub via: Via,
    pub state: String,
    pub first: String,
    pub last: String,
    pub count: u32,
    pub host: Option<String>,
}

impl Known {
    pub fn overridden(&self) -> bool {
        self.state == OVERRIDDEN
    }

    pub fn describe(&self) -> String {
        let when = if self.count > 1 && self.first != self.last {
            format!("{} times, {} to {}", self.count, self.first, self.last)
        } else {
            format!("on {}", self.first)
        };
        match &self.via {
            Via::Machine => format!("this PC, {when}"),
            Via::Group(prefix) => {
                let who = self.host.as_deref().unwrap_or("another PC");
                format!("{who} (matched {prefix}*), {when}")
            }
        }
    }
}

impl Memory {
    pub fn empty() -> Memory {
        Memory {
            lines: HEADER.lines().map(|l| Line::Raw(l.to_string())).collect(),
        }
    }

    pub fn parse(text: &str) -> Memory {
        Memory {
            lines: text
                .lines()
                .map(|line| match Entry::parse(line) {
                    Some(e) => Line::Entry(e),
                    None => Line::Raw(line.to_string()),
                })
                .collect(),
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                Line::Raw(s) => out.push_str(s),
                Line::Entry(e) => out.push_str(&e.render()),
            }
            out.push('\n');
        }
        out
    }

    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.lines.iter().filter_map(|l| match l {
            Line::Entry(e) => Some(e),
            Line::Raw(_) => None,
        })
    }

    fn find(&mut self, kind: Kind, key: &str) -> &mut Entry {
        let at = self.lines.iter().position(
            |l| matches!(l, Line::Entry(e) if e.kind == kind && e.key.eq_ignore_ascii_case(key)),
        );
        let at = match at {
            Some(at) => at,
            None => {
                self.lines.push(Line::Entry(Entry::new(kind, key)));
                self.lines.len() - 1
            }
        };
        match &mut self.lines[at] {
            Line::Entry(e) => e,
            Line::Raw(_) => unreachable!("the index came from an entry"),
        }
    }

    pub fn lookup(&self, id: &Identity) -> Option<Known> {
        let known = |e: &Entry, via: Via| Known {
            via,
            state: e.state().to_string(),
            first: e.get("first").unwrap_or("?").to_string(),
            last: e.get("last").unwrap_or("?").to_string(),
            count: e.count(),
            host: e.get("host").map(str::to_string),
        };

        if let Some(e) = self
            .entries()
            .find(|e| e.kind == Kind::Machine && e.key.eq_ignore_ascii_case(&id.key))
        {
            return Some(known(e, Via::Machine));
        }

        self.entries()
            .filter(|e| e.kind == Kind::Group && !e.key.is_empty())
            .filter(|e| {
                let prefix = e.key.to_ascii_uppercase();
                id.host.starts_with(&prefix) && id.host.len() > prefix.len()
            })
            .max_by_key(|e| e.key.len())
            .map(|e| known(e, Via::Group(e.key.clone())))
    }

    pub fn record(&mut self, id: &Identity, state: &str, today: &str, build: Option<&str>) {
        let group = id.group().map(str::to_string);
        for (kind, key) in [(Kind::Machine, Some(id.key.clone())), (Kind::Group, group)] {
            let Some(key) = key else { continue };
            let entry = self.find(kind, &key);
            let repeat = entry.state() == state;
            let count = if repeat { entry.count() + 1 } else { 1 };
            if !repeat || entry.get("first").is_none() {
                entry.set("first", today);
            }
            entry.set("state", state);
            entry.set("last", today);
            entry.set("count", &count.to_string());
            entry.set("host", &id.host);
            if kind == Kind::Machine {
                entry.set("id", id.source);
                if let Some(build) = build {
                    entry.set("build", build);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(host: &str, key: &str) -> Identity {
        Identity {
            key: key.to_string(),
            source: "uuid",
            host: host.to_ascii_uppercase(),
            prefixes: crate::machine::host_prefixes(host),
        }
    }

    #[test]
    fn records_machine_and_room() {
        let mut m = Memory::empty();
        let pc = id("SIT-L424-PC505", "aaaa");
        m.record(&pc, OVERRIDDEN, "2026-11-05", Some("26200"));

        let machine = m.entries().find(|e| e.kind == Kind::Machine).unwrap();
        assert_eq!(machine.key, "aaaa");
        assert_eq!(machine.state(), OVERRIDDEN);
        assert_eq!(machine.get("host"), Some("SIT-L424-PC505"));
        assert_eq!(machine.get("build"), Some("26200"));
        assert_eq!(machine.count(), 1);

        let group = m.entries().find(|e| e.kind == Kind::Group).unwrap();
        assert_eq!(group.key, "SIT-L424-");
        assert_eq!(group.get("build"), None);
    }

    #[test]
    fn one_pc_warns_for_the_rest_of_the_room() {
        let mut m = Memory::empty();
        m.record(
            &id("SIT-L424-PC505", "aaaa"),
            OVERRIDDEN,
            "2026-11-05",
            None,
        );

        let known = m.lookup(&id("SIT-L424-PC507", "bbbb")).expect("same room");
        assert_eq!(known.via, Via::Group("SIT-L424-".into()));
        assert!(known.overridden());
        assert_eq!(known.host.as_deref(), Some("SIT-L424-PC505"));
        assert!(
            known.describe().contains("SIT-L424-PC505"),
            "{}",
            known.describe()
        );

        assert_eq!(m.lookup(&id("SIT-L501-PC100", "cccc")), None);
        assert_eq!(m.lookup(&id("HOME-DESKTOP", "dddd")), None);
    }

    #[test]
    fn a_hand_written_prefix_covers_the_whole_site() {
        let mut m =
            Memory::parse("group SIT- state=overridden first=2026-11-05 last=2026-11-05 count=1\n");
        let known = m.lookup(&id("SIT-L999-PC001", "zzzz")).expect("site match");
        assert_eq!(known.via, Via::Group("SIT-".into()));

        // The narrower record wins once it exists
        m.record(
            &id("SIT-L424-PC505", "aaaa"),
            OVERRIDDEN,
            "2026-11-06",
            None,
        );
        let known = m.lookup(&id("SIT-L424-PC777", "yyyy")).unwrap();
        assert_eq!(known.via, Via::Group("SIT-L424-".into()));

        let known = m.lookup(&id("SIT-L424-PC505", "aaaa")).unwrap();
        assert_eq!(known.via, Via::Machine);
    }

    #[test]
    fn prefix_must_not_match_the_whole_name() {
        let m = Memory::parse("group SIT-L424-PC505 state=overridden\n");
        assert_eq!(m.lookup(&id("SIT-L424-PC505", "aaaa")), None);
        assert!(m.lookup(&id("SIT-L424-PC505-DOCK", "aaaa")).is_some());
    }

    #[test]
    fn matching_is_case_insensitive() {
        let m = Memory::parse("group sit-l424- state=overridden\nmachine AAAA state=overridden\n");
        assert!(m.lookup(&id("SIT-L424-PC505", "zzzz")).is_some());
        assert_eq!(m.lookup(&id("OTHER", "aaaa")).unwrap().via, Via::Machine);
    }

    #[test]
    fn repeats_bump_the_count_and_a_change_of_heart_resets_it() {
        let mut m = Memory::empty();
        let pc = id("SIT-L424-PC505", "aaaa");
        m.record(&pc, OVERRIDDEN, "2026-11-05", None);
        m.record(&pc, OVERRIDDEN, "2026-11-07", None);
        let k = m.lookup(&pc).unwrap();
        assert_eq!(
            (k.count, k.first.as_str(), k.last.as_str()),
            (2, "2026-11-05", "2026-11-07")
        );
        assert!(k.describe().contains("2 times"), "{}", k.describe());

        m.record(&pc, RESPECTED, "2026-11-09", None);
        let k = m.lookup(&pc).unwrap();
        assert!(!k.overridden());
        assert_eq!((k.count, k.first.as_str()), (1, "2026-11-09"));
    }

    #[test]
    fn a_pc_whose_name_has_no_separator_gets_no_group() {
        let mut m = Memory::empty();
        m.record(&id("LAPTOP", "aaaa"), OVERRIDDEN, "2026-11-05", None);
        assert!(m.entries().all(|e| e.kind == Kind::Machine));
        assert_eq!(m.lookup(&id("LAPTOP", "aaaa")).unwrap().via, Via::Machine);
    }

    #[test]
    fn comments_and_unknown_fields_survive_a_rewrite() {
        let text = "# my own note\nmachine aaaa state=overridden first=2026-11-05 mystery=keep\n\n";
        let mut m = Memory::parse(text);
        m.record(&id("PC1", "aaaa"), OVERRIDDEN, "2026-11-06", None);
        let out = m.render();
        assert!(out.starts_with("# my own note\n"), "{out}");
        assert!(out.contains("mystery=keep"), "{out}");
        assert!(out.contains("count=2"), "{out}");
        // One machine line, not two.
        assert_eq!(out.matches("machine aaaa").count(), 1, "{out}");
        // Parsing what we rendered gives the same thing back.
        assert_eq!(Memory::parse(&out), m);
    }

    #[test]
    fn an_empty_or_junk_file_is_harmless() {
        for text in ["", "\n\n", "garbage\nmachine\n#\n"] {
            let m = Memory::parse(text);
            assert_eq!(m.lookup(&id("PC1", "aaaa")), None, "{text:?}");
        }
    }
}
