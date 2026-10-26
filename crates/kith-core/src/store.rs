//! The address book on disk: one `.vcf` file per contact in `<root>/contacts`, deleted ones in `<root>/trash`
//! (kept 30 days), whole-book backups in `<root>/backups`. Everything is loaded into memory at start.

use crate::model::*;
use crate::search::{digits, fold};
use crate::vcard;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Msg(String),
}
pub type Result<T> = std::result::Result<T, Error>;

pub fn default_root() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_DATA_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x).join("kith");
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".local/share/kith")
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Summary {
    pub id: String,
    pub display: String,
    pub letter: String,
    pub subtitle: String,
    pub favorite: bool,
    pub has_photo: bool,
    pub is_org: bool,
    pub updated: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Query {
    pub text: String,
    /// "all" (default), "favorites", "recent", "nolabel", or "label:<name>".
    pub group: String,
    pub sort_family: bool,
    pub family_first: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
}

pub struct Store {
    root: PathBuf,
    contacts: BTreeMap<String, Contact>,
}

fn atomic_write(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("vcf.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn same_content(a: &Contact, b: &Contact) -> bool {
    let mut x = a.clone();
    let mut y = b.clone();
    x.updated = 0;
    y.updated = 0;
    x.created = 0;
    y.created = 0;
    x == y
}

impl Store {
    pub fn open(root: impl Into<PathBuf>) -> Result<Store> {
        let root = root.into();
        for d in ["contacts", "trash", "backups"] {
            fs::create_dir_all(root.join(d))?;
        }
        let mut contacts = BTreeMap::new();
        for e in fs::read_dir(root.join("contacts"))? {
            let p = e?.path();
            if p.extension().and_then(|x| x.to_str()) != Some("vcf") {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&p) {
                for c in vcard::parse(&text) {
                    contacts.insert(c.id.clone(), c);
                }
            }
        }
        let mut s = Store { root, contacts };
        s.purge_trash(30);
        Ok(s)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn len(&self) -> usize {
        self.contacts.len()
    }
    pub fn is_empty(&self) -> bool {
        self.contacts.is_empty()
    }
    pub fn get(&self, id: &str) -> Option<&Contact> {
        self.contacts.get(id)
    }
    pub fn all(&self) -> impl Iterator<Item = &Contact> {
        self.contacts.values()
    }

    fn path_of(&self, id: &str) -> PathBuf {
        self.root.join("contacts").join(format!("{}.vcf", id))
    }

    /// Saves (creating or replacing). Stamps `updated`; makes an id for a new contact.
    pub fn save(&mut self, mut c: Contact) -> Result<Contact> {
        if c.id.is_empty() || !c.id.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch)) {
            c.id = new_id();
        }
        if c.is_blank() {
            return Err(Error::Msg("Nothing to save: the contact is empty.".into()));
        }
        let t = now();
        if let Some(old) = self.contacts.get(&c.id) {
            if c.created == 0 {
                c.created = old.created;
            }
        }
        if c.created == 0 {
            c.created = t;
        }
        c.updated = t;
        atomic_write(&self.path_of(&c.id), &vcard::write(&c))?;
        self.contacts.insert(c.id.clone(), c.clone());
        Ok(c)
    }

    /// Saves without touching `updated` (a sync stores what the server says).
    pub fn put_unchanged(&mut self, c: Contact) -> Result<()> {
        atomic_write(&self.path_of(&c.id), &vcard::write(&c))?;
        self.contacts.insert(c.id.clone(), c);
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        if let Some(c) = self.contacts.remove(id) {
            atomic_write(&self.root.join("trash").join(format!("{}.vcf", id)), &vcard::write(&c))?;
            let _ = fs::remove_file(self.path_of(id));
        }
        Ok(())
    }

    pub fn trash(&self) -> Vec<Contact> {
        let mut out = vec![];
        if let Ok(rd) = fs::read_dir(self.root.join("trash")) {
            for e in rd.flatten() {
                if let Ok(t) = fs::read_to_string(e.path()) {
                    out.extend(vcard::parse(&t));
                }
            }
        }
        out.sort_by(|a, b| a.display_name(false).cmp(&b.display_name(false)));
        out
    }

    pub fn restore(&mut self, id: &str) -> Result<()> {
        let p = self.root.join("trash").join(format!("{}.vcf", id));
        let text = fs::read_to_string(&p)?;
        for c in vcard::parse(&text) {
            self.put_unchanged(c)?;
        }
        let _ = fs::remove_file(p);
        Ok(())
    }

    pub fn empty_trash(&mut self) {
        if let Ok(rd) = fs::read_dir(self.root.join("trash")) {
            for e in rd.flatten() {
                let _ = fs::remove_file(e.path());
            }
        }
    }

    fn purge_trash(&mut self, days: u64) {
        let limit = std::time::Duration::from_secs(days * 86400);
        if let Ok(rd) = fs::read_dir(self.root.join("trash")) {
            for e in rd.flatten() {
                let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).map(|a| a > limit).unwrap_or(false);
                if old {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }

    // ── lists and search ──

    fn haystack(c: &Contact) -> String {
        let mut s = String::new();
        let mut add = |x: &str| {
            if !x.is_empty() {
                s.push_str(x);
                s.push('\n');
            }
        };
        let n = &c.name;
        for x in [&n.prefix, &n.given, &n.middle, &n.family, &n.suffix, &n.maiden, &n.phonetic_given, &n.phonetic_family] {
            add(x);
        }
        c.nicknames.iter().for_each(|x| add(x));
        for o in &c.orgs {
            add(&o.company);
            add(&o.department);
            add(&o.title);
        }
        for l in c.emails.iter().chain(&c.urls).chain(&c.ims).chain(&c.social).chain(&c.relations).chain(&c.custom) {
            add(&l.value);
        }
        for a in &c.addresses {
            for x in [&a.street, &a.city, &a.region, &a.postcode, &a.country] {
                add(x);
            }
        }
        c.categories.iter().for_each(|x| add(x));
        c.interests.iter().for_each(|x| add(x));
        add(&c.notes);
        fold(&s)
    }

    fn matches(c: &Contact, text: &str) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return true;
        }
        let hay = Self::haystack(c);
        let phones: Vec<String> = c.phones.iter().map(|p| digits(&p.value)).collect();
        text.split_whitespace().all(|tok| {
            let t = fold(tok);
            if hay.contains(&t) {
                return true;
            }
            let d = digits(tok);
            d.len() >= 3 && phones.iter().any(|p| p.contains(&d))
        })
    }

    fn in_group(c: &Contact, group: &str) -> bool {
        match group {
            "" | "all" => true,
            "favorites" => c.favorite,
            "nolabel" => c.categories.is_empty(),
            "recent" => true,
            g => match g.strip_prefix("label:") {
                Some(l) => c.categories.iter().any(|x| x == l),
                None => true,
            },
        }
    }

    pub fn list(&self, q: &Query) -> Vec<Summary> {
        let mut rows: Vec<(String, &Contact)> = self
            .contacts
            .values()
            .filter(|c| Self::in_group(c, &q.group) && Self::matches(c, &q.text))
            .map(|c| {
                let n = &c.name;
                let key = if c.is_org() || (n.given.is_empty() && n.family.is_empty()) {
                    c.display_name(false)
                } else if q.sort_family {
                    format!("{} {}", n.family, n.given)
                } else {
                    format!("{} {}", n.given, n.family)
                };
                (fold(key.trim()), c)
            })
            .collect();
        if q.group == "recent" {
            rows.sort_by(|a, b| b.1.created.cmp(&a.1.created));
            rows.truncate(50);
        } else {
            rows.sort_by(|a, b| a.0.cmp(&b.0));
        }
        rows.into_iter()
            .map(|(key, c)| {
                let letter = key.chars().next().filter(|ch| ch.is_alphabetic()).map(|ch| ch.to_uppercase().collect::<String>()).unwrap_or_else(|| "#".into());
                let subtitle = c
                    .orgs
                    .iter()
                    .find(|o| !o.company.is_empty() || !o.title.is_empty())
                    .map(|o| [o.title.as_str(), o.company.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" · "))
                    .filter(|s| !c.is_org() || s != &c.display_name(false))
                    .or_else(|| c.emails.iter().find(|e| !e.value.is_empty()).map(|e| e.value.clone()))
                    .or_else(|| c.phones.iter().find(|e| !e.value.is_empty()).map(|e| e.value.clone()))
                    .unwrap_or_default();
                Summary {
                    id: c.id.clone(),
                    display: c.display_name(q.family_first),
                    letter,
                    subtitle,
                    favorite: c.favorite,
                    has_photo: c.photo.is_some(),
                    is_org: c.is_org(),
                    updated: c.updated,
                }
            })
            .collect()
    }

    /// Labels in use with their contact counts.
    pub fn labels(&self) -> Vec<(String, usize)> {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for c in self.contacts.values() {
            for l in &c.categories {
                *m.entry(l.clone()).or_default() += 1;
            }
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by_key(|(l, _)| fold(l));
        v
    }

    pub fn rename_label(&mut self, from: &str, to: &str) -> Result<()> {
        let ids: Vec<String> = self.contacts.values().filter(|c| c.categories.iter().any(|l| l == from)).map(|c| c.id.clone()).collect();
        for id in ids {
            let mut c = self.contacts[&id].clone();
            c.categories.retain(|l| l != from);
            if !to.is_empty() && !c.categories.iter().any(|l| l == to) {
                c.categories.push(to.to_string());
            }
            self.save(c)?;
        }
        Ok(())
    }

    // ── import / export / backup ──

    pub fn import_vcf(&mut self, text: &str) -> Result<ImportReport> {
        let mut r = ImportReport::default();
        for c in vcard::parse(text) {
            if c.is_blank() {
                r.skipped += 1;
                continue;
            }
            match self.contacts.get(&c.id) {
                Some(old) if same_content(old, &c) => r.skipped += 1,
                Some(_) => {
                    self.put_unchanged(c)?;
                    r.updated += 1;
                }
                None => {
                    self.put_unchanged(c)?;
                    r.added += 1;
                }
            }
        }
        Ok(r)
    }

    pub fn export_vcf(&self, ids: Option<&[String]>) -> String {
        match ids {
            None => vcard::write_all(self.contacts.values()),
            Some(ids) => vcard::write_all(ids.iter().filter_map(|i| self.contacts.get(i))),
        }
    }

    /// Writes every contact into one `.vcf` (also what "Export all" gives) and keeps the last 10.
    pub fn backup(&self, keep: usize) -> Result<PathBuf> {
        let dir = self.root.join("backups");
        let stamp = {
            let t = now();
            format!("{}", t)
        };
        let p = dir.join(format!("{}.vcf", stamp));
        fs::write(&p, self.export_vcf(None))?;
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)?.flatten().map(|e| e.path()).filter(|p| p.extension().and_then(|x| x.to_str()) == Some("vcf")).collect();
        files.sort();
        while files.len() > keep {
            let _ = fs::remove_file(files.remove(0));
        }
        Ok(p)
    }

    // ── duplicates ──

    /// Groups of contacts that look like the same person: the same name, e-mail address or phone number.
    pub fn duplicates(&self) -> Vec<Vec<String>> {
        let mut parent: HashMap<String, String> = HashMap::new();
        fn find(p: &mut HashMap<String, String>, x: &str) -> String {
            let mut r = x.to_string();
            while let Some(n) = p.get(&r).cloned() {
                if n == r {
                    break;
                }
                r = n;
            }
            r
        }
        let mut seen: HashMap<String, String> = HashMap::new();
        for c in self.contacts.values() {
            parent.insert(c.id.clone(), c.id.clone());
            let mut keys: Vec<String> = vec![];
            let name = fold(&format!("{} {}", c.name.given, c.name.family)).split_whitespace().collect::<Vec<_>>().join(" ");
            if name.contains(' ') {
                keys.push(format!("n:{}", name));
            }
            for e in &c.emails {
                if !e.value.is_empty() {
                    keys.push(format!("e:{}", e.value.to_lowercase()));
                }
            }
            for p in &c.phones {
                let d = digits(&p.value);
                if d.len() >= 7 {
                    keys.push(format!("p:{}", &d[d.len() - 7..]));
                }
            }
            for k in keys {
                match seen.get(&k) {
                    Some(other) => {
                        let (a, b) = (find(&mut parent, other), find(&mut parent, &c.id));
                        if a != b {
                            parent.insert(b, a);
                        }
                    }
                    None => {
                        seen.insert(k, c.id.clone());
                    }
                }
            }
        }
        let ids: Vec<String> = parent.keys().cloned().collect();
        let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for id in ids {
            let r = find(&mut parent, &id);
            groups.entry(r).or_default().insert(id);
        }
        groups.into_values().filter(|g| g.len() > 1).map(|g| g.into_iter().collect()).collect()
    }

    /// Merges the contacts into the first one (union of everything, no value twice); the others go to the trash.
    pub fn merge(&mut self, ids: &[String]) -> Result<Contact> {
        let mut it = ids.iter().filter_map(|i| self.contacts.get(i).cloned());
        let mut base = it.next().ok_or_else(|| Error::Msg("Nothing to merge.".into()))?;
        let others: Vec<Contact> = it.collect();
        for o in &others {
            merge_into(&mut base, o);
        }
        let saved = self.save(base)?;
        for o in &others {
            self.delete(&o.id)?;
        }
        Ok(saved)
    }
}

fn merge_labeled(into: &mut Vec<Labeled>, from: &[Labeled], digits_only: bool) {
    for x in from {
        // phones: the same number written with or without the country code is the same number
        let key = |v: &str| {
            if digits_only {
                let d = digits(v);
                d[d.len().saturating_sub(9)..].to_string()
            } else {
                v.trim().to_lowercase()
            }
        };
        if x.value.is_empty() || into.iter().any(|y| key(&y.value) == key(&x.value)) {
            continue;
        }
        into.push(x.clone());
    }
}

pub fn merge_into(a: &mut Contact, b: &Contact) {
    macro_rules! fill {
        ($($f:ident).+) => { if a.$($f).+.is_empty() { a.$($f).+ = b.$($f).+.clone(); } };
    }
    fill!(name.prefix);
    fill!(name.given);
    fill!(name.middle);
    fill!(name.family);
    fill!(name.suffix);
    fill!(name.maiden);
    fill!(name.phonetic_given);
    fill!(name.phonetic_middle);
    fill!(name.phonetic_family);
    fill!(gender);
    fill!(pronouns);
    fill!(timezone);
    fill!(geo);
    if a.photo.is_none() {
        a.photo = b.photo.clone();
    }
    if a.kind.is_empty() {
        a.kind = b.kind.clone();
    }
    for n in &b.nicknames {
        if !a.nicknames.contains(n) {
            a.nicknames.push(n.clone());
        }
    }
    for o in &b.orgs {
        if !a.orgs.contains(o) {
            a.orgs.push(o.clone());
        }
    }
    merge_labeled(&mut a.phones, &b.phones, true);
    merge_labeled(&mut a.emails, &b.emails, false);
    merge_labeled(&mut a.urls, &b.urls, false);
    merge_labeled(&mut a.ims, &b.ims, false);
    merge_labeled(&mut a.social, &b.social, false);
    merge_labeled(&mut a.relations, &b.relations, false);
    merge_labeled(&mut a.keys, &b.keys, false);
    merge_labeled(&mut a.custom, &b.custom, false);
    for ad in &b.addresses {
        if !a.addresses.contains(ad) {
            a.addresses.push(ad.clone());
        }
    }
    for d in &b.dates {
        if !a.dates.iter().any(|x| x.label == d.label && x.month == d.month && x.day == d.day) {
            a.dates.push(d.clone());
        }
    }
    for l in b.languages.iter() {
        if !a.languages.contains(l) {
            a.languages.push(l.clone());
        }
    }
    for l in b.interests.iter() {
        if !a.interests.contains(l) {
            a.interests.push(l.clone());
        }
    }
    for l in b.categories.iter() {
        if !a.categories.contains(l) {
            a.categories.push(l.clone());
        }
    }
    if !b.notes.is_empty() && !a.notes.contains(&b.notes) {
        a.notes = if a.notes.is_empty() { b.notes.clone() } else { format!("{}\n\n{}", a.notes, b.notes) };
    }
    a.favorite |= b.favorite;
    for x in &b.extra {
        if !a.extra.contains(x) {
            a.extra.push(x.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(given: &str, family: &str, phone: &str, email: &str) -> Contact {
        let mut c = Contact::new();
        c.name.given = given.into();
        c.name.family = family.into();
        if !phone.is_empty() {
            c.phones.push(Labeled::new("mobile", phone));
        }
        if !email.is_empty() {
            c.emails.push(Labeled::new("home", email));
        }
        c
    }

    #[test]
    fn save_list_search_delete_restore() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path()).unwrap();
        let a = s.save(person("José", "Álvarez", "+1 (555) 010-0200", "jose@example.com")).unwrap();
        s.save(person("Anna", "Zed", "", "anna@example.com")).unwrap();
        assert_eq!(s.len(), 2);
        let q = Query { text: "jose".into(), ..Default::default() };
        assert_eq!(s.list(&q).len(), 1);
        let q = Query { text: "5550100".into(), ..Default::default() };
        assert_eq!(s.list(&q).len(), 1);
        let q = Query { text: "555 010 0200".into(), ..Default::default() };
        assert_eq!(s.list(&q).len(), 1);
        let q = Query { text: "nobody".into(), ..Default::default() };
        assert_eq!(s.list(&q).len(), 0);
        let q = Query { text: "0200".into(), ..Default::default() };
        assert_eq!(s.list(&q).len(), 1);
        let all = s.list(&Query::default());
        assert_eq!(all[0].display, "Anna Zed");
        assert_eq!(all[1].letter, "J");
        // reopen from disk
        let mut s2 = Store::open(dir.path()).unwrap();
        assert_eq!(s2.len(), 2);
        s2.delete(&a.id).unwrap();
        assert_eq!(s2.len(), 1);
        assert_eq!(s2.trash().len(), 1);
        s2.restore(&a.id).unwrap();
        assert_eq!(s2.len(), 2);
        assert!(s2.save(Contact::new()).is_err());
    }

    #[test]
    fn import_export_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path()).unwrap();
        s.save(person("A", "One", "", "a@x.org")).unwrap();
        s.save(person("B", "Two", "", "b@x.org")).unwrap();
        let text = s.export_vcf(None);
        let dir2 = tempfile::tempdir().unwrap();
        let mut t = Store::open(dir2.path()).unwrap();
        assert_eq!(t.import_vcf(&text).unwrap(), ImportReport { added: 2, updated: 0, skipped: 0 });
        assert_eq!(t.import_vcf(&text).unwrap(), ImportReport { added: 0, updated: 0, skipped: 2 });
        let b = t.backup(10).unwrap();
        assert!(b.exists());
    }

    #[test]
    fn duplicates_and_merge() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path()).unwrap();
        let a = s.save(person("Ann", "Lee", "+44 20 7946 0001", "")).unwrap();
        let b = s.save(person("Ann", "Lee", "", "ann@x.org")).unwrap();
        let c = s.save(person("Bob", "Ray", "020 7946 0001", "")).unwrap(); // same phone tail as Ann
        s.save(person("Zed", "Zed", "", "z@x.org")).unwrap();
        let d = s.duplicates();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].len(), 3);
        let m = s.merge(&[a.id.clone(), b.id.clone(), c.id.clone()]).unwrap();
        assert_eq!(m.phones.len(), 1);
        assert_eq!(m.emails.len(), 1);
        assert_eq!(s.len(), 2);
    }
}
