//! Conversion between Kith's [`Contact`] and the People API `Person` resource (as `serde_json::Value`).
//!
//! What Google can hold is mapped both ways; what it cannot (time zone, coordinates, pronouns, keys, interests,
//! social profiles, the company-card flag, unknown vCard lines) stays local and is never touched by a sync.
//! Conversely, things Kith does not model are preserved on Google when a contact is updated: see [`merge_for_update`].
//!
//! Label vocabulary: Kith uses lowercase words ("mobile", "home fax"), Google uses camelCase codes ("homeFax") and
//! free text for custom labels. Both directions go through small tables; anything not in a table is a custom label
//! and travels unchanged.

use crate::model::{Address, Contact, Labeled, LabeledDate, Org};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// `personFields` to request when reading (everything we map, plus `metadata` for etags and deletions).
pub const PERSON_FIELDS: &str = "names,nicknames,organizations,phoneNumbers,emailAddresses,urls,addresses,imClients,birthdays,events,relations,biographies,genders,locales,userDefined,memberships,photos,metadata";
/// `updatePersonFields`: exactly the fields [`contact_to_person`] writes. Other Google fields are never in the mask,
/// so the API leaves them alone.
pub const UPDATE_PERSON_FIELDS: &str = "names,nicknames,organizations,phoneNumbers,emailAddresses,urls,addresses,imClients,birthdays,events,relations,biographies,genders,locales,userDefined,memberships";
/// Everything in the person is in the "My Contacts" group; it is not a label.
pub const GROUP_MY_CONTACTS: &str = "contactGroups/myContacts";
/// The system group behind the star: ↔ `Contact::favorite`.
pub const GROUP_STARRED: &str = "contactGroups/starred";

// ───────────────────────────── label tables: (Google code, Kith label) ─────────────────────────────

const PHONE_TYPES: &[(&str, &str)] = &[
    ("mobile", "mobile"),
    ("home", "home"),
    ("work", "work"),
    ("homeFax", "home fax"),
    ("workFax", "work fax"),
    ("otherFax", "other fax"),
    ("pager", "pager"),
    ("workMobile", "work mobile"),
    ("workPager", "work pager"),
    ("main", "main"),
    ("googleVoice", "google voice"),
    ("other", "other"),
];
const EMAIL_TYPES: &[(&str, &str)] = &[("home", "home"), ("work", "work"), ("other", "other"), ("mobile", "mobile")];
const URL_TYPES: &[(&str, &str)] = &[
    ("homePage", "homepage"),
    ("blog", "blog"),
    ("profile", "profile"),
    ("home", "home"),
    ("work", "work"),
    ("ftp", "ftp"),
    ("reservations", "reservations"),
    ("appInstallPage", "app install page"),
    ("other", "other"),
];
const ADDRESS_TYPES: &[(&str, &str)] = &[("home", "home"), ("work", "work"), ("other", "other")];
const IM_PROTOCOLS: &[(&str, &str)] = &[
    ("aim", "aim"),
    ("msn", "msn"),
    ("yahoo", "yahoo"),
    ("skype", "skype"),
    ("qq", "qq"),
    ("googleTalk", "google talk"),
    ("icq", "icq"),
    ("jabber", "jabber"),
    ("netMeeting", "netmeeting"),
];
const RELATION_TYPES: &[(&str, &str)] = &[
    ("spouse", "spouse"),
    ("child", "child"),
    ("mother", "mother"),
    ("father", "father"),
    ("parent", "parent"),
    ("brother", "brother"),
    ("sister", "sister"),
    ("friend", "friend"),
    ("relative", "relative"),
    ("domesticPartner", "domestic partner"),
    ("manager", "manager"),
    ("assistant", "assistant"),
    ("referredBy", "referred by"),
    ("partner", "partner"),
];
const EVENT_TYPES: &[(&str, &str)] = &[("anniversary", "anniversary"), ("other", "other")];

fn label_from_google(table: &[(&str, &str)], code: &str) -> String {
    let code = code.trim();
    table.iter().find(|(g, _)| *g == code).map(|(_, k)| (*k).to_string()).unwrap_or_else(|| code.to_string())
}

fn label_to_google(table: &[(&str, &str)], label: &str) -> Option<String> {
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    let lower = label.to_lowercase();
    Some(table.iter().find(|(_, k)| *k == lower).map(|(g, _)| (*g).to_string()).unwrap_or_else(|| label.to_string()))
}

// ───────────────────────────── small JSON helpers ─────────────────────────────

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn list<'a>(person: &'a Value, key: &str) -> &'a [Value] {
    person.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn is_primary(item: &Value) -> bool {
    let m = item.get("metadata");
    m.and_then(|m| m.get("primary")).and_then(Value::as_bool).unwrap_or(false)
        || m.and_then(|m| m.get("sourcePrimary")).and_then(Value::as_bool).unwrap_or(false)
}

/// The item Kith treats as "the" one for singleton fields: the primary one, else the first.
fn primary_item(items: &[Value]) -> Option<&Value> {
    items.iter().find(|i| is_primary(i)).or_else(|| items.first())
}

fn put(m: &mut Map<String, Value>, key: &str, value: &str) {
    if !value.trim().is_empty() {
        m.insert(key.into(), Value::String(value.to_string()));
    }
}

fn put_type(m: &mut Map<String, Value>, table: &[(&str, &str)], label: &str) {
    if let Some(t) = label_to_google(table, label) {
        m.insert("type".into(), Value::String(t));
    }
}

