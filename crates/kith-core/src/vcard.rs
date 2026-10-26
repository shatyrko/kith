//! vCard reading (2.1, 3.0, 4.0, with the Apple / Google / Android habits) and writing (3.0: the one every
//! phone, Mac, Google and Thunderbird takes). Unknown lines are kept in `Contact::extra` and written back.

use crate::model::*;
use std::collections::{HashMap, HashSet};

// ───────────────────────────── low level ─────────────────────────────

#[derive(Debug, Clone)]
struct Line {
    group: Option<String>,
    name: String,
    params: Vec<(String, String)>,
    value: String,
    raw: String,
}

impl Line {
    fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
    fn types(&self) -> Vec<String> {
        let mut out = vec![];
        for (k, v) in &self.params {
            if k == "TYPE" {
                for t in v.split(',') {
                    let t = t.trim().trim_matches('"').to_lowercase();
                    if !t.is_empty() {
                        out.push(t);
                    }
                }
            }
        }
        out
    }
}

fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
    for l in text.split('\n') {
        if (l.starts_with(' ') || l.starts_with('\t')) && !out.is_empty() {
            let last = out.last_mut().unwrap();
            last.push_str(&l[1..]);
            continue;
        }
        // quoted-printable soft line break (vCard 2.1): "…=" and the next line goes on
        if let Some(last) = out.last_mut() {
            if last.to_uppercase().contains("QUOTED-PRINTABLE") && last.ends_with('=') {
                last.pop();
                last.push_str(l);
                continue;
            }
        }
        out.push(l.to_string());
    }
    out.retain(|l| !l.trim().is_empty());
    out
}

