//! The catalog of every field Kith can show, and the templates (which fields a form shows by default).
//! A field that already holds data is always shown, whatever the template says: nothing is ever hidden away.

use crate::model::Contact;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug)]
pub struct FieldDef {
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub icon: &'static str,
    /// Always shown; cannot be switched off.
    pub required: bool,
}

macro_rules! f {
    ($k:expr, $l:expr, $g:expr, $i:expr) => {
        FieldDef { key: $k, label: $l, group: $g, icon: $i, required: false }
    };
    ($k:expr, $l:expr, $g:expr, $i:expr, req) => {
        FieldDef { key: $k, label: $l, group: $g, icon: $i, required: true }
    };
}

pub const FIELDS: &[FieldDef] = &[
    // Name
    f!("name.given", "First name", "Name", "user", req),
    f!("name.family", "Last name", "Name", "user", req),
    f!("name.prefix", "Title (Dr., Ms.)", "Name", "badge"),
    f!("name.middle", "Middle name", "Name", "user"),
    f!("name.suffix", "Suffix (Jr., PhD)", "Name", "badge"),
    f!("name.maiden", "Maiden name", "Name", "user"),
    f!("name.phonetic", "Phonetic name", "Name", "speech"),
    f!("nicknames", "Nickname", "Name", "smile"),
    // Work
    f!("org.company", "Company", "Work", "building"),
    f!("org.department", "Department", "Work", "layers"),
    f!("org.title", "Job title", "Work", "briefcase"),
    // Ways to reach
    f!("phones", "Phone", "Reach", "phone"),
    f!("emails", "Email", "Reach", "mail"),
    f!("addresses", "Address", "Reach", "map-pin"),
    f!("urls", "Website", "Reach", "globe"),
    f!("ims", "Messengers", "Reach", "message-circle"),
    f!("social", "Social profiles", "Reach", "at-sign"),
    f!("keys", "Keys (PGP)", "Reach", "key"),
    // Personal
    f!("birthday", "Birthday", "Personal", "cake"),
    f!("anniversary", "Anniversary", "Personal", "heart"),
    f!("dates.other", "Other dates", "Personal", "calendar"),
    f!("relations", "Related people", "Personal", "users"),
    f!("gender", "Gender", "Personal", "user-round"),
    f!("pronouns", "Pronouns", "Personal", "message-square"),
    f!("languages", "Languages", "Personal", "languages"),
    f!("timezone", "Time zone", "Personal", "clock"),
    f!("geo", "Location (coordinates)", "Personal", "map"),
    f!("interests", "Interests", "Personal", "sparkles"),
    // Other
    f!("notes", "Notes", "Other", "notebook-pen"),
    f!("categories", "Labels", "Other", "tag"),
    f!("custom", "Custom fields", "Other", "plus-square"),
    f!("kind", "Company card (no person)", "Other", "building-2"),
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Template {
    pub name: String,
    pub builtin: bool,
    pub fields: Vec<String>,
}

fn t(name: &str, fields: &[&str]) -> Template {
    Template { name: name.into(), builtin: true, fields: fields.iter().map(|s| s.to_string()).collect() }
}

pub fn builtin_templates() -> Vec<Template> {
    vec![
        t("Basic", &["name.given", "name.family", "org.company", "phones", "emails", "notes"]),
        t(
            "Personal",
            &["name.given", "name.family", "name.middle", "nicknames", "phones", "emails", "addresses", "birthday", "anniversary", "relations", "social", "ims", "notes", "categories"],
        ),
        t(
            "Business",
            &["name.given", "name.family", "name.prefix", "name.suffix", "org.company", "org.department", "org.title", "phones", "emails", "addresses", "urls", "social", "ims", "notes", "categories"],
        ),
        t("Everything", &FIELDS.iter().map(|f| f.key).collect::<Vec<_>>()),
    ]
}

/// Does the contact hold data for this field (so it must be shown even if the template hides it)?
pub fn has_data(key: &str, c: &Contact) -> bool {
    let n = &c.name;
    match key {
        "name.given" | "name.family" => true,
        "name.prefix" => !n.prefix.is_empty(),
        "name.middle" => !n.middle.is_empty(),
        "name.suffix" => !n.suffix.is_empty(),
        "name.maiden" => !n.maiden.is_empty(),
        "name.phonetic" => !(n.phonetic_given.is_empty() && n.phonetic_middle.is_empty() && n.phonetic_family.is_empty()),
        "nicknames" => c.nicknames.iter().any(|s| !s.is_empty()),
        "org.company" => c.orgs.iter().any(|o| !o.company.is_empty()),
        "org.department" => c.orgs.iter().any(|o| !o.department.is_empty()),
        "org.title" => c.orgs.iter().any(|o| !o.title.is_empty()),
        "phones" => c.phones.iter().any(|x| !x.value.is_empty()),
        "emails" => c.emails.iter().any(|x| !x.value.is_empty()),
        "addresses" => c.addresses.iter().any(|a| !a.is_empty()),
        "urls" => c.urls.iter().any(|x| !x.value.is_empty()),
        "ims" => c.ims.iter().any(|x| !x.value.is_empty()),
        "social" => c.social.iter().any(|x| !x.value.is_empty()),
        "keys" => c.keys.iter().any(|x| !x.value.is_empty()),
        "birthday" => c.dates.iter().any(|d| d.label == "birthday"),
        "anniversary" => c.dates.iter().any(|d| d.label == "anniversary"),
        "dates.other" => c.dates.iter().any(|d| d.label != "birthday" && d.label != "anniversary"),
        "relations" => c.relations.iter().any(|x| !x.value.is_empty()),
        "gender" => !c.gender.is_empty(),
        "pronouns" => !c.pronouns.is_empty(),
        "languages" => c.languages.iter().any(|s| !s.is_empty()),
        "timezone" => !c.timezone.is_empty(),
        "geo" => !c.geo.is_empty(),
        "interests" => c.interests.iter().any(|s| !s.is_empty()),
        "notes" => !c.notes.is_empty(),
        "categories" => c.categories.iter().any(|s| !s.is_empty()),
        "custom" => c.custom.iter().any(|x| !x.value.is_empty()),
        "kind" => c.is_org(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_only_use_known_fields() {
        for t in builtin_templates() {
            for f in &t.fields {
                assert!(FIELDS.iter().any(|d| d.key == f), "{} in {}", f, t.name);
            }
        }
        assert!(FIELDS.len() >= 30);
    }

    #[test]
    fn has_data_knows_every_field() {
        let c = Contact::new();
        for f in FIELDS {
            if !f.required {
                assert!(!has_data(f.key, &c), "{}", f.key);
            }
        }
    }
}