fn labeled_from(items: &[Value], value_key: &str, type_key: &str, table: &[(&str, &str)]) -> Vec<Labeled> {
    let many = items.len() > 1;
    items
        .iter()
        .filter_map(|i| {
            let value = text(i, value_key);
            if value.trim().is_empty() {
                return None;
            }
            Some(Labeled { label: label_from_google(table, &text(i, type_key)), value, pref: many && is_primary(i) })
        })
        .collect()
}

fn date_from(d: &Value) -> Option<(Option<u16>, u8, u8)> {
    let year = d.get("year").and_then(Value::as_u64).and_then(|y| u16::try_from(y).ok()).filter(|y| *y > 0);
    let month = d.get("month").and_then(Value::as_u64).and_then(|m| u8::try_from(m).ok()).unwrap_or(0);
    let day = d.get("day").and_then(Value::as_u64).and_then(|m| u8::try_from(m).ok()).unwrap_or(0);
    if year.is_none() && month == 0 && day == 0 {
        None
    } else {
        Some((year, month, day))
    }
}

fn date_to(d: &LabeledDate) -> Value {
    let mut m = Map::new();
    if let Some(y) = d.year {
        m.insert("year".into(), json!(y));
    }
    if d.month > 0 {
        m.insert("month".into(), json!(d.month));
    }
    if d.day > 0 {
        m.insert("day".into(), json!(d.day));
    }
    Value::Object(m)
}

fn sort_dedup_ci(v: &mut Vec<String>) {
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
}

// ───────────────────────────── Person → Contact ─────────────────────────────

/// Builds a [`Contact`] from a People API person. `group_names` maps a user label's resource name
/// (`contactGroups/123`) to its name; only these become categories (system groups other than the star are ignored).
/// The result has an empty `id`, zero timestamps and no photo: the sync fills those in.
pub fn person_to_contact(person: &Value, group_names: &HashMap<String, String>) -> Contact {
    let mut c = Contact::default();

    if let Some(n) = primary_item(list(person, "names")) {
        c.name.prefix = text(n, "honorificPrefix");
        c.name.given = text(n, "givenName");
        c.name.middle = text(n, "middleName");
        c.name.family = text(n, "familyName");
        c.name.suffix = text(n, "honorificSuffix");
        c.name.phonetic_given = text(n, "phoneticGivenName");
        c.name.phonetic_middle = text(n, "phoneticMiddleName");
        c.name.phonetic_family = text(n, "phoneticFamilyName");
    }

    for n in list(person, "nicknames") {
        let v = text(n, "value");
        if v.trim().is_empty() {
            continue;
        }
        if text(n, "type") == "MAIDEN_NAME" {
            if c.name.maiden.is_empty() {
                c.name.maiden = v;
            }
        } else {
            c.nicknames.push(v);
        }
    }

    for o in list(person, "organizations") {
        let org = Org { company: text(o, "name"), department: text(o, "department"), title: text(o, "title"), phonetic: text(o, "phoneticName") };
        if org != Org::default() {
            c.orgs.push(org);
        }
    }

    c.phones = labeled_from(list(person, "phoneNumbers"), "value", "type", PHONE_TYPES);
    c.emails = labeled_from(list(person, "emailAddresses"), "value", "type", EMAIL_TYPES);
    c.urls = labeled_from(list(person, "urls"), "value", "type", URL_TYPES);
    c.ims = list(person, "imClients")
        .iter()
        .filter_map(|i| {
            let value = text(i, "username");
            if value.trim().is_empty() {
                return None;
            }
            Some(Labeled { label: label_from_google(IM_PROTOCOLS, &text(i, "protocol")), value, pref: false })
        })
        .collect();

    let many_addr = list(person, "addresses").len() > 1;
    for a in list(person, "addresses") {
        let mut addr = Address {
            label: label_from_google(ADDRESS_TYPES, &text(a, "type")),
            po_box: text(a, "poBox"),
            extended: text(a, "extendedAddress"),
            street: text(a, "streetAddress"),
            city: text(a, "city"),
            region: text(a, "region"),
            postcode: text(a, "postalCode"),
            country: text(a, "country"),
            pref: many_addr && is_primary(a),
        };
        if addr.is_empty() {
            // An address Google only holds as one free-text line: keep it rather than lose it.
            let formatted = text(a, "formattedValue");
            if formatted.trim().is_empty() {
                continue;
            }
            addr.street = formatted;
        }
        c.addresses.push(addr);
    }

    if let Some(b) = primary_item(list(person, "birthdays")) {
        if let Some((year, month, day)) = b.get("date").and_then(date_from) {
            c.dates.push(LabeledDate { label: "birthday".into(), year, month, day });
        }
    }
    for e in list(person, "events") {
        if let Some((year, month, day)) = e.get("date").and_then(date_from) {
            c.dates.push(LabeledDate { label: label_from_google(EVENT_TYPES, &text(e, "type")), year, month, day });
        }
    }

    c.relations = list(person, "relations")
        .iter()
        .filter_map(|r| {
            let value = text(r, "person");
            (!value.trim().is_empty()).then(|| Labeled { label: label_from_google(RELATION_TYPES, &text(r, "type")), value, pref: false })
        })
        .collect();

    c.notes = list(person, "biographies").iter().map(|b| text(b, "value")).filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n");

    if let Some(g) = primary_item(list(person, "genders")) {
        c.gender = text(g, "value");
    }
    c.languages = list(person, "locales").iter().map(|l| text(l, "value")).filter(|s| !s.is_empty()).collect();
    c.custom = list(person, "userDefined")
        .iter()
        .filter_map(|u| {
            let value = text(u, "value");
            (!value.trim().is_empty()).then(|| Labeled { label: text(u, "key"), value, pref: false })
        })
        .collect();

    for m in list(person, "memberships") {
        let Some(res) = m.get("contactGroupMembership").and_then(|g| g.get("contactGroupResourceName")).and_then(Value::as_str) else {
            continue;
        };
        if res == GROUP_STARRED {
            c.favorite = true;
        } else if let Some(name) = group_names.get(res) {
            c.categories.push(name.clone());
        }
    }
    sort_dedup_ci(&mut c.categories);
    c
}