fn split_outside_quotes(s: &str, sep: char) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    for ch in s.chars() {
        if ch == '"' {
            quoted = !quoted;
            cur.push(ch);
        } else if ch == sep && !quoted {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

fn parse_line(raw: &str) -> Option<Line> {
    // the first ':' outside quotes ends the name and parameters
    let mut quoted = false;
    let mut colon = None;
    for (i, ch) in raw.char_indices() {
        if ch == '"' {
            quoted = !quoted;
        } else if ch == ':' && !quoted {
            colon = Some(i);
            break;
        }
    }
    let colon = colon?;
    let head = &raw[..colon];
    let mut value = raw[colon + 1..].to_string();
    let mut parts = split_outside_quotes(head, ';').into_iter();
    let first = parts.next()?;
    let (group, name) = match first.split_once('.') {
        Some((g, n)) => (Some(g.to_string()), n.to_uppercase()),
        None => (None, first.to_uppercase()),
    };
    let mut params = vec![];
    for p in parts {
        match p.split_once('=') {
            Some((k, v)) => params.push((k.trim().to_uppercase(), v.trim().to_string())),
            None if !p.trim().is_empty() => {
                // vCard 2.1: a bare word is a type (or ENCODING / CHARSET value)
                let up = p.trim().to_uppercase();
                if up == "QUOTED-PRINTABLE" || up == "BASE64" || up == "B" {
                    params.push(("ENCODING".into(), up));
                } else {
                    params.push(("TYPE".into(), p.trim().to_string()));
                }
            }
            None => {}
        }
    }
    let line = Line { group, name, params, value: String::new(), raw: raw.to_string() };
    if line.param("ENCODING").map(|e| e.eq_ignore_ascii_case("QUOTED-PRINTABLE")).unwrap_or(false) {
        value = qp_decode(&value);
    }
    Some(Line { value, ..line })
}

fn qp_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' && i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 + 0 {
            let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = h {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(o) => out.push(o),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// Splits on a separator that is not escaped; the pieces stay escaped.
fn split_unescaped(s: &str, sep: char) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut esc = false;
    for c in s.chars() {
        if esc {
            cur.push('\\');
            cur.push(c);
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == sep {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    if esc {
        cur.push('\\');
    }
    out.push(cur);
    out
}

/// One structured component may hold several comma separated values: join them with a space.
fn comp(s: &str) -> String {
    split_unescaped(s, ',').iter().map(|p| unescape(p)).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    let mut limit = 75usize;
    for ch in line.chars() {
        let n = ch.len_utf8();
        if count + n > limit {
            out.push_str("\r\n ");
            count = 1;
            limit = 75;
        }
        out.push(ch);
        count += n;
    }
    out.push_str("\r\n");
    out
}

// ───────────────────────────── dates ─────────────────────────────

fn parse_date(s: &str) -> Option<(Option<u16>, u8, u8)> {
    let s = s.trim();
    let s = s.split(['T', ' ']).next().unwrap_or(s);
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    let (y, m, d): (Option<u16>, u8, u8) = if let Some(rest) = s.strip_prefix("--") {
        let rest: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
        if rest.len() != 4 {
            return None;
        }
        (None, rest[0..2].parse().ok()?, rest[2..4].parse().ok()?)
    } else if digits.len() == 8 {
        (Some(digits[0..4].parse().ok()?), digits[4..6].parse().ok()?, digits[6..8].parse().ok()?)
    } else {
        return None;
    };
    if m == 0 || m > 12 || d == 0 || d > 31 {
        return None;
    }
    // Apple writes 1604 for "no year"; 1900 and 0000 are the same habit elsewhere
    let y = y.filter(|y| *y != 1604 && *y != 0);
    Some((y, m, d))
}

fn fmt_date(d: &LabeledDate) -> String {
    match d.year {
        Some(y) => format!("{:04}-{:02}-{:02}", y, d.month, d.day),
        None => format!("--{:02}-{:02}", d.month, d.day),
    }
}

pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn fmt_rev(ts: u64) -> String {
    let days = (ts / 86400) as i64;
    let rem = ts % 86400;
    let (y, m, d) = civil_from_days(days);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}Z", y, m, d, rem / 3600, rem / 60 % 60, rem % 60)
}

fn parse_rev(s: &str) -> Option<u64> {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    let y: i64 = digits[0..4].parse().ok()?;
    let m: i64 = digits[4..6].parse().ok()?;
    let d: i64 = digits[6..8].parse().ok()?;
    let (hh, mm, ss): (i64, i64, i64) = if digits.len() >= 14 {
        (digits[8..10].parse().ok()?, digits[10..12].parse().ok()?, digits[12..14].parse().ok()?)
    } else {
        (0, 0, 0)
    };
    let t = days_from_civil(y, m, d) * 86400 + hh * 3600 + mm * 60 + ss;
    if t < 0 { None } else { Some(t as u64) }
}

// ───────────────────────────── labels ─────────────────────────────

/// Apple wraps its built-in labels as `_$!<Mobile>!$_`.
fn clean_label(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("_$!<").and_then(|x| x.strip_suffix(">!$_")).unwrap_or(s);
    let low = s.to_lowercase();
    match low.as_str() {
        "mobile" | "cell" | "iphone" | "home" | "work" | "main" | "other" | "pager" | "homepage" | "blog" | "profile" | "fax"
        | "home fax" | "work fax" | "other fax" | "spouse" | "child" | "parent" | "mother" | "father" | "brother" | "sister"
        | "friend" | "partner" | "assistant" | "manager" | "anniversary" | "birthday" => {
            if low == "cell" { "mobile".into() } else if low == "iphone" { "iphone".into() } else { low }
        }
        "homefax" => "home fax".into(),
        "workfax" => "work fax".into(),
        "otherfax" => "other fax".into(),
        _ => s.to_string(),
    }
}

const IGNORED_TYPES: &[&str] = &["voice", "internet", "pref", "x400", "pcs", "msg", "text", "textphone", "x-inet", "oth"];

fn label_from(line: &Line, group_labels: &HashMap<String, String>) -> (String, bool) {
    let types = line.types();
    let pref = types.iter().any(|t| t == "pref") || line.param("PREF").is_some();
    if let Some(g) = &line.group {
        if let Some(l) = group_labels.get(g) {
            return (clean_label(l), pref);
        }
    }
    let kept: Vec<&String> = types.iter().filter(|t| !IGNORED_TYPES.contains(&t.as_str())).collect();
    let has = |x: &str| kept.iter().any(|t| t.as_str() == x);
    let label = if has("fax") {
        if has("home") { "home fax".to_string() } else if has("work") { "work fax".to_string() } else if has("other") { "other fax".to_string() } else { "fax".to_string() }
    } else if has("cell") || has("mobile") || has("iphone") {
        "mobile".to_string()
    } else if let Some(t) = kept.first() {
        clean_label(t)
    } else {
        String::new()
    };
    (label, pref)
}

// ───────────────────────────── reading ─────────────────────────────

pub fn parse(text: &str) -> Vec<Contact> {
    let lines = unfold(text);
    let mut cards: Vec<Vec<Line>> = vec![];
    let mut cur: Option<Vec<Line>> = None;
    for raw in &lines {
        let up = raw.trim().to_uppercase();
        if up == "BEGIN:VCARD" {
            cur = Some(vec![]);
        } else if up == "END:VCARD" {
            if let Some(c) = cur.take() {
                cards.push(c);
            }
        } else if let Some(c) = cur.as_mut() {
            if let Some(l) = parse_line(raw) {
                c.push(l);
            }
        }
    }
    cards.into_iter().map(card_to_contact).collect()
}

fn photo_from(line: &Line) -> Option<String> {
    let v = line.value.trim();
    if v.starts_with("data:") {
        return Some(v.to_string());
    }
    let enc = line.param("ENCODING").unwrap_or("").to_uppercase();
    if enc == "B" || enc == "BASE64" || (line.param("VALUE").is_none() && !v.contains(':') && v.len() > 64) {
        let ty = line.types().first().cloned().unwrap_or_else(|| "jpeg".into());
        let ty = ty.trim_start_matches("image/").to_string();
        let clean: String = v.chars().filter(|c| !c.is_whitespace()).collect();
        return Some(format!("data:image/{};base64,{}", ty, clean));
    }
    None
}

fn card_to_contact(lines: Vec<Line>) -> Contact {
    let mut c = Contact::default();
    let mut group_labels: HashMap<String, String> = HashMap::new();
    for l in &lines {
        if l.name == "X-ABLABEL" {
            if let Some(g) = &l.group {
                group_labels.insert(g.clone(), unescape(&l.value));
            }
        }
    }
    let mut consumed: HashSet<String> = HashSet::new();
    let mut fn_text = String::new();
    let mut titles: Vec<String> = vec![];
    let mut have_n = false;
    let mut favorite_seen = false;

    for l in &lines {
        let v = l.value.as_str();
        let mut used = true;
        match l.name.as_str() {
            "VERSION" | "PRODID" | "X-ABLABEL" | "CLASS" | "SORT-STRING" | "X-ABUID" => {
                used = false;
                if l.name == "X-ABLABEL" {
                    continue;
                }
            }
            "UID" => {
                let u = unescape(v);
                let u = u.trim().trim_start_matches("urn:uuid:").to_string();
                if !u.is_empty() && u.len() <= 80 && u.chars().all(|ch| ch.is_ascii_alphanumeric() || "-_.".contains(ch)) {
                    c.id = u;
                }
            }
            "FN" => fn_text = unescape(v),
            "N" => {
                let p = split_unescaped(v, ';');
                let g = |i: usize| p.get(i).map(|s| comp(s)).unwrap_or_default();
                c.name.family = g(0);
                c.name.given = g(1);
                c.name.middle = g(2);
                c.name.prefix = g(3);
                c.name.suffix = g(4);
                have_n = true;
            }
            "NICKNAME" | "X-NICKNAME" => {
                for n in split_unescaped(v, ',') {
                    let n = unescape(&n);
                    if !n.is_empty() {
                        c.nicknames.push(n);
                    }
                }
            }
            "X-PHONETIC-FIRST-NAME" => c.name.phonetic_given = unescape(v),
            "X-PHONETIC-MIDDLE-NAME" => c.name.phonetic_middle = unescape(v),
            "X-PHONETIC-LAST-NAME" => c.name.phonetic_family = unescape(v),
            "X-MAIDEN-NAME" | "X-MAIDENNAME" => c.name.maiden = unescape(v),
            "ORG" => {
                let p = split_unescaped(v, ';');
                let company = p.first().map(|s| unescape(s)).unwrap_or_default();
                let dept = p.iter().skip(1).map(|s| unescape(s)).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
                c.orgs.push(Org { company, department: dept, ..Default::default() });
            }
            "TITLE" => titles.push(unescape(v)),
            "TEL" => {
                let (label, pref) = label_from(l, &group_labels);
                let val = unescape(v);
                let val = val.strip_prefix("tel:").unwrap_or(&val).to_string();
                c.phones.push(Labeled { label: if label.is_empty() { "other".into() } else { label }, value: val, pref });
            }
            "EMAIL" => {
                let (label, pref) = label_from(l, &group_labels);
                c.emails.push(Labeled { label: if label.is_empty() { "other".into() } else { label }, value: unescape(v), pref });
            }
            "URL" => {
                let (label, pref) = label_from(l, &group_labels);
                c.urls.push(Labeled { label: if label.is_empty() { "homepage".into() } else { label }, value: unescape(v), pref });
            }
            "IMPP" | "X-SIP" => {
                let val = unescape(v);
                let (scheme, rest) = match val.split_once(':') {
                    Some((s, r)) => (s.to_string(), r.trim_start_matches("//").to_string()),
                    None => (String::new(), val.clone()),
                };
                let label = l
                    .param("X-SERVICE-TYPE")
                    .map(|s| s.to_string())
                    .or_else(|| group_labels.get(l.group.as_deref().unwrap_or("")).cloned())
                    .unwrap_or_else(|| {
                        let mut ch = scheme.chars();
                        match ch.next() {
                            Some(f) if scheme != "x-apple" => f.to_uppercase().collect::<String>() + ch.as_str(),
                            _ => String::new(),
                        }
                    });
                c.ims.push(Labeled { label, value: rest, pref: false });
            }
            n if n.starts_with("X-") && ["SKYPE", "JABBER", "AIM", "ICQ", "MSN", "YAHOO", "GOOGLE-TALK", "QQ", "GADUGADU", "GROUPWISE"].contains(&&n[2..]) => {
                let svc = &n[2..];
                let nice = match svc {
                    "GOOGLE-TALK" => "Google Talk".to_string(),
                    "JABBER" => "XMPP".to_string(),
                    other => other[..1].to_string() + &other[1..].to_lowercase(),
                };
                c.ims.push(Labeled::new(&nice, &unescape(v)));
            }
            "X-SOCIALPROFILE" => {
                let label = l.types().first().cloned().unwrap_or_default();
                let val = if !unescape(v).is_empty() { unescape(v) } else { l.param("X-USER").unwrap_or("").to_string() };
                c.social.push(Labeled::new(&label, &val));
            }
            "ADR" => {
                let p = split_unescaped(v, ';');
                let g = |i: usize| p.get(i).map(|s| comp(s)).unwrap_or_default();
                let (label, pref) = label_from(l, &group_labels);
                c.addresses.push(Address {
                    label: if label.is_empty() { "other".into() } else { label },
                    po_box: g(0),
                    extended: g(1),
                    street: g(2),
                    city: g(3),
                    region: g(4),
                    postcode: g(5),
                    country: g(6),
                    pref,
                });
            }
            "BDAY" | "ANNIVERSARY" | "DEATHDATE" | "X-ABDATE" => {
                match parse_date(v) {
                    Some((y, m, d)) => {
                        let label = match l.name.as_str() {
                            "BDAY" => "birthday".to_string(),
                            "ANNIVERSARY" => "anniversary".to_string(),
                            "DEATHDATE" => "death".to_string(),
                            _ => {
                                let lab = group_labels.get(l.group.as_deref().unwrap_or("")).map(|s| clean_label(s)).unwrap_or_default();
                                if lab.is_empty() { "other".to_string() } else { lab }
                            }
                        };
                        c.dates.push(LabeledDate { label, year: y, month: m, day: d });
                    }
                    None => {
                        c.extra.push(l.raw.clone());
                    }
                }
            }
            "X-ABRELATEDNAMES" | "RELATED" => {
                let val = unescape(v);
                if val.starts_with("urn:") || val.starts_with("http") {
                    c.extra.push(l.raw.clone());
                } else {
                    let label = group_labels
                        .get(l.group.as_deref().unwrap_or(""))
                        .map(|s| clean_label(s))
                        .or_else(|| l.types().first().cloned())
                        .unwrap_or_else(|| "relative".into());
                    c.relations.push(Labeled::new(&label, &val));
                }
            }
            "NOTE" => {
                let n = unescape(v);
                if c.notes.is_empty() { c.notes = n } else { c.notes = format!("{}\n{}", c.notes, n) }
            }
            "PHOTO" => match photo_from(l) {
                Some(p) if c.photo.is_none() => c.photo = Some(p),
                _ => c.extra.push(l.raw.clone()),
            },
            "GENDER" | "X-GENDER" => {
                let p = split_unescaped(v, ';');
                let sex = p.first().map(|s| unescape(s)).unwrap_or_default();
                let text = p.get(1).map(|s| unescape(s)).unwrap_or_default();
                c.gender = if !text.is_empty() {
                    text
                } else {
                    match sex.as_str() {
                        "M" => "male".into(),
                        "F" => "female".into(),
                        "O" => "other".into(),
                        "N" | "U" | "" => String::new(),
                        s => s.to_string(),
                    }
                };
            }
            "PRONOUNS" | "X-PRONOUNS" => c.pronouns = unescape(v),
            "LANG" | "X-LANGUAGE" | "X-LANGUAGES" => {
                for p in split_unescaped(v, ',') {
                    let p = unescape(&p);
                    if !p.is_empty() {
                        c.languages.push(p);
                    }
                }
            }
            "TZ" => c.timezone = unescape(v),
            "GEO" => {
                let g = unescape(v);
                c.geo = g.trim_start_matches("geo:").replace(';', ",");
            }
            "INTEREST" | "HOBBY" | "EXPERTISE" => {
                let n = unescape(v);
                if !n.is_empty() {
                    c.interests.push(n);
                }
            }
            "CATEGORIES" => {
                for p in split_unescaped(v, ',') {
                    let p = unescape(&p);
                    if !p.is_empty() && !c.categories.contains(&p) {
                        c.categories.push(p);
                    }
                }
            }
            "KIND" => {
                let k = unescape(v).to_lowercase();
                if k == "org" { c.kind = "org".into() }
            }
            "X-ABSHOWAS" => {
                if v.eq_ignore_ascii_case("COMPANY") { c.kind = "org".into() }
            }
            "KEY" | "X-KITH-KEY" => {
                let enc = l.param("ENCODING").is_some();
                if enc && l.name == "KEY" {
                    c.extra.push(l.raw.clone());
                } else {
                    let label = if l.name == "X-KITH-KEY" {
                        group_labels.get(l.group.as_deref().unwrap_or("")).cloned().unwrap_or_else(|| "pgp".into())
                    } else {
                        l.types().first().cloned().unwrap_or_else(|| "key".into())
                    };
                    c.keys.push(Labeled::new(&label, &unescape(v)));
                }
            }
            "X-KITH-FIELD" => {
                let label = group_labels.get(l.group.as_deref().unwrap_or("")).cloned().unwrap_or_default();
                c.custom.push(Labeled::new(&label, &unescape(v)));
            }
            "X-KITH-FAVORITE" => {
                c.favorite = v.trim() == "1";
                favorite_seen = true;
            }
            "X-KITH-CREATED" => c.created = v.trim().parse().unwrap_or(0),
            "REV" => c.updated = parse_rev(v).unwrap_or(0),
            _ => {
                used = false;
                c.extra.push(l.raw.clone());
            }
        }
        if used {
            if let Some(g) = &l.group {
                consumed.insert(g.clone());
            }
        }
    }
    let _ = favorite_seen;

    // a label line whose property we did not take stays with it
    for l in &lines {
        if l.name == "X-ABLABEL" {
            if let Some(g) = &l.group {
                if !consumed.contains(g) {
                    c.extra.push(l.raw.clone());
                }
            }
        }
    }

    // titles go with the organisations in order
    for (i, t) in titles.into_iter().enumerate() {
        if i < c.orgs.len() {
            c.orgs[i].title = t;
        } else {
            c.orgs.push(Org { title: t, ..Default::default() });
        }
    }

    // no N: take the name from FN (or it is a company card)
    let name_empty = c.name == Name::default();
    if (!have_n || name_empty) && !fn_text.is_empty() {
        let is_company = c.orgs.first().map(|o| o.company == fn_text).unwrap_or(false);
        if is_company {
            c.kind = "org".into();
        } else if let Some((fam, giv)) = fn_text.split_once(',') {
            c.name.family = fam.trim().into();
            c.name.given = giv.trim().into();
        } else {
            let words: Vec<&str> = fn_text.split_whitespace().collect();
            match words.len() {
                0 => {}
                1 => c.name.given = words[0].into(),
                _ => {
                    c.name.family = words[words.len() - 1].into();
                    c.name.given = words[..words.len() - 1].join(" ");
                }
            }
        }
    }

    if c.id.is_empty() {
        c.id = new_id();
    }
    let t = now();
    if c.updated == 0 {
        c.updated = t;
    }
    if c.created == 0 {
        c.created = c.updated;
    }
    c
}

// ───────────────────────────── writing ─────────────────────────────

struct Out {
    lines: Vec<String>,
    group: usize,
}

impl Out {
    fn plain(&mut self, name: &str, params: &str, value: &str) {
        let p = if params.is_empty() { String::new() } else { format!(";{}", params) };
        self.lines.push(format!("{}{}:{}", name, p, value));
    }
    /// A property with a custom label: Apple style group + X-ABLabel.
    fn grouped(&mut self, name: &str, params: &str, value: &str, label: &str) {
        self.group += 1;
        let g = format!("k{}", self.group);
        let p = if params.is_empty() { String::new() } else { format!(";{}", params) };
        self.lines.push(format!("{}.{}{}:{}", g, name, p, value));
        self.lines.push(format!("{}.X-ABLabel:{}", g, escape(label)));
    }
}

fn apple_label(label: &str) -> String {
    match label {
        "spouse" | "child" | "parent" | "mother" | "father" | "brother" | "sister" | "friend" | "partner" | "assistant" | "manager"
        | "anniversary" | "other" | "homepage" => {
            let mut c = label.chars();
            let f = c.next().unwrap().to_uppercase().collect::<String>() + c.as_str();
            let f = if label == "homepage" { "HomePage".to_string() } else { f };
            format!("_$!<{}>!$_", f)
        }
        _ => label.to_string(),
    }
}

fn phone_types(label: &str) -> Option<&'static str> {
    Some(match label {
        "mobile" => "TYPE=CELL",
        "home" => "TYPE=HOME",
        "work" => "TYPE=WORK",
        "home fax" => "TYPE=HOME;TYPE=FAX",
        "work fax" => "TYPE=WORK;TYPE=FAX",
        "fax" | "other fax" => "TYPE=FAX",
        "pager" => "TYPE=PAGER",
        "other" | "" => "TYPE=VOICE",
        _ => return None,
    })
}

fn simple_types(label: &str, base: &'static str) -> Option<String> {
    Some(match label {
        "home" => "TYPE=HOME".to_string(),
        "work" => "TYPE=WORK".to_string(),
        "other" | "" => base.to_string(),
        _ => return None,
    })
}

fn with_pref(params: String, pref: bool) -> String {
    if !pref {
        params
    } else if params.is_empty() {
        "TYPE=PREF".into()
    } else {
        format!("{};TYPE=PREF", params)
    }
}

pub fn write(c: &Contact) -> String {
    let mut o = Out { lines: vec![], group: 0 };
    o.lines.push("BEGIN:VCARD".into());
    o.lines.push("VERSION:3.0".into());
    o.lines.push("PRODID:-//Kith//EN".into());
    o.plain("UID", "", &escape(&c.id));
    let n = &c.name;
    o.plain("N", "", &format!("{};{};{};{};{}", escape(&n.family), escape(&n.given), escape(&n.middle), escape(&n.prefix), escape(&n.suffix)));
    o.plain("FN", "", &escape(&c.display_name(false)));
    if !c.nicknames.is_empty() {
        o.plain("NICKNAME", "", &c.nicknames.iter().map(|s| escape(s)).collect::<Vec<_>>().join(","));
    }
    for (k, v) in [
        ("X-PHONETIC-FIRST-NAME", &n.phonetic_given),
        ("X-PHONETIC-MIDDLE-NAME", &n.phonetic_middle),
        ("X-PHONETIC-LAST-NAME", &n.phonetic_family),
        ("X-MAIDEN-NAME", &n.maiden),
    ] {
        if !v.is_empty() {
            o.plain(k, "", &escape(v));
        }
    }
    for (i, org) in c.orgs.iter().enumerate() {
        let val = if org.department.is_empty() { escape(&org.company) } else { format!("{};{}", escape(&org.company), escape(&org.department)) };
        if i == 0 {
            if !org.company.is_empty() || !org.department.is_empty() {
                o.plain("ORG", "", &val);
            }
            if !org.title.is_empty() {
                o.plain("TITLE", "", &escape(&org.title));
            }
        } else {
            o.group += 1;
            let g = format!("k{}", o.group);
            if !org.company.is_empty() || !org.department.is_empty() {
                o.lines.push(format!("{}.ORG:{}", g, val));
            }
            if !org.title.is_empty() {
                o.lines.push(format!("{}.TITLE:{}", g, escape(&org.title)));
            }
        }
    }
    for p in c.phones.iter().filter(|p| !p.value.is_empty()) {
        match phone_types(&p.label) {
            Some(t) => o.plain("TEL", &with_pref(t.to_string(), p.pref), &escape(&p.value)),
            None => o.grouped("TEL", &with_pref(String::new(), p.pref), &escape(&p.value), &apple_label(&p.label)),
        }
    }
    for e in c.emails.iter().filter(|e| !e.value.is_empty()) {
        match simple_types(&e.label, "TYPE=INTERNET") {
            Some(t) => o.plain("EMAIL", &with_pref(t, e.pref), &escape(&e.value)),
            None => o.grouped("EMAIL", &with_pref("TYPE=INTERNET".into(), e.pref), &escape(&e.value), &apple_label(&e.label)),
        }
    }
    for u in c.urls.iter().filter(|u| !u.value.is_empty()) {
        match u.label.as_str() {
            "homepage" | "" => o.plain("URL", &with_pref(String::new(), u.pref), &escape(&u.value)),
            "home" => o.plain("URL", &with_pref("TYPE=HOME".into(), u.pref), &escape(&u.value)),
            "work" => o.plain("URL", &with_pref("TYPE=WORK".into(), u.pref), &escape(&u.value)),
            l => o.grouped("URL", &with_pref(String::new(), u.pref), &escape(&u.value), &apple_label(l)),
        }
    }
    for im in c.ims.iter().filter(|i| !i.value.is_empty()) {
        let scheme: String = im.label.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        let scheme = if scheme.is_empty() { "x-apple".to_string() } else { scheme };
        let params = if im.label.is_empty() { String::new() } else { format!("X-SERVICE-TYPE={}", im.label.replace([';', ':', ','], " ")) };
        o.plain("IMPP", &params, &escape(&format!("{}:{}", scheme, im.value)));
    }
    for s in c.social.iter().filter(|s| !s.value.is_empty()) {
        let params = if s.label.is_empty() { String::new() } else { format!("TYPE={}", s.label.replace([';', ':', ','], " ")) };
        o.plain("X-SOCIALPROFILE", &params, &escape(&s.value));
    }
    for a in c.addresses.iter().filter(|a| !a.is_empty()) {
        let val = format!(
            "{};{};{};{};{};{};{}",
            escape(&a.po_box), escape(&a.extended), escape(&a.street), escape(&a.city), escape(&a.region), escape(&a.postcode), escape(&a.country)
        );
        match simple_types(&a.label, "") {
            Some(t) => o.plain("ADR", &with_pref(t, a.pref), &val),
            None => o.grouped("ADR", &with_pref(String::new(), a.pref), &val, &apple_label(&a.label)),
        }
    }
    for d in &c.dates {
        match d.label.as_str() {
            "birthday" => o.plain("BDAY", "", &fmt_date(d)),
            "" => o.grouped("X-ABDATE", "", &fmt_date(d), "other"),
            l => o.grouped("X-ABDATE", "", &fmt_date(d), &apple_label(l)),
        }
    }
    for r in c.relations.iter().filter(|r| !r.value.is_empty()) {
        o.grouped("X-ABRELATEDNAMES", "", &escape(&r.value), &apple_label(&r.label));
    }
    for k in c.keys.iter().filter(|k| !k.value.is_empty()) {
        o.grouped("X-KITH-KEY", "", &escape(&k.value), &k.label);
    }
    if !c.notes.is_empty() {
        o.plain("NOTE", "", &escape(&c.notes));
    }
    if let Some(p) = &c.photo {
        if let Some(rest) = p.strip_prefix("data:") {
            if let Some((meta, data)) = rest.split_once(',') {
                let ty = meta.split(';').next().unwrap_or("image/jpeg").trim_start_matches("image/").to_uppercase();
                o.plain("PHOTO", &format!("ENCODING=b;TYPE={}", ty), data);
            }
        }
    }
    if !c.gender.is_empty() {
        o.plain("X-GENDER", "", &escape(&c.gender));
    }
    if !c.pronouns.is_empty() {
        o.plain("X-PRONOUNS", "", &escape(&c.pronouns));
    }
    for l in c.languages.iter().filter(|l| !l.is_empty()) {
        o.plain("LANG", "", &escape(l));
    }
    if !c.timezone.is_empty() {
        o.plain("TZ", "", &escape(&c.timezone));
    }
    if !c.geo.is_empty() {
        o.plain("GEO", "", &c.geo.replace(',', ";"));
    }
    for i in c.interests.iter().filter(|i| !i.is_empty()) {
        o.plain("INTEREST", "", &escape(i));
    }
    if !c.categories.is_empty() {
        o.plain("CATEGORIES", "", &c.categories.iter().map(|s| escape(s)).collect::<Vec<_>>().join(","));
    }
    if c.is_org() {
        o.plain("KIND", "", "org");
        o.plain("X-ABShowAs", "", "COMPANY");
    }
    for f in c.custom.iter().filter(|f| !f.value.is_empty()) {
        o.grouped("X-KITH-FIELD", "", &escape(&f.value), &f.label);
    }
    if c.favorite {
        o.plain("X-KITH-FAVORITE", "", "1");
    }
    if c.created > 0 {
        o.plain("X-KITH-CREATED", "", &c.created.to_string());
    }
    o.plain("REV", "", &fmt_rev(if c.updated > 0 { c.updated } else { now() }));
    for x in &c.extra {
        o.lines.push(x.clone());
    }
    o.lines.push("END:VCARD".into());
    o.lines.iter().map(|l| fold(l)).collect()
}

pub fn write_all<'a>(contacts: impl IntoIterator<Item = &'a Contact>) -> String {
    contacts.into_iter().map(write).collect()
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const APPLE: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nPRODID:-//Apple Inc.//iPhone OS 17//EN\r\nN:Doe;Jane;Q.;Dr.;PhD\r\nFN:Dr. Jane Q. Doe PhD\r\nNICKNAME:JD\r\nORG:Acme\\, Inc.;Research\r\nTITLE:Chief\r\nitem1.TEL;type=pref:+1 555 0100\r\nitem1.X-ABLabel:_$!<Mobile>!$_\r\nTEL;type=WORK;type=VOICE:+1 555 0101\r\nitem2.EMAIL;type=INTERNET:jane@example.com\r\nitem2.X-ABLabel:Side gig\r\nitem3.ADR;type=HOME:;;1 Main St;Springfield;IL;62701;USA\r\nitem3.X-ABLabel:_$!<Home>!$_\r\nBDAY:1985-03-07\r\nitem4.X-ABDATE;type=pref:1604-06-01\r\nitem4.X-ABLabel:_$!<Anniversary>!$_\r\nitem5.X-ABRELATEDNAMES:John Doe\r\nitem5.X-ABLabel:_$!<Spouse>!$_\r\nNOTE:Line one\\nLine two\r\nX-SOMETHING-ELSE:keep me\r\nEND:VCARD\r\n";

    #[test]
    fn reads_an_apple_card() {
        let c = &parse(APPLE)[0];
        assert_eq!(c.name.given, "Jane");
        assert_eq!(c.name.family, "Doe");
        assert_eq!(c.name.middle, "Q.");
        assert_eq!(c.name.prefix, "Dr.");
        assert_eq!(c.nicknames, vec!["JD"]);
        assert_eq!(c.orgs[0].company, "Acme, Inc.");
        assert_eq!(c.orgs[0].department, "Research");
        assert_eq!(c.orgs[0].title, "Chief");
        assert_eq!(c.phones[0], Labeled { label: "mobile".into(), value: "+1 555 0100".into(), pref: true });
        assert_eq!(c.phones[1].label, "work");
        assert_eq!(c.emails[0].label, "Side gig");
        assert_eq!(c.addresses[0].label, "home");
        assert_eq!(c.addresses[0].street, "1 Main St");
        assert_eq!(c.birthday().unwrap().year, Some(1985));
        let ann = c.dates.iter().find(|d| d.label == "anniversary").unwrap();
        assert_eq!((ann.year, ann.month, ann.day), (None, 6, 1));
        assert_eq!(c.relations[0], Labeled::new("spouse", "John Doe"));
        assert_eq!(c.notes, "Line one\nLine two");
        assert_eq!(c.extra, vec!["X-SOMETHING-ELSE:keep me"]);
    }

    #[test]
    fn round_trips() {
        let a = &parse(APPLE)[0];
        let text = write(a);
        let b = &parse(&text)[0];
        let mut a2 = a.clone();
        let mut b2 = b.clone();
        a2.updated = 0;
        b2.updated = 0;
        a2.created = 0;
        b2.created = 0;
        a2.id.clear();
        b2.id.clear();
        assert_eq!(a2, b2);
        for l in text.split("\r\n") {
            assert!(l.len() <= 75, "line too long: {l}");
        }
    }

    #[test]
    fn reads_google_and_v4_styles() {
        let g = "BEGIN:VCARD\nVERSION:3.0\nFN:Ada Lovelace\nN:Lovelace;Ada;;;\nTEL;TYPE=CELL:+44 20 7946 0000\nEMAIL;TYPE=INTERNET,HOME:ada@example.org\nCATEGORIES:myContacts,Friends\nPHOTO;ENCODING=b;TYPE=JPEG:QUJD\nEND:VCARD\n";
        let c = &parse(g)[0];
        assert_eq!(c.phones[0].label, "mobile");
        assert_eq!(c.emails[0].label, "home");
        assert_eq!(c.categories, vec!["myContacts", "Friends"]);
        assert_eq!(c.photo.as_deref(), Some("data:image/jpeg;base64,QUJD"));
        let v4 = "BEGIN:VCARD\nVERSION:4.0\nFN:Grace Hopper\nTEL;TYPE=\"work,voice\";VALUE=uri:tel:+1-555-0199\nGENDER:F\nBDAY:--0309\nANNIVERSARY:19500101\nIMPP:xmpp:grace@jabber.example\nEND:VCARD\n";
        let c = &parse(v4)[0];
        assert_eq!(c.name.given, "Grace");
        assert_eq!(c.name.family, "Hopper");
        assert_eq!(c.phones[0].value, "+1-555-0199");
        assert_eq!(c.phones[0].label, "work");
        assert_eq!(c.gender, "female");
        assert_eq!(c.birthday().unwrap().year, None);
        assert_eq!(c.ims[0], Labeled::new("Xmpp", "grace@jabber.example"));
    }

    #[test]
    fn folding_and_quoted_printable() {
        let long = "N:A;B;;;\r\nFN:B A\r\nNOTE:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n bbbbbbbbbb\r\nEND:VCARD\r\n";
        let c = &parse(&format!("BEGIN:VCARD\r\nVERSION:3.0\r\n{long}"))[0];
        assert!(c.notes.ends_with("bbbbbbbbbb"));
        let qp = "BEGIN:VCARD\nVERSION:2.1\nN;ENCODING=QUOTED-PRINTABLE;CHARSET=UTF-8:=D0=9F=D0=B5=D1=82=D1=80=D0=B5=\n=D0=BD=D0=BA=D0=BE;=D0=98=D0=B2=D0=B0=D0=BD;;;\nTEL;CELL;VOICE:123\nEND:VCARD\n";
        let c = &parse(qp)[0];
        assert_eq!(c.name.family, "Петренко");
        assert_eq!(c.name.given, "Иван");
        assert_eq!(c.phones[0].label, "mobile");
    }

    #[test]
    fn company_card_and_no_n() {
        let s = "BEGIN:VCARD\nVERSION:3.0\nFN:Acme Corp\nORG:Acme Corp\nEND:VCARD\n";
        let c = &parse(s)[0];
        assert!(c.is_org());
        assert_eq!(c.display_name(false), "Acme Corp");
        let s = "BEGIN:VCARD\nVERSION:3.0\nFN:Smith, Bob\nEND:VCARD\n";
        let c = &parse(s)[0];
        assert_eq!((c.name.given.as_str(), c.name.family.as_str()), ("Bob", "Smith"));
    }

    #[test]
    fn dates_roundtrip_through_rev() {
        let ts = 1_793_000_000u64;
        assert_eq!(parse_rev(&fmt_rev(ts)), Some(ts));
    }

    #[test]
    fn full_contact_round_trips() {
        let mut c = Contact::new();
        c.name.given = "Ünal".into();
        c.name.family = "Ö;zkan, Jr".into();
        c.name.maiden = "Yılmaz".into();
        c.name.phonetic_given = "Oonal".into();
        c.orgs = vec![Org { company: "A".into(), department: "B".into(), title: "C".into(), phonetic: String::new() }, Org { company: "Second".into(), title: "Advisor".into(), ..Default::default() }];
        c.phones = vec![Labeled::new("home fax", "1"), Labeled::new("Satellite", "2")];
        c.ims = vec![Labeled::new("Signal", "@ünal.01")];
        c.social = vec![Labeled::new("twitter", "https://x.example/u")];
        c.keys = vec![Labeled::new("pgp", "0xDEADBEEF")];
        c.custom = vec![Labeled::new("Passport", "X123")];
        c.categories = vec!["Family".into(), "Work, remote".into()];
        c.languages = vec!["tr".into(), "en".into()];
        c.geo = "41.01,28.97".into();
        c.favorite = true;
        c.kind = String::new();
        c.dates = vec![LabeledDate { label: "birthday".into(), year: Some(1990), month: 2, day: 28 }, LabeledDate { label: "first met".into(), year: None, month: 7, day: 4 }];
        c.notes = "a\nb;c,d\\e".into();
        c.photo = Some("data:image/png;base64,iVBORw0KGgo=".into());
        let back = &parse(&write(&c))[0];
        assert_eq!(back.name, c.name);
        assert_eq!(back.orgs, c.orgs);
        assert_eq!(back.phones, c.phones);
        assert_eq!(back.ims, vec![Labeled::new("Signal", "@ünal.01")]);
        assert_eq!(back.social, c.social);
        assert_eq!(back.keys, c.keys);
        assert_eq!(back.custom, c.custom);
        assert_eq!(back.categories, c.categories);
        assert_eq!(back.languages, c.languages);
        assert_eq!(back.geo, c.geo);
        assert!(back.favorite);
        assert_eq!(back.dates, c.dates);
        assert_eq!(back.notes, c.notes);
        assert_eq!(back.photo, c.photo);
        assert_eq!(back.id, c.id);
    }
}
