//! The contact model. One flat, serde-friendly struct: the UI edits it as JSON, the store writes it as vCard.
//! Everything is a plain string or a small list so that nothing is lost when a field is switched off in the UI.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Name {
    pub prefix: String,
    pub given: String,
    pub middle: String,
    pub family: String,
    pub suffix: String,
    pub maiden: String,
    pub phonetic_given: String,
    pub phonetic_middle: String,
    pub phonetic_family: String,
}

/// A value with a label: phone ("mobile"), e-mail ("work"), URL, instant message (service), custom field (name)…
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Labeled {
    pub label: String,
    pub value: String,
    pub pref: bool,
}

impl Labeled {
    pub fn new(label: &str, value: &str) -> Self {
        Labeled { label: label.into(), value: value.into(), pref: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Org {
    pub company: String,
    pub department: String,
    pub title: String,
    pub phonetic: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Address {
    pub label: String,
    pub po_box: String,
    pub extended: String,
    pub street: String,
    pub city: String,
    pub region: String,
    pub postcode: String,
    pub country: String,
    pub pref: bool,
}

impl Address {
    pub fn is_empty(&self) -> bool {
        self.po_box.is_empty()
            && self.extended.is_empty()
            && self.street.is_empty()
            && self.city.is_empty()
            && self.region.is_empty()
            && self.postcode.is_empty()
            && self.country.is_empty()
    }
}

/// A date with a label ("birthday", "anniversary", or anything else). The year is optional.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct LabeledDate {
    pub label: String,
    pub year: Option<u16>,
    pub month: u8,
    pub day: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Contact {
    pub id: String,
    pub name: Name,
    pub nicknames: Vec<String>,
    pub orgs: Vec<Org>,
    pub phones: Vec<Labeled>,
    pub emails: Vec<Labeled>,
    pub urls: Vec<Labeled>,
    /// Instant messengers: label = service (Signal, Telegram, Matrix…), value = the handle.
    pub ims: Vec<Labeled>,
    /// Social profiles: label = network, value = handle or link.
    pub social: Vec<Labeled>,
    pub addresses: Vec<Address>,
    pub dates: Vec<LabeledDate>,
    /// Related people: label = spouse, child, parent, manager, assistant, friend…, value = their name.
    pub relations: Vec<Labeled>,
    /// Public keys (PGP…): label = kind, value = fingerprint or link.
    pub keys: Vec<Labeled>,
    pub notes: String,
    /// `data:image/jpeg;base64,…` (the UI scales it down before saving).
    pub photo: Option<String>,
    pub gender: String,
    pub pronouns: String,
    pub languages: Vec<String>,
    pub timezone: String,
    /// "lat,lon"
    pub geo: String,
    pub interests: Vec<String>,
    /// Labels / groups.
    pub categories: Vec<String>,
    pub favorite: bool,
    /// "individual" (default) or "org" (a company or place shown by its organisation name).
    pub kind: String,
    /// Free fields: label = the name you give it.
    pub custom: Vec<Labeled>,
    /// vCard lines Kith does not know, kept as they were so that nothing is lost on a round trip.
    pub extra: Vec<String>,
    /// Unix seconds.
    pub created: u64,
    pub updated: u64,
}

impl Default for Contact {
    fn default() -> Self {
        Contact {
            id: String::new(),
            name: Name::default(),
            nicknames: vec![],
            orgs: vec![],
            phones: vec![],
            emails: vec![],
            urls: vec![],
            ims: vec![],
            social: vec![],
            addresses: vec![],
            dates: vec![],
            relations: vec![],
            keys: vec![],
            notes: String::new(),
            photo: None,
            gender: String::new(),
            pronouns: String::new(),
            languages: vec![],
            timezone: String::new(),
            geo: String::new(),
            interests: vec![],
            categories: vec![],
            favorite: false,
            kind: String::new(),
            custom: vec![],
            extra: vec![],
            created: 0,
            updated: 0,
        }
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl Contact {
    pub fn new() -> Contact {
        let t = now();
        Contact { id: new_id(), created: t, updated: t, ..Default::default() }
    }

    pub fn is_org(&self) -> bool {
        self.kind == "org"
    }

    pub fn birthday(&self) -> Option<&LabeledDate> {
        self.dates.iter().find(|d| d.label == "birthday")
    }

    /// "Given Family" (or "Family Given"), falling back to nickname, company, e-mail, phone.
    pub fn display_name(&self, family_first: bool) -> String {
        let n = &self.name;
        let full = if family_first && !n.family.is_empty() {
            let rest = [n.given.as_str(), n.middle.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
            if rest.is_empty() { n.family.clone() } else { format!("{}, {}", n.family, rest) }
        } else {
            [n.given.as_str(), n.middle.as_str(), n.family.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ")
        };
        if self.is_org() {
            if let Some(o) = self.orgs.iter().find(|o| !o.company.is_empty()) {
                return o.company.clone();
            }
        }
        if !full.is_empty() {
            return full;
        }
        if let Some(nick) = self.nicknames.iter().find(|s| !s.is_empty()) {
            return nick.clone();
        }
        if let Some(o) = self.orgs.iter().find(|o| !o.company.is_empty()) {
            return o.company.clone();
        }
        if let Some(e) = self.emails.iter().find(|e| !e.value.is_empty()) {
            return e.value.clone();
        }
        if let Some(p) = self.phones.iter().find(|p| !p.value.is_empty()) {
            return p.value.clone();
        }
        "Unnamed".to_string()
    }

    /// True when there is nothing worth saving.
    pub fn is_blank(&self) -> bool {
        let mut c = self.clone();
        c.id.clear();
        c.created = 0;
        c.updated = 0;
        c.favorite = false;
        c.extra.clear();
        c.phones.retain(|x| !x.value.is_empty());
        c.emails.retain(|x| !x.value.is_empty());
        c.urls.retain(|x| !x.value.is_empty());
        c.ims.retain(|x| !x.value.is_empty());
        c.social.retain(|x| !x.value.is_empty());
        c.relations.retain(|x| !x.value.is_empty());
        c.keys.retain(|x| !x.value.is_empty());
        c.custom.retain(|x| !x.value.is_empty());
        c.addresses.retain(|a| !a.is_empty());
        c.orgs.retain(|o| *o != Org::default());
        c.nicknames.retain(|s| !s.is_empty());
        c.languages.retain(|s| !s.is_empty());
        c.interests.retain(|s| !s.is_empty());
        c.categories.retain(|s| !s.is_empty());
        let blank = Contact { id: String::new(), kind: c.kind.clone(), ..Contact::default() };
        c == blank
    }
}