/// URL of the contact's own (non-default) photo, sized for Kith (about 256 px), if there is one.
pub fn photo_url(person: &Value) -> Option<String> {
    let photos = list(person, "photos");
    let real = |p: &&Value| p.get("default").and_then(Value::as_bool) != Some(true) && !text(p, "url").is_empty();
    let p = photos.iter().filter(real).find(|p| is_primary(p)).or_else(|| photos.iter().find(real))?;
    Some(sized_photo_url(&text(p, "url"), 256))
}

/// Asks Google's image server for a given size: `…=s100` / `…=s100-c` and `…?sz=50` styles are rewritten, other URLs are left alone.
pub fn sized_photo_url(url: &str, size: u32) -> String {
    if let Some(pos) = url.rfind("=s") {
        let rest = &url[pos + 2..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && matches!(&rest[digits..], "" | "-c") {
            return format!("{}=s{}{}", &url[..pos], size, &rest[digits..]);
        }
    }
    if let Some(pos) = url.find("?sz=") {
        let rest = &url[pos + 4..];
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            return format!("{}?sz={}{}", &url[..pos], size, &rest[digits..]);
        }
    }
    url.to_string()
}

// ───────────────────────────── Contact → Person ─────────────────────────────

/// Builds the writable part of a People API person from a contact. `group_resources` maps a label name to its
/// resource name (`contactGroups/123`), compared case-insensitively; categories without a group are skipped (the sync
/// creates missing groups first). Empty values are dropped, singleton fields (name, birthday, notes, gender) are
/// only emitted when set, so putting this in an update mask clears what the user removed.
pub fn contact_to_person(c: &Contact, group_resources: &HashMap<String, String>) -> Value {
    let mut p = Map::new();

    let n = &c.name;
    let mut name = Map::new();
    put(&mut name, "honorificPrefix", &n.prefix);
    put(&mut name, "givenName", &n.given);
    put(&mut name, "middleName", &n.middle);
    put(&mut name, "familyName", &n.family);
    put(&mut name, "honorificSuffix", &n.suffix);
    put(&mut name, "phoneticGivenName", &n.phonetic_given);
    put(&mut name, "phoneticMiddleName", &n.phonetic_middle);
    put(&mut name, "phoneticFamilyName", &n.phonetic_family);
    if !name.is_empty() {
        p.insert("names".into(), json!([name]));
    }

    let mut nicks: Vec<Value> = c.nicknames.iter().filter(|s| !s.trim().is_empty()).map(|s| json!({ "value": s })).collect();
    if !n.maiden.trim().is_empty() {
        nicks.push(json!({ "value": n.maiden, "type": "MAIDEN_NAME" }));
    }
    set_list(&mut p, "nicknames", nicks);

    let orgs = c
        .orgs
        .iter()
        .filter(|o| **o != Org::default())
        .map(|o| {
            let mut m = Map::new();
            put(&mut m, "name", &o.company);
            put(&mut m, "department", &o.department);
            put(&mut m, "title", &o.title);
            put(&mut m, "phoneticName", &o.phonetic);
            Value::Object(m)
        })
        .collect();
    set_list(&mut p, "organizations", orgs);

    set_list(&mut p, "phoneNumbers", labeled_to(&c.phones, "value", PHONE_TYPES, "type"));
    set_list(&mut p, "emailAddresses", labeled_to(&c.emails, "value", EMAIL_TYPES, "type"));
    set_list(&mut p, "urls", labeled_to(&c.urls, "value", URL_TYPES, "type"));
    set_list(&mut p, "imClients", labeled_to(&c.ims, "username", IM_PROTOCOLS, "protocol"));

    let addresses = c
        .addresses
        .iter()
        .filter(|a| !a.is_empty())
        .map(|a| {
            let mut m = Map::new();
            put_type(&mut m, ADDRESS_TYPES, &a.label);
            put(&mut m, "streetAddress", &a.street);
            put(&mut m, "extendedAddress", &a.extended);
            put(&mut m, "poBox", &a.po_box);
            put(&mut m, "city", &a.city);
            put(&mut m, "region", &a.region);
            put(&mut m, "postalCode", &a.postcode);
            put(&mut m, "country", &a.country);
            Value::Object(m)
        })
        .collect();
    set_list(&mut p, "addresses", addresses);

    // Google allows one birthday; further "birthday" dates travel as custom events so nothing is lost.
    let mut events = vec![];
    let mut birthday_done = false;
    for d in &c.dates {
        let empty = d.year.is_none() && d.month == 0 && d.day == 0;
        if empty {
            continue;
        }
        if d.label == "birthday" && !birthday_done {
            birthday_done = true;
            p.insert("birthdays".into(), json!([{ "date": date_to(d) }]));
        } else {
            let mut m = Map::new();
            m.insert("date".into(), date_to(d));
            put_type(&mut m, EVENT_TYPES, &d.label);
            events.push(Value::Object(m));
        }
    }
    set_list(&mut p, "events", events);

    let relations = c
        .relations
        .iter()
        .filter(|r| !r.value.trim().is_empty())
        .map(|r| {
            let mut m = Map::new();
            put(&mut m, "person", &r.value);
            put_type(&mut m, RELATION_TYPES, &r.label);
            Value::Object(m)
        })
        .collect();
    set_list(&mut p, "relations", relations);

    if !c.notes.trim().is_empty() {
        p.insert("biographies".into(), json!([{ "value": c.notes }]));
    }
    if !c.gender.trim().is_empty() {
        p.insert("genders".into(), json!([{ "value": c.gender }]));
    }
    set_list(&mut p, "locales", c.languages.iter().filter(|s| !s.trim().is_empty()).map(|s| json!({ "value": s })).collect());
    let user_defined = c
        .custom
        .iter()
        .filter(|u| !u.value.trim().is_empty())
        .map(|u| {
            let mut m = Map::new();
            put(&mut m, "key", &u.label);
            put(&mut m, "value", &u.value);
            Value::Object(m)
        })
        .collect();
    set_list(&mut p, "userDefined", user_defined);

    // Memberships: "My Contacts" always (Google rejects an update that leaves a contact with none), the star, labels.
    let lookup: HashMap<String, &String> = group_resources.iter().map(|(k, v)| (k.to_lowercase(), v)).collect();
    let mut memberships = vec![membership(GROUP_MY_CONTACTS)];
    if c.favorite {
        memberships.push(membership(GROUP_STARRED));
    }
    let mut seen: Vec<&str> = vec![];
    for cat in &c.categories {
        if let Some(res) = lookup.get(&cat.trim().to_lowercase()) {
            if !seen.contains(&res.as_str()) {
                seen.push(res.as_str());
                memberships.push(membership(res));
            }
        }
    }
    p.insert("memberships".into(), Value::Array(memberships));

    Value::Object(p)
}

fn membership(resource: &str) -> Value {
    json!({ "contactGroupMembership": { "contactGroupResourceName": resource } })
}

fn set_list(p: &mut Map<String, Value>, key: &str, items: Vec<Value>) {
    if !items.is_empty() {
        p.insert(key.into(), Value::Array(items));
    }
}

fn labeled_to(items: &[Labeled], value_key: &str, table: &[(&str, &str)], type_key: &str) -> Vec<Value> {
    items
        .iter()
        .filter(|l| !l.value.trim().is_empty())
        .map(|l| {
            let mut m = Map::new();
            put(&mut m, value_key, &l.value);
            put_type(&mut m, table, &l.label);
            if type_key != "type" {
                if let Some(t) = m.remove("type") {
                    m.insert(type_key.into(), t);
                }
            }
            Value::Object(m)
        })
        .collect()
}

/// The contact as Google would see it, with labels named by themselves; used to hash content for change detection.
/// Order-insensitive for labels, blind to everything Google cannot hold.
pub fn google_view(c: &Contact) -> Value {
    let mut c = c.clone();
    sort_dedup_ci(&mut c.categories);
    let identity: HashMap<String, String> = c.categories.iter().map(|n| (n.clone(), n.clone())).collect();
    contact_to_person(&c, &identity)
}

// ───────────────────────────── applying a remote contact onto a local one ─────────────────────────────

/// Overwrites on `local` every field Google can hold with the remote value and keeps everything else
/// (id, timestamps, time zone, geo, pronouns, keys, interests, social profiles, kind, unknown vCard lines,
/// and the photo when the remote has none).
pub fn apply_remote(local: &mut Contact, remote: Contact) {
    local.name = remote.name;
    local.nicknames = remote.nicknames;
    local.orgs = remote.orgs;
    local.phones = remote.phones;
    local.emails = remote.emails;
    local.urls = remote.urls;
    local.ims = remote.ims;
    local.addresses = remote.addresses;
    local.dates = remote.dates;
    local.relations = remote.relations;
    local.notes = remote.notes;
    local.gender = remote.gender;
    local.languages = remote.languages;
    local.custom = remote.custom;
    local.categories = remote.categories;
    local.favorite = remote.favorite;
    if remote.photo.is_some() {
        local.photo = remote.photo;
    }
}

// ───────────────────────────── preparing an update without losing Google-only data ─────────────────────────────

/// (list field, fields that identify an item, fields Kith does not model but Google keeps on the item).
const PRESERVE: &[(&str, &[&str], &[&str])] = &[
    ("emailAddresses", &["value"], &["displayName"]),
    (
        "organizations",
        &["name", "title", "department"],
        &["type", "startDate", "endDate", "current", "domain", "symbol", "location", "jobDescription", "costCenter", "fullTimeEquivalentMillipercent"],
    ),
    ("imClients", &["username"], &["type"]),
    ("nicknames", &["value"], &["type"]),
    ("biographies", &["value"], &["contentType"]),
    ("addresses", &["streetAddress", "city", "postalCode", "country"], &["countryCode"]),
    ("genders", &["value"], &["addressMeAs"]),
];

fn identity(item: &Value, keys: &[&str]) -> String {
    keys.iter().map(|k| text(item, k).trim().to_lowercase()).collect::<Vec<_>>().join("\u{1}")
}

/// Turns `ours` (from [`contact_to_person`]) into the body of an `updateContact` call: starts from the freshly read
/// `remote` person so that (a) the etag and contact source are the current ones, (b) sub-fields of an item that
/// Kith does not model (organization dates, e-mail display names, nickname types, …) survive, and (c) memberships
/// in Google system groups other than "My Contacts" and the star are kept. Labels in `group_resources` follow `ours`.
pub fn merge_for_update(remote: &Value, ours: Value, group_resources: &HashMap<String, String>) -> Value {
    let mut out = match ours {
        Value::Object(m) => m,
        _ => Map::new(),
    };

    for (field, keys, preserved) in PRESERVE {
        let Some(Value::Array(items)) = out.get_mut(*field) else { continue };
        let old = list(remote, field);
        for item in items.iter_mut() {
            let id = identity(item, keys);
            let Some(prev) = old.iter().find(|o| identity(o, keys) == id) else { continue };
            if let Value::Object(m) = item {
                for k in *preserved {
                    if !m.contains_key(*k) {
                        if let Some(v) = prev.get(*k) {
                            m.insert((*k).to_string(), v.clone());
                        }
                    }
                }
            }
        }
    }
    // names: the phonetic extras Kith has no field for
    if let Some(Value::Array(items)) = out.get_mut("names") {
        if let (Some(Value::Object(m)), Some(prev)) = (items.first_mut(), primary_item(list(remote, "names"))) {
            for k in ["phoneticFullName", "phoneticHonorificPrefix", "phoneticHonorificSuffix"] {
                if let Some(v) = prev.get(k) {
                    m.entry(k.to_string()).or_insert_with(|| v.clone());
                }
            }
        }
    }

    // memberships: keep ones we do not manage
    let managed: Vec<&str> = group_resources.values().map(String::as_str).chain([GROUP_MY_CONTACTS, GROUP_STARRED]).collect();
    if let Some(Value::Array(ms)) = out.get_mut("memberships") {
        for m in list(remote, "memberships") {
            let res = m.get("contactGroupMembership").and_then(|g| g.get("contactGroupResourceName")).and_then(Value::as_str);
            if let Some(res) = res {
                if !managed.contains(&res) {
                    ms.push(membership(res));
                }
            }
        }
    }

    // etag / contact source from the fresh read
    if let Some(e) = remote.get("etag") {
        out.insert("etag".into(), e.clone());
    }
    let sources: Vec<Value> = remote
        .get("metadata")
        .and_then(|m| m.get("sources"))
        .and_then(Value::as_array)
        .map(|s| s.iter().filter(|x| text(x, "type") == "CONTACT").map(|x| json!({ "type": "CONTACT", "id": x.get("id").cloned().unwrap_or(Value::Null), "etag": x.get("etag").cloned().unwrap_or(Value::Null) })).collect())
        .unwrap_or_default();
    if !sources.is_empty() {
        out.insert("metadata".into(), json!({ "sources": sources }));
    }
    Value::Object(out)
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn groups() -> HashMap<String, String> {
        [("contactGroups/111".to_string(), "Family".to_string()), ("contactGroups/222".to_string(), "Work friends".to_string())].into()
    }

    fn resources() -> HashMap<String, String> {
        groups().into_iter().map(|(r, n)| (n, r)).collect()
    }

    /// Roughly what people.connections.list returns for a well-filled contact.
    fn full_person() -> Value {
        json!({
          "resourceName": "people/c1234567890",
          "etag": "%EgUBAj0DNy4=",
          "metadata": { "sources": [ { "type": "CONTACT", "id": "1234567890", "etag": "#abc", "updateTime": "2024-05-01T10:00:00.000Z" } ], "objectType": "PERSON" },
          "names": [ {
            "metadata": { "primary": true, "source": { "type": "CONTACT", "id": "1234567890" } },
            "displayName": "Dr. Jane Q. Doe Jr.", "displayNameLastFirst": "Doe, Dr. Jane Q. Jr.", "unstructuredName": "Dr. Jane Q. Doe Jr.",
            "familyName": "Doe", "givenName": "Jane", "middleName": "Q.", "honorificPrefix": "Dr.", "honorificSuffix": "Jr.",
            "phoneticFamilyName": "Dough", "phoneticGivenName": "Jayn", "phoneticMiddleName": "Kew" } ],
          "nicknames": [ { "value": "JD" }, { "value": "Smith", "type": "MAIDEN_NAME" } ],
          "organizations": [ { "name": "Acme Corp", "department": "R&D", "title": "Engineer", "phoneticName": "Ak-mee", "startDate": {"year": 2020}, "current": true } ],
          "phoneNumbers": [
            { "metadata": { "primary": true }, "value": "+1 555 0100", "canonicalForm": "+15550100", "type": "mobile", "formattedType": "Mobile" },
            { "value": "+1 555 0101", "type": "homeFax", "formattedType": "Home Fax" },
            { "value": "+1 555 0102", "type": "Night shift", "formattedType": "Night shift" } ],
          "emailAddresses": [ { "value": "jane@example.com", "type": "work", "formattedType": "Work", "displayName": "Jane" }, { "value": "jane@home.example" } ],
          "urls": [ { "value": "https://jane.example", "type": "homePage" }, { "value": "https://blog.example", "type": "blog" } ],
          "addresses": [ { "formattedValue": "1 Main St\nSpringfield, IL 62701\nUSA", "type": "home", "streetAddress": "1 Main St", "extendedAddress": "Apt 2", "poBox": "PO 9",
                           "city": "Springfield", "region": "IL", "postalCode": "62701", "country": "USA", "countryCode": "US" } ],
          "imClients": [ { "username": "janedoe", "protocol": "skype", "type": "home" }, { "username": "@jane", "protocol": "Telegram" } ],
          "birthdays": [ { "date": { "month": 3, "day": 14 } } ],
          "events": [ { "date": { "year": 2010, "month": 6, "day": 5 }, "type": "anniversary" }, { "date": { "year": 1999, "month": 1, "day": 2 }, "type": "Graduation" } ],
          "relations": [ { "person": "John Doe", "type": "spouse" }, { "person": "Kim", "type": "domesticPartner" } ],
          "biographies": [ { "value": "Met at the conference.\nLoves tea.", "contentType": "TEXT_PLAIN" } ],
          "genders": [ { "value": "female", "formattedValue": "Female" } ],
          "locales": [ { "value": "en" }, { "value": "uk" } ],
          "userDefined": [ { "key": "Passport", "value": "X123" } ],
          "memberships": [
            { "contactGroupMembership": { "contactGroupId": "myContacts", "contactGroupResourceName": "contactGroups/myContacts" } },
            { "contactGroupMembership": { "contactGroupResourceName": "contactGroups/starred" } },
            { "contactGroupMembership": { "contactGroupResourceName": "contactGroups/222" } },
            { "contactGroupMembership": { "contactGroupResourceName": "contactGroups/111" } },
            { "contactGroupMembership": { "contactGroupResourceName": "contactGroups/friends" } } ],
          "photos": [ { "metadata": { "primary": true }, "url": "https://lh3.googleusercontent.com/contacts/AAAA=s100" } ]
        })
    }

    #[test]
    fn full_person_maps_every_field() {
        let c = person_to_contact(&full_person(), &groups());
        assert_eq!(c.name.prefix, "Dr.");
        assert_eq!(c.name.given, "Jane");
        assert_eq!(c.name.middle, "Q.");
        assert_eq!(c.name.family, "Doe");
        assert_eq!(c.name.suffix, "Jr.");
        assert_eq!(c.name.maiden, "Smith");
        assert_eq!(c.name.phonetic_given, "Jayn");
        assert_eq!(c.name.phonetic_middle, "Kew");
        assert_eq!(c.name.phonetic_family, "Dough");
        assert_eq!(c.nicknames, vec!["JD"]);
        assert_eq!(c.orgs, vec![Org { company: "Acme Corp".into(), department: "R&D".into(), title: "Engineer".into(), phonetic: "Ak-mee".into() }]);
        let phones: Vec<_> = c.phones.iter().map(|p| (p.label.as_str(), p.value.as_str(), p.pref)).collect();
        assert_eq!(phones, vec![("mobile", "+1 555 0100", true), ("home fax", "+1 555 0101", false), ("Night shift", "+1 555 0102", false)]);
        assert_eq!(c.emails[0], Labeled { label: "work".into(), value: "jane@example.com".into(), pref: false });
        assert_eq!(c.emails[1].label, "");
        assert_eq!(c.urls[0].label, "homepage");
        assert_eq!(c.urls[1].label, "blog");
        assert_eq!(c.addresses.len(), 1);
        let a = &c.addresses[0];
        assert_eq!((a.label.as_str(), a.street.as_str(), a.extended.as_str(), a.po_box.as_str()), ("home", "1 Main St", "Apt 2", "PO 9"));
        assert_eq!((a.city.as_str(), a.region.as_str(), a.postcode.as_str(), a.country.as_str()), ("Springfield", "IL", "62701", "USA"));
        assert_eq!(c.ims, vec![Labeled::new("skype", "janedoe"), Labeled::new("Telegram", "@jane")]);
        assert_eq!(c.birthday(), Some(&LabeledDate { label: "birthday".into(), year: None, month: 3, day: 14 }));
        assert_eq!(c.dates[1], LabeledDate { label: "anniversary".into(), year: Some(2010), month: 6, day: 5 });
        assert_eq!(c.dates[2].label, "Graduation");
        assert_eq!(c.relations, vec![Labeled::new("spouse", "John Doe"), Labeled::new("domestic partner", "Kim")]);
        assert_eq!(c.notes, "Met at the conference.\nLoves tea.");
        assert_eq!(c.gender, "female");
        assert_eq!(c.languages, vec!["en", "uk"]);
        assert_eq!(c.custom, vec![Labeled::new("Passport", "X123")]);
        assert!(c.favorite);
        assert_eq!(c.categories, vec!["Family", "Work friends"]);
        assert!(c.id.is_empty() && c.photo.is_none());
    }

    #[test]
    fn minimal_and_odd_people_do_not_panic() {
        for p in [json!({}), json!({"resourceName": "people/c1"}), json!({"names": [{}], "phoneNumbers": [{"value": ""}, {}], "birthdays": [{"text": "March"}], "events": [{"date": {}}]}), json!(null), json!({"names": "x", "memberships": [{}]})] {
            let c = person_to_contact(&p, &groups());
            assert!(c.is_blank(), "{p}");
        }
    }

    #[test]
    fn address_only_formatted_value_is_kept() {
        let p = json!({"addresses": [{"formattedValue": "Somewhere 5, Lviv", "type": "work"}]});
        let c = person_to_contact(&p, &groups());
        assert_eq!(c.addresses[0].street, "Somewhere 5, Lviv");
        assert_eq!(c.addresses[0].label, "work");
    }

    #[test]
    fn dates_with_and_without_year() {
        let p = json!({"birthdays": [{"date": {"year": 1980, "month": 12, "day": 31}}], "events": [{"date": {"year": 0, "month": 2, "day": 3}, "type": "other"}]});
        let c = person_to_contact(&p, &groups());
        assert_eq!(c.dates[0].year, Some(1980));
        assert_eq!(c.dates[1], LabeledDate { label: "other".into(), year: None, month: 2, day: 3 });
        let out = contact_to_person(&c, &resources());
        assert_eq!(out["birthdays"][0]["date"], json!({"year": 1980, "month": 12, "day": 31}));
        assert_eq!(out["events"][0]["date"], json!({"month": 2, "day": 3}));
    }

    #[test]
    fn round_trip_contact_person_contact() {
        let mut c = Contact::new();
        c.name.prefix = "Prof.".into();
        c.name.given = "Олексій".into();
        c.name.middle = "Іванович".into();
        c.name.family = "Шевченко".into();
        c.name.suffix = "PhD".into();
        c.name.maiden = "Коваль".into();
        c.name.phonetic_given = "Oleksii".into();
        c.nicknames = vec!["Alex".into(), "Лесь".into()];
        c.orgs = vec![Org { company: "Skipr".into(), department: "Eng".into(), title: "CTO".into(), phonetic: "Skiper".into() }, Org { company: "Other".into(), ..Org::default() }];
        c.phones = vec![Labeled { label: "mobile".into(), value: "+380 50 000 0000".into(), pref: true }, Labeled::new("work fax", "+380 44 000"), Labeled::new("Custom thing", "123")];
        c.emails = vec![Labeled::new("home", "a@b.c"), Labeled::new("", "x@y.z")];
        c.urls = vec![Labeled::new("homepage", "https://a.b"), Labeled::new("profile", "https://p.q")];
        c.ims = vec![Labeled::new("skype", "alex"), Labeled::new("Matrix", "@a:b.c")];
        c.addresses = vec![Address { label: "home".into(), po_box: "1".into(), extended: "2".into(), street: "3 St".into(), city: "Kyiv".into(), region: "KY".into(), postcode: "01001".into(), country: "Ukraine".into(), pref: false }];
        c.dates = vec![
            LabeledDate { label: "birthday".into(), year: Some(1990), month: 5, day: 6 },
            LabeledDate { label: "anniversary".into(), year: None, month: 7, day: 8 },
            LabeledDate { label: "Moved".into(), year: Some(2015), month: 9, day: 10 },
        ];
        c.relations = vec![Labeled::new("friend", "Taras"), Labeled::new("referred by", "Ivan"), Labeled::new("Cousin", "Olena")];
        c.notes = "Line 1\nLine 2".into();
        c.gender = "male".into();
        c.languages = vec!["uk".into(), "en".into()];
        c.custom = vec![Labeled::new("Locker", "42")];
        c.favorite = true;
        c.categories = vec!["Family".into(), "Work friends".into()];

        let person = contact_to_person(&c, &resources());
        let mut back = person_to_contact(&person, &groups());
        // multi-item lists: Google marks the primary one; ours only the first phone
        back.id = c.id.clone();
        back.created = c.created;
        back.updated = c.updated;
        // the empty-label e-mail and the second org survive; pref is only read, never written
        c.phones[0].pref = false;
        assert_eq!(back, c);
    }

    #[test]
    fn local_only_fields_are_not_sent_and_not_overwritten() {
        let mut local = Contact::new();
        local.name.given = "Local".into();
        local.timezone = "Europe/Kyiv".into();
        local.geo = "50.4,30.5".into();
        local.pronouns = "he/him".into();
        local.keys = vec![Labeled::new("pgp", "ABCD")];
        local.interests = vec!["chess".into()];
        local.social = vec![Labeled::new("mastodon", "@x@y.z")];
        local.kind = "org".into();
        local.extra = vec!["X-FOO:bar".into()];
        local.photo = Some("data:image/jpeg;base64,AAAA".into());
        let person = contact_to_person(&local, &HashMap::new());
        let text = person.to_string();
        for s in ["Europe/Kyiv", "50.4", "he/him", "ABCD", "chess", "mastodon", "X-FOO", "AAAA"] {
            assert!(!text.contains(s), "{s} leaked into {text}");
        }
        let (id, created, updated) = (local.id.clone(), local.created, local.updated);
        apply_remote(&mut local, person_to_contact(&full_person(), &groups()));
        assert_eq!(local.name.given, "Jane");
        assert_eq!((local.timezone.as_str(), local.geo.as_str(), local.pronouns.as_str()), ("Europe/Kyiv", "50.4,30.5", "he/him"));
        assert_eq!(local.keys.len() + local.interests.len() + local.social.len() + local.extra.len(), 4);
        assert_eq!(local.kind, "org");
        assert_eq!(local.photo.as_deref(), Some("data:image/jpeg;base64,AAAA"));
        assert_eq!((local.id, local.created, local.updated), (id, created, updated));
    }

    #[test]
    fn apply_remote_clears_what_remote_removed() {
        let mut local = person_to_contact(&full_person(), &groups());
        let bare = person_to_contact(&json!({"names": [{"givenName": "Jane"}]}), &groups());
        apply_remote(&mut local, bare);
        assert!(local.phones.is_empty() && local.emails.is_empty() && local.categories.is_empty() && !local.favorite);
        assert!(local.notes.is_empty() && local.dates.is_empty() && local.name.family.is_empty());
    }

    #[test]
    fn memberships_unknown_groups_and_my_contacts_are_ignored() {
        let p = json!({"memberships": [
            {"contactGroupMembership": {"contactGroupResourceName": "contactGroups/myContacts"}},
            {"contactGroupMembership": {"contactGroupResourceName": "contactGroups/friends"}},
            {"contactGroupMembership": {"contactGroupResourceName": "contactGroups/999"}},
            {"domainMembership": {"inViewerDomain": true}}]});
        let c = person_to_contact(&p, &groups());
        assert!(c.categories.is_empty() && !c.favorite);
    }

    #[test]
    fn contact_to_person_memberships_and_skips() {
        let mut c = Contact::new();
        c.name.given = "A".into();
        c.favorite = true;
        c.categories = vec!["family".into(), "Unknown label".into(), "FAMILY".into()];
        c.phones = vec![Labeled::new("mobile", "")];
        let p = contact_to_person(&c, &resources());
        let ms: Vec<&str> = p["memberships"].as_array().unwrap().iter().map(|m| m["contactGroupMembership"]["contactGroupResourceName"].as_str().unwrap()).collect();
        assert_eq!(ms, vec![GROUP_MY_CONTACTS, GROUP_STARRED, "contactGroups/111"]);
        assert!(p.get("phoneNumbers").is_none());
        assert_eq!(p["names"][0]["givenName"], "A");
    }

    #[test]
    fn label_tables_both_ways() {
        let mut c = Contact::new();
        c.phones = vec![Labeled::new("Home Fax", "1"), Labeled::new("WORK", "2"), Labeled::new("my own", "3")];
        c.urls = vec![Labeled::new("homepage", "u")];
        c.relations = vec![Labeled::new("domestic partner", "p")];
        let p = contact_to_person(&c, &HashMap::new());
        let types: Vec<&str> = p["phoneNumbers"].as_array().unwrap().iter().map(|x| x["type"].as_str().unwrap()).collect();
        assert_eq!(types, vec!["homeFax", "work", "my own"]);
        assert_eq!(p["urls"][0]["type"], "homePage");
        assert_eq!(p["relations"][0]["type"], "domesticPartner");
        for (g, k) in PHONE_TYPES.iter().chain(EMAIL_TYPES).chain(URL_TYPES).chain(ADDRESS_TYPES).chain(IM_PROTOCOLS).chain(RELATION_TYPES).chain(EVENT_TYPES) {
            assert_eq!(label_to_google(&[(g, k)], k).as_deref(), Some(*g));
            assert_eq!(label_from_google(&[(g, k)], g), *k);
        }
    }

    #[test]
    fn photo_urls() {
        assert_eq!(photo_url(&full_person()).as_deref(), Some("https://lh3.googleusercontent.com/contacts/AAAA=s256"));
        let default_only = json!({"photos": [{"url": "https://lh3.googleusercontent.com/a/default-user=s100", "default": true}]});
        assert_eq!(photo_url(&default_only), None);
        assert_eq!(photo_url(&json!({})), None);
        assert_eq!(sized_photo_url("https://x/y=s64-c", 256), "https://x/y=s256-c");
        assert_eq!(sized_photo_url("https://x/photo?sz=50", 256), "https://x/photo?sz=256");
        assert_eq!(sized_photo_url("https://x/photo.jpg", 256), "https://x/photo.jpg");
        assert_eq!(sized_photo_url("https://x/y=sabc", 256), "https://x/y=sabc");
    }

    #[test]
    fn google_view_ignores_label_order_and_local_only_fields() {
        let mut a = person_to_contact(&full_person(), &groups());
        let mut b = a.clone();
        b.categories.reverse();
        b.timezone = "UTC".into();
        b.photo = Some("data:x".into());
        assert_eq!(google_view(&a), google_view(&b));
        a.notes.push('!');
        assert_ne!(google_view(&a), google_view(&b));
    }

    #[test]
    fn merge_for_update_keeps_google_only_data() {
        let remote = full_person();
        let mut c = person_to_contact(&remote, &groups());
        c.notes = "Changed".into();
        c.categories = vec!["Work friends".into()];
        let ours = contact_to_person(&c, &resources());
        let merged = merge_for_update(&remote, ours, &resources());
        // org keeps its dates, e-mail its display name, address its country code, maiden name stays typed
        assert_eq!(merged["organizations"][0]["startDate"], json!({"year": 2020}));
        assert_eq!(merged["organizations"][0]["current"], true);
        assert_eq!(merged["emailAddresses"][0]["displayName"], "Jane");
        assert_eq!(merged["addresses"][0]["countryCode"], "US");
        assert_eq!(merged["imClients"][0]["type"], "home");
        assert_eq!(merged["biographies"][0]["value"], "Changed");
        assert!(merged["biographies"][0].get("contentType").is_none());
        // etag and contact source come from the fresh read
        assert_eq!(merged["etag"], "%EgUBAj0DNy4=");
        assert_eq!(merged["metadata"]["sources"][0]["etag"], "#abc");
        // memberships: label Family removed, system group "friends" kept
        let ms: Vec<&str> = merged["memberships"].as_array().unwrap().iter().map(|m| m["contactGroupMembership"]["contactGroupResourceName"].as_str().unwrap()).collect();
        assert!(ms.contains(&"contactGroups/222") && ms.contains(&"contactGroups/friends") && !ms.contains(&"contactGroups/111"));
    }

    #[test]
    fn merge_does_not_preserve_changed_items() {
        let remote = full_person();
        let mut c = person_to_contact(&remote, &groups());
        c.orgs[0].company = "New Corp".into();
        let merged = merge_for_update(&remote, contact_to_person(&c, &resources()), &resources());
        assert!(merged["organizations"][0].get("startDate").is_none());
    }
}
