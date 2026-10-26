//! The sync engine: [`sync`] reconciles a [`Store`] with Google Contacts through any [`PeopleApi`].
//!
//! # Data safety first
//!
//! * Before the first change of a run (local or remote) the whole book is saved with `store.backup(10)`; a dry run
//!   changes nothing and makes no backup.
//! * Google deletions go to the Kith **trash** (kept 30 days); Kith deletions are only sent to Google when
//!   [`SyncOptions::allow_remote_deletes`] is on (and Google keeps those 30 days in its own trash).
//! * Before a contact is updated or deleted on Google, its current Google version is appended to
//!   `<root>/sync/google-originals.jsonl`, so an unwanted change can be undone by hand.
//! * Updates start from the freshly read Google person and keep what Kith does not model
//!   (see [`crate::google::mapping::merge_for_update`]); only the fields in `UPDATE_PERSON_FIELDS` are replaced.
//! * A suspicious bulk deletion (more than 10 contacts and more than half of all linked ones) is never carried out
//!   in either direction; a warning is reported instead.
//!
//! # Rules
//!
//! **State.** `<root>/sync/google.json` ([`SyncState`]) remembers the sync token, and for every linked contact its
//! Google resource name, etag, a hash of the content Google can hold at the last sync, and photo bookkeeping.
//! The state is saved after each page of the pull, every few pushes, and on the way out of a failed run, so it
//! always matches what was really done; the sync token only moves when a whole pull succeeded. Repeating an
//! interrupted run is therefore safe (applying a page twice changes nothing).
//!
//! **Pull** (always). First run: full list. Later runs: changes since the sync token; if Google says it expired, the
//! whole book is fetched again, and linked contacts missing from that full list count as deleted on Google.
//! * New people become new contacts. A new person that is identical to an unlinked local contact (same content hash,
//!   e.g. after importing Google's own export) is *adopted* instead of duplicated; near matches are not merged.
//! * Changed people are applied with `apply_remote` (only fields Google can hold), without bumping `updated`.
//! * People deleted on Google go to the trash, unless the local contact has unsynced edits: then it is kept, unlinked
//!   (so that a two-way sync sends it back as a new contact), and counted as a conflict.
//! * Photos are downloaded when the Google photo URL changed.
//! * A contact that was deleted in Kith but changed on Google comes back.
//!
//! **Push** (only with `two_way`). Local contacts without a link are created on Google (missing labels are created
//! first); linked ones whose content hash differs from the last-synced hash are updated with the current etag; changed
//! local photos are uploaded. Contacts are never pushed right after being pulled: their hash is recorded at pull time.
//! Linked contacts missing locally are deleted on Google only with `allow_remote_deletes`, otherwise reported.
//!
//! **Conflicts** (changed on both sides since the last sync, or Google refuses an update because its etag moved):
//! Google wins, and the local version is saved as `.vcf` in `<root>/conflicts/` and counted.
//!
//! **Labels.** Google labels are Kith categories. A label renamed or deleted on Google is renamed/removed on the
//! linked contacts that had no unsynced edits. The star is `favorite`.
//!
//! **Errors.** Any API error stops the run, keeps the state consistent and is returned; nothing is half applied.
//!
//! **Dry run** computes [`SyncReport::planned`] (reads from Google are still made) and writes nothing, neither
//! contacts nor state; counters stay zero. Pushes are planned against the current state, so a contact that the pull
//! would have adopted may still show up as a planned push.
//!
//! Not handled: two syncs at once (the caller must serialize them) and clearing a photo on Google.

use crate::google::api::{ApiError, PeopleApi, USER_GROUP};
use crate::google::mapping::{apply_remote, contact_to_person, google_view, merge_for_update, person_to_contact, photo_url, UPDATE_PERSON_FIELDS};
use crate::model::{new_id, now, Contact};
use crate::store::{self, Store};
use crate::vcard;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    Store(#[from] store::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    State(String),
}

pub type Result<T> = std::result::Result<T, Error>;

// ───────────────────────────── options and report ─────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncOptions {
    /// Send local changes to Google (otherwise Google is only read).
    pub two_way: bool,
    /// Delete on Google the contacts deleted in Kith (otherwise they are only reported).
    pub allow_remote_deletes: bool,
    /// Compute what would happen, change nothing.
    pub dry_run: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    PullNew,
    PullUpdate,
    PullDelete,
    PushNew,
    PushUpdate,
    PushDelete,
    Conflict,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PlannedAction {
    pub kind: ActionKind,
    /// Kith contact id, when the contact exists locally.
    pub id: Option<String>,
    pub resource_name: Option<String>,
    /// Display name, for showing the plan.
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SyncReport {
    pub pulled_new: usize,
    pub pulled_updated: usize,
    pub pulled_deleted: usize,
    pub pushed_new: usize,
    pub pushed_updated: usize,
    pub pushed_deleted: usize,
    pub conflicts: usize,
    /// Left alone on purpose (empty on Google, deletions not allowed, …).
    pub skipped: usize,
    /// Local contacts recognised as already on Google and linked instead of duplicated.
    pub adopted: usize,
    /// The whole address book was read (first run or expired sync token).
    pub full_sync: bool,
    pub warnings: Vec<String>,
    pub planned: Vec<PlannedAction>,
}

impl SyncReport {
    /// Nothing happened in either direction.
    pub fn is_quiet(&self) -> bool {
        self.pulled_new + self.pulled_updated + self.pulled_deleted + self.pushed_new + self.pushed_updated + self.pushed_deleted + self.conflicts == 0
    }
}

// ───────────────────────────── persistent state ─────────────────────────────

/// What is remembered about one linked contact.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Link {
    pub resource_name: String,
    pub etag: String,
    /// [`content_hash`] of the local contact at the last sync.
    pub hash: String,
    /// [`content_hash`] of Google's version at the last sync (tells "Google changed it" from "Google touched something unmapped").
    pub remote_hash: String,
    pub photo_url: Option<String>,
    /// [`photo_hash`] of the local photo at the last sync ("" = none).
    pub photo_hash: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SyncState {
    pub version: u32,
    pub sync_token: Option<String>,
    /// Kith contact id → link.
    pub links: BTreeMap<String, Link>,
    /// Google label resource name → name (user labels only).
    pub groups: BTreeMap<String, String>,
    #[serde(skip)]
    by_resource: HashMap<String, String>,
}

impl SyncState {
    pub fn path(root: &Path) -> PathBuf {
        root.join("sync").join("google.json")
    }

    /// Loads the state; a missing file is a fresh state, a damaged one is an error (resetting silently would duplicate contacts).
    pub fn load(root: &Path) -> Result<SyncState> {
        let p = Self::path(root);
        match fs::read_to_string(&p) {
            Ok(text) => {
                let mut s: SyncState = serde_json::from_str(&text).map_err(|e| Error::State(format!("{} is damaged ({e}); move it away to start a fresh sync", p.display())))?;
                s.reindex();
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SyncState { version: 1, ..Default::default() }),
            Err(e) => Err(e.into()),
        }
    }

    /// Atomic: written to a temporary file, then renamed.
    pub fn save(&self, root: &Path) -> Result<()> {
        let p = Self::path(root);
        if let Some(d) = p.parent() {
            fs::create_dir_all(d)?;
        }
        let tmp = p.with_extension("json.tmp");
        let mut f = fs::File::create(&tmp)?;
        f.write_all(serde_json::to_string_pretty(self).map_err(|e| Error::State(e.to_string()))?.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, &p)?;
        Ok(())
    }

    fn reindex(&mut self) {
        self.by_resource = self.links.iter().map(|(id, l)| (l.resource_name.clone(), id.clone())).collect();
    }

    /// The Kith id linked to a Google resource name.
    pub fn id_of(&self, resource_name: &str) -> Option<&str> {
        self.by_resource.get(resource_name).map(String::as_str)
    }

    pub fn link(&self, id: &str) -> Option<&Link> {
        self.links.get(id)
    }

    fn set_link(&mut self, id: &str, link: Link) {
        if let Some(old) = self.links.get(id) {
            self.by_resource.remove(&old.resource_name);
        }
        self.by_resource.insert(link.resource_name.clone(), id.to_string());
        self.links.insert(id.to_string(), link);
    }

    fn remove_link(&mut self, id: &str) {
        if let Some(old) = self.links.remove(id) {
            self.by_resource.remove(&old.resource_name);
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hash of everything Google can hold of a contact (labels order-insensitive; local-only fields, ids and timestamps excluded).
pub fn content_hash(c: &Contact) -> String {
    hex(&Sha256::digest(google_view(c).to_string().as_bytes()))
}

/// Hash of the photo data URI; "" for no photo.
pub fn photo_hash(photo: Option<&str>) -> String {
    photo.filter(|s| !s.is_empty()).map(|s| hex(&Sha256::digest(s.as_bytes()))).unwrap_or_default()
}

// ───────────────────────────── helpers ─────────────────────────────

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn is_deleted(p: &Value) -> bool {
    p.get("metadata").and_then(|m| m.get("deleted")).and_then(Value::as_bool).unwrap_or(false)
}

fn sniff_mime(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if b.starts_with(b"GIF8") {
        "image/gif"
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

fn to_data_uri(bytes: &[u8]) -> String {
    format!("data:{};base64,{}", sniff_mime(bytes), base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn from_data_uri(uri: &str) -> Option<Vec<u8>> {
    let rest = uri.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()
}

/// Group ids Google makes up for user labels are hex; system groups ("friends", "starred", …) are words.
fn is_user_group_like(resource: &str) -> bool {
    resource.strip_prefix("contactGroups/").map(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit())).unwrap_or(false)
}

fn has_unknown_group(p: &Value, known: &HashMap<String, String>) -> bool {
    p.get("memberships").and_then(Value::as_array).into_iter().flatten().filter_map(|m| m.get("contactGroupMembership").and_then(|g| g.get("contactGroupResourceName")).and_then(Value::as_str)).any(|r| is_user_group_like(r) && !known.contains_key(r))
}

/// More than 10 and more than half of all linked contacts: not something a sync should do on its own.
fn is_mass(count: usize, total: usize) -> bool {
    count > 10 && count * 2 > total
}

// ───────────────────────────── the engine ─────────────────────────────

/// Runs one synchronisation. See the module documentation for the rules.
///
/// `state` is updated in place and (unless `dry_run`) also saved to `<store root>/sync/google.json`, even when the run fails
/// half-way, so that what was done is never forgotten.
pub fn sync(store: &mut Store, api: &dyn PeopleApi, state: &mut SyncState, opts: &SyncOptions) -> Result<SyncReport> {
    let (result, report) = {
        let mut e = Engine {
            store: &mut *store,
            api,
            state: &mut *state,
            opts,
            report: SyncReport::default(),
            backed_up: false,
            since_save: 0,
            group_names: HashMap::new(),
            group_res: HashMap::new(),
            groups_relisted: false,
            unmapped: None,
        };
        let r = e.run();
        (r, e.report)
    };
    if !opts.dry_run {
        state.version = 1;
        let saved = state.save(store.root());
        if result.is_ok() {
            saved?;
        }
    }
    result.map(|_| report)
}

struct Engine<'a> {
    store: &'a mut Store,
    api: &'a dyn PeopleApi,
    state: &'a mut SyncState,
    opts: &'a SyncOptions,
    report: SyncReport,
    backed_up: bool,
    since_save: usize,
    /// resource name → label name
    group_names: HashMap<String, String>,
    /// label name → resource name
    group_res: HashMap<String, String>,
    groups_relisted: bool,
    /// Unlinked local contacts by content hash, for adoption.
    unmapped: Option<HashMap<String, Vec<String>>>,
}

impl Engine<'_> {
    fn dry(&self) -> bool {
        self.opts.dry_run
    }

    fn run(&mut self) -> Result<()> {
        self.load_groups()?;
        self.pull()?;
        if self.opts.two_way {
            self.push()?;
        }
        Ok(())
    }

    fn warn(&mut self, msg: String) {
        self.report.warnings.push(msg);
    }

    fn plan(&mut self, kind: ActionKind, id: Option<&str>, resource_name: Option<&str>, name: String) {
        self.report.planned.push(PlannedAction { kind, id: id.map(str::to_string), resource_name: resource_name.map(str::to_string), name });
    }

    /// Saves the whole book once, before the first change of this run.
    fn backup(&mut self) -> Result<()> {
        if !self.dry() && !self.backed_up {
            self.store.backup(10)?;
            self.backed_up = true;
        }
        Ok(())
    }

    fn checkpoint(&mut self) -> Result<()> {
        if !self.dry() {
            self.state.version = 1;
            self.state.save(self.store.root())?;
            self.since_save = 0;
        }
        Ok(())
    }

    /// After each step that changes state; saves every few steps.
    fn touch(&mut self) -> Result<()> {
        self.since_save += 1;
        if self.since_save >= 10 {
            self.checkpoint()?;
        }
        Ok(())
    }

    fn save_conflict(&mut self, local: &Contact) -> Result<()> {
        let dir = self.store.root().join("conflicts");
        fs::create_dir_all(&dir)?;
        let p = dir.join(format!("{}-{}.vcf", local.id, now()));
        fs::write(p, vcard::write(local))?;
        Ok(())
    }

    /// Keeps Google's version of a contact before it is overwritten or deleted there.
    fn save_original(&mut self, person: &Value) -> Result<()> {
        let dir = self.store.root().join("sync");
        fs::create_dir_all(&dir)?;
        let p = dir.join("google-originals.jsonl");
        if fs::metadata(&p).map(|m| m.len() > 20 * 1024 * 1024).unwrap_or(false) {
            let _ = fs::rename(&p, dir.join("google-originals.old.jsonl"));
        }
        let mut f = fs::OpenOptions::new().create(true).append(true).open(p)?;
        writeln!(f, "{}", serde_json::json!({ "saved_at": now(), "person": person }))?;
        Ok(())
    }

    // ── labels ──

    fn load_groups(&mut self) -> Result<()> {
        let list = self.api.list_groups()?;
        let current: BTreeMap<String, String> = list.into_iter().filter(|g| g.2 == USER_GROUP && !g.0.is_empty() && !g.1.trim().is_empty()).map(|g| (g.0, g.1)).collect();
        if !self.dry() {
            for (res, old) in self.state.groups.clone() {
                match current.get(&res) {
                    Some(new) if *new == old => {}
                    Some(new) => self.relabel(&old, Some(new))?,
                    None if current.values().any(|n| n.eq_ignore_ascii_case(&old)) => {}
                    None => self.relabel(&old, None)?,
                }
            }
            self.state.groups = current.clone();
        }
        self.group_names = current.iter().map(|(r, n)| (r.clone(), n.clone())).collect();
        self.group_res = current.iter().map(|(r, n)| (n.clone(), r.clone())).collect();
        Ok(())
    }

    /// A label was renamed (`Some`) or deleted (`None`) on Google: follow on the linked contacts.
    fn relabel(&mut self, old: &str, new: Option<&str>) -> Result<()> {
        let ids: Vec<String> = self.state.links.keys().cloned().collect();
        let mut n = 0;
        for id in ids {
            let Some(c) = self.store.get(&id).cloned() else { continue };
            if !c.categories.iter().any(|x| x.eq_ignore_ascii_case(old)) {
                continue;
            }
            self.backup()?;
            let synced = self.state.link(&id).map(|l| l.hash == content_hash(&c)).unwrap_or(false);
            let mut updated = c.clone();
            let mut cats: Vec<String> = vec![];
            for x in &c.categories {
                let mapped = if x.eq_ignore_ascii_case(old) { new.map(str::to_string) } else { Some(x.clone()) };
                if let Some(m) = mapped {
                    if !cats.iter().any(|y| y.eq_ignore_ascii_case(&m)) {
                        cats.push(m);
                    }
                }
            }
            updated.categories = cats;
            self.store.put_unchanged(updated.clone())?;
            if synced {
                if let Some(l) = self.state.links.get_mut(&id) {
                    l.hash = content_hash(&updated);
                    l.remote_hash = l.hash.clone();
                }
            }
            n += 1;
        }
        if n > 0 {
            let msg = match new {
                Some(new) => format!("Label \"{old}\" was renamed to \"{new}\" on Google; updated {n} contact(s)."),
                None => format!("Label \"{old}\" was deleted on Google; removed it from {n} contact(s)."),
            };
            self.warn(msg);
        }
        Ok(())
    }

    /// Creates on Google the labels that do not exist there yet.
    fn ensure_groups(&mut self, categories: &[String]) -> Result<()> {
        for cat in categories {
            let name = cat.trim();
            if name.is_empty() || self.group_res.keys().any(|n| n.eq_ignore_ascii_case(name)) {
                continue;
            }
            let (res, gname, _) = self.api.create_group(name)?;
            self.group_names.insert(res.clone(), gname.clone());
            self.group_res.insert(gname.clone(), res.clone());
            self.state.groups.insert(res, gname);
            self.touch()?;
        }
        Ok(())
    }

    // ── pull ──

    fn pull(&mut self) -> Result<()> {
        let mut token = self.state.sync_token.clone();
        loop {
            match self.pull_pass(token.as_deref()) {
                Ok(()) => return Ok(()),
                Err(Error::Api(ApiError::SyncTokenExpired)) if token.is_some() => {
                    self.warn("The saved Google sync position had expired; the whole address book was read again.".into());
                    token = None;
                }
                Err(e) => return Err(e),
            }
        }
    }

    fn pull_pass(&mut self, token: Option<&str>) -> Result<()> {
        let full = token.is_none();
        self.report.full_sync = full;
        let mut seen: HashSet<String> = HashSet::new();
        let mut page_token: Option<String> = None;
        let mut new_token: Option<String> = None;
        loop {
            let page = self.api.list_connections(token, page_token.as_deref())?;
            for p in &page.people {
                let rn = str_of(p, "resourceName");
                if rn.is_empty() {
                    continue;
                }
                seen.insert(rn.to_string());
                self.process_person(p)?;
            }
            self.checkpoint()?;
            if page.next_sync_token.is_some() {
                new_token = page.next_sync_token;
            }
            match page.next_page_token {
                Some(t) => page_token = Some(t),
                None => break,
            }
        }
        if full {
            self.handle_absent(&seen)?;
        }
        match new_token {
            Some(t) => {
                if !self.dry() {
                    self.state.sync_token = Some(t);
                }
            }
            None => self.warn("Google gave no sync token; the next sync will read everything again.".into()),
        }
        Ok(())
    }

    fn process_person(&mut self, p: &Value) -> Result<()> {
        let rn = str_of(p, "resourceName");
        let id = self.state.id_of(rn).map(str::to_string);
        if is_deleted(p) {
            if let Some(id) = id {
                self.remote_deleted(&id)?;
            }
            return Ok(());
        }
        match id {
            Some(id) => self.pull_mapped(&id, p),
            None => self.pull_new(p),
        }
    }

    /// Downloads a photo; a failure is a warning, not an error.
    fn fetch_photo(&mut self, url: &str, who: &str) -> Option<String> {
        match self.api.download_photo(url) {
            Ok(b) if !b.is_empty() => Some(to_data_uri(&b)),
            Ok(_) => None,
            Err(e) => {
                self.warn(format!("Photo of {who} could not be downloaded: {e}"));
                None
            }
        }
    }

    fn unmapped_index(&mut self) -> &mut HashMap<String, Vec<String>> {
        if self.unmapped.is_none() {
            let mut m: HashMap<String, Vec<String>> = HashMap::new();
            for c in self.store.all() {
                if self.state.link(&c.id).is_none() && !c.is_blank() {
                    m.entry(content_hash(c)).or_default().push(c.id.clone());
                }
            }
            self.unmapped = Some(m);
        }
        self.unmapped.get_or_insert_with(HashMap::new)
    }

    fn refresh_groups_if_unknown(&mut self, p: &Value) -> Result<()> {
        if !self.groups_relisted && has_unknown_group(p, &self.group_names) {
            self.groups_relisted = true;
            self.load_groups()?;
        }
        Ok(())
    }

    fn pull_new(&mut self, p: &Value) -> Result<()> {
        let rn = str_of(p, "resourceName").to_string();
        self.refresh_groups_if_unknown(p)?;
        let mut c = person_to_contact(p, &self.group_names);
        if c.is_blank() {
            self.report.skipped += 1;
            return Ok(());
        }
        let remote_hash = content_hash(&c);
        let url = photo_url(p);
        let etag = str_of(p, "etag").to_string();

        // Already here? Only an identical contact is adopted.
        let adopt = self.unmapped_index().get_mut(&remote_hash).and_then(|v| (!v.is_empty()).then(|| v.remove(0)));
        if let Some(id) = adopt {
            if self.dry() {
                return Ok(());
            }
            let mut link = Link { resource_name: rn, etag, hash: remote_hash.clone(), remote_hash, photo_url: None, photo_hash: String::new() };
            if let Some(local) = self.store.get(&id).cloned() {
                link.photo_hash = photo_hash(local.photo.as_deref());
                if local.photo.is_none() {
                    if let Some(u) = &url {
                        if let Some(uri) = self.fetch_photo(u, &local.display_name(false)) {
                            self.backup()?;
                            let mut with_photo = local.clone();
                            link.photo_hash = photo_hash(Some(&uri));
                            with_photo.photo = Some(uri);
                            self.store.put_unchanged(with_photo)?;
                            link.photo_url = url.clone();
                            self.report.pulled_updated += 1;
                        }
                    }
                } else {
                    link.photo_url = url;
                }
            }
            self.state.set_link(&id, link);
            self.report.adopted += 1;
            return self.touch();
        }

        let name = c.display_name(false);
        if self.dry() {
            self.plan(ActionKind::PullNew, None, Some(&rn), name);
            return Ok(());
        }
        self.backup()?;
        c.id = new_id();
        c.created = now();
        c.updated = c.created;
        if c.name == Default::default() && c.orgs.iter().any(|o| !o.company.is_empty()) {
            c.kind = "org".into();
        }
        let mut link = Link { resource_name: rn, etag, hash: String::new(), remote_hash: remote_hash.clone(), photo_url: None, photo_hash: String::new() };
        if let Some(u) = &url {
            if let Some(uri) = self.fetch_photo(u, &name) {
                link.photo_hash = photo_hash(Some(&uri));
                link.photo_url = Some(u.clone());
                c.photo = Some(uri);
            }
        }
        link.hash = content_hash(&c);
        self.store.put_unchanged(c.clone())?;
        self.state.set_link(&c.id, link);
        self.report.pulled_new += 1;
        self.touch()
    }

    fn pull_mapped(&mut self, id: &str, p: &Value) -> Result<()> {
        let Some(link) = self.state.link(id).cloned() else { return Ok(()) };
        let etag = str_of(p, "etag").to_string();
        if !etag.is_empty() && etag == link.etag {
            return Ok(());
        }
        self.refresh_groups_if_unknown(p)?;
        let remote_c = person_to_contact(p, &self.group_names);
        let remote_hash = content_hash(&remote_c);
        let remote_photo = photo_url(p);
        let rn = link.resource_name.clone();

        let Some(local) = self.store.get(id).cloned() else {
            // Deleted in Kith, but changed on Google since: Google's version comes back (nothing is lost).
            if remote_c.is_blank() {
                return Ok(());
            }
            let name = remote_c.display_name(false);
            if self.dry() {
                self.plan(ActionKind::PullUpdate, Some(id), Some(&rn), name);
                return Ok(());
            }
            self.backup()?;
            let mut c = remote_c;
            c.id = id.to_string();
            c.created = now();
            c.updated = c.created;
            let mut new_link = Link { etag, hash: String::new(), remote_hash, photo_url: None, photo_hash: String::new(), ..link };
            if let Some(u) = &remote_photo {
                if let Some(uri) = self.fetch_photo(u, &name) {
                    new_link.photo_hash = photo_hash(Some(&uri));
                    new_link.photo_url = Some(u.clone());
                    c.photo = Some(uri);
                }
            }
            new_link.hash = content_hash(&c);
            self.store.put_unchanged(c)?;
            self.state.set_link(id, new_link);
            self.warn(format!("\"{name}\" was deleted in Kith but changed on Google, so it came back."));
            self.report.pulled_updated += 1;
            return self.touch();
        };

        let local_hash = content_hash(&local);
        let content_changed_locally = local_hash != link.hash;
        let remote_changed = remote_hash != link.remote_hash;
        let mut new_local = local.clone();
        let mut conflict = false;
        if remote_changed {
            apply_remote(&mut new_local, remote_c);
            conflict = content_changed_locally && content_hash(&new_local) != local_hash;
        }
        let photo_url_changed = remote_photo.is_some() && remote_photo != link.photo_url;
        let local_photo_changed = photo_hash(local.photo.as_deref()) != link.photo_hash;
        conflict |= photo_url_changed && local_photo_changed;

        let name = local.display_name(false);
        if self.dry() {
            if conflict {
                self.plan(ActionKind::Conflict, Some(id), Some(&rn), name);
            } else if remote_changed || photo_url_changed {
                self.plan(ActionKind::PullUpdate, Some(id), Some(&rn), name);
            }
            return Ok(());
        }

        let mut new_link = Link { etag, ..link.clone() };
        if remote_changed {
            new_link.remote_hash = remote_hash;
        }
        let mut changed = new_local != local;
        if photo_url_changed {
            if let Some(u) = &remote_photo {
                if let Some(uri) = self.fetch_photo(u, &name) {
                    new_link.photo_hash = photo_hash(Some(&uri));
                    new_link.photo_url = Some(u.clone());
                    new_local.photo = Some(uri);
                    changed = new_local != local;
                }
            }
        } else if remote_photo.is_none() {
            new_link.photo_url = None;
        }
        if conflict {
            self.backup()?;
            self.save_conflict(&local)?;
            self.report.conflicts += 1;
        }
        if changed {
            self.backup()?;
            self.store.put_unchanged(new_local.clone())?;
            if !conflict {
                self.report.pulled_updated += 1;
            }
        }
        if remote_changed {
            new_link.hash = content_hash(&new_local);
        }
        self.state.set_link(id, new_link);
        self.touch()
    }

    /// Google deleted (or no longer lists) a linked contact.
    fn remote_deleted(&mut self, id: &str) -> Result<()> {
        let Some(link) = self.state.link(id).cloned() else { return Ok(()) };
        let Some(local) = self.store.get(id).cloned() else {
            // already gone here
            if !self.dry() {
                self.state.remove_link(id);
                self.touch()?;
            }
            return Ok(());
        };
        let name = local.display_name(false);
        let edited = content_hash(&local) != link.hash;
        if self.dry() {
            let kind = if edited { ActionKind::Conflict } else { ActionKind::PullDelete };
            self.plan(kind, Some(id), Some(&link.resource_name), name);
            return Ok(());
        }
        self.backup()?;
        if edited {
            // Keep the unsynced edits; the contact becomes a new local one (a two-way sync sends it back).
            self.state.remove_link(id);
            self.report.conflicts += 1;
            self.warn(format!("\"{name}\" was deleted on Google but you changed it here, so it was kept."));
        } else {
            self.store.delete(id)?;
            self.state.remove_link(id);
            self.report.pulled_deleted += 1;
        }
        self.touch()
    }

    /// After a full read: linked contacts Google no longer lists were deleted there.
    fn handle_absent(&mut self, seen: &HashSet<String>) -> Result<()> {
        let absent: Vec<String> = self.state.links.iter().filter(|(_, l)| !seen.contains(&l.resource_name)).map(|(id, _)| id.clone()).collect();
        if absent.is_empty() {
            return Ok(());
        }
        if seen.is_empty() || is_mass(absent.len(), self.state.links.len()) {
            self.warn(format!("Google no longer lists {} of the {} linked contacts. That looks wrong (another account? a Google problem?), so nothing was deleted here.", absent.len(), self.state.links.len()));
            self.report.skipped += absent.len();
            return Ok(());
        }
        for id in absent {
            self.remote_deleted(&id)?;
        }
        Ok(())
    }

    // ── push ──

    fn photo_pending(c: &Contact, link: &Link) -> bool {
        c.photo.as_deref().map(|p| !p.is_empty()).unwrap_or(false) && photo_hash(c.photo.as_deref()) != link.photo_hash
    }

    fn push(&mut self) -> Result<()> {
        let ids: Vec<String> = self.store.all().map(|c| c.id.clone()).collect();
        let mut new_ids = vec![];
        let mut update_ids = vec![];
        for id in &ids {
            let Some(c) = self.store.get(id) else { continue };
            if c.is_blank() {
                continue;
            }
            match self.state.link(id) {
                None => new_ids.push(id.clone()),
                Some(l) => {
                    if content_hash(c) != l.hash || Self::photo_pending(c, l) {
                        update_ids.push(id.clone());
                    }
                }
            }
        }
        if self.dry() {
            for id in &new_ids {
                let name = self.store.get(id).map(|c| c.display_name(false)).unwrap_or_default();
                self.plan(ActionKind::PushNew, Some(id), None, name);
            }
            for id in &update_ids {
                let name = self.store.get(id).map(|c| c.display_name(false)).unwrap_or_default();
                let rn = self.state.link(id).map(|l| l.resource_name.clone());
                self.plan(ActionKind::PushUpdate, Some(id), rn.as_deref(), name);
            }
            return self.push_deletes();
        }
        for id in new_ids {
            self.push_new(&id)?;
        }
        for id in update_ids {
            self.push_update(&id)?;
        }
        self.push_deletes()
    }

    fn push_new(&mut self, id: &str) -> Result<()> {
        let Some(c) = self.store.get(id).cloned() else { return Ok(()) };
        self.ensure_groups(&c.categories)?;
        let person = contact_to_person(&c, &self.group_res);
        let created = self.api.create_contact(&person)?;
        let rn = str_of(&created, "resourceName").to_string();
        if rn.is_empty() {
            return Err(Error::State("Google created a contact but did not say which one".into()));
        }
        let hash = content_hash(&c);
        self.state.set_link(id, Link { resource_name: rn, etag: str_of(&created, "etag").to_string(), hash: hash.clone(), remote_hash: hash, photo_url: photo_url(&created), photo_hash: String::new() });
        self.report.pushed_new += 1;
        self.touch()?;
        self.push_photo(id)
    }

    /// Uploads the local photo if it is new. A failure of this alone is a warning.
    fn push_photo(&mut self, id: &str) -> Result<()> {
        let (Some(c), Some(link)) = (self.store.get(id).cloned(), self.state.link(id).cloned()) else { return Ok(()) };
        if !Self::photo_pending(&c, &link) {
            return Ok(());
        }
        let Some(bytes) = c.photo.as_deref().and_then(from_data_uri) else {
            self.warn(format!("The photo of {} is not in a format that can be sent to Google.", c.display_name(false)));
            return Ok(());
        };
        match self.api.set_photo(&link.resource_name, &bytes) {
            Ok(person) => {
                let mut l = link;
                if !str_of(&person, "etag").is_empty() {
                    l.etag = str_of(&person, "etag").to_string();
                }
                l.photo_url = photo_url(&person);
                l.photo_hash = photo_hash(c.photo.as_deref());
                self.state.set_link(id, l);
                self.touch()
            }
            Err(ApiError::Other(m)) => {
                self.warn(format!("The photo of {} could not be sent: {m}", c.display_name(false)));
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    }

    fn push_update(&mut self, id: &str) -> Result<()> {
        for _attempt in 0..2 {
            let (Some(c), Some(link)) = (self.store.get(id).cloned(), self.state.link(id).cloned()) else { return Ok(()) };
            if content_hash(&c) == link.hash {
                break;
            }
            let fresh = match self.api.get_contact(&link.resource_name) {
                Ok(p) => p,
                Err(ApiError::NotFound) => return self.recreate(id, &c),
                Err(e) => return Err(e.into()),
            };
            if str_of(&fresh, "etag") != link.etag {
                // Google changed it since we read it: Google wins (and the local version is kept), unless nothing mapped changed there.
                self.pull_mapped(id, &fresh)?;
                continue;
            }
            self.ensure_groups(&c.categories)?;
            let ours = contact_to_person(&c, &self.group_res);
            let body = merge_for_update(&fresh, ours, &self.group_res);
            self.save_original(&fresh)?;
            match self.api.update_contact(&link.resource_name, &link.etag, &body, UPDATE_PERSON_FIELDS) {
                Ok(updated) => {
                    let mut l = link;
                    if !str_of(&updated, "etag").is_empty() {
                        l.etag = str_of(&updated, "etag").to_string();
                    }
                    l.hash = content_hash(&c);
                    l.remote_hash = l.hash.clone();
                    self.state.set_link(id, l);
                    self.report.pushed_updated += 1;
                    self.touch()?;
                    break;
                }
                Err(ApiError::Conflict) => {
                    let fresh = self.api.get_contact(&link.resource_name)?;
                    self.pull_mapped(id, &fresh)?;
                }
                Err(ApiError::NotFound) => return self.recreate(id, &c),
                Err(e) => return Err(e.into()),
            }
        }
        if let (Some(c), Some(l)) = (self.store.get(id), self.state.link(id)) {
            if content_hash(c) != l.hash {
                self.report.skipped += 1;
                self.warn(format!("\"{}\" keeps changing on Google; its local changes were not sent this time.", c.display_name(false)));
                return Ok(());
            }
        }
        self.push_photo(id)
    }

    /// The linked contact no longer exists on Google although we saw no deletion: send it as a new one.
    fn recreate(&mut self, id: &str, c: &Contact) -> Result<()> {
        self.warn(format!("\"{}\" was missing on Google and was created there again.", c.display_name(false)));
        self.state.remove_link(id);
        self.push_new(id)
    }

    fn push_deletes(&mut self) -> Result<()> {
        let vanished: Vec<(String, String)> = self.state.links.iter().filter(|(id, _)| self.store.get(id).is_none()).map(|(id, l)| (id.clone(), l.resource_name.clone())).collect();
        if vanished.is_empty() {
            return Ok(());
        }
        if is_mass(vanished.len(), self.state.links.len()) {
            self.warn(format!("{} of the {} linked contacts are missing in Kith. That looks wrong, so nothing was deleted on Google.", vanished.len(), self.state.links.len()));
            self.report.skipped += vanished.len();
            return Ok(());
        }
        if !self.opts.allow_remote_deletes {
            self.report.skipped += vanished.len();
            self.warn(format!("{} contact(s) deleted in Kith still exist on Google; deleting on Google is switched off.", vanished.len()));
            return Ok(());
        }
        for (id, rn) in vanished {
            if self.dry() {
                self.plan(ActionKind::PushDelete, Some(&id), Some(&rn), rn.clone());
                continue;
            }
            match self.api.get_contact(&rn) {
                Ok(p) => self.save_original(&p)?,
                Err(ApiError::NotFound) => {
                    self.state.remove_link(&id);
                    self.touch()?;
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
            match self.api.delete_contact(&rn) {
                Ok(()) | Err(ApiError::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
            self.state.remove_link(&id);
            self.report.pushed_deleted += 1;
            self.touch()?;
        }
        Ok(())
    }
}

// ───────────────────────────── tests ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::google::api::{GroupInfo, Page};
    use crate::model::{Labeled, Org};
    use serde_json::json;
    use std::cell::RefCell;

    // ── a small in-memory Google ──

    #[derive(Default)]
    struct Inner {
        ver: u64,
        next_id: u64,
        people: BTreeMap<String, Value>,
        /// (version, resource name, deleted)
        changes: Vec<(u64, String, bool)>,
        groups: Vec<GroupInfo>,
        photos: HashMap<String, Vec<u8>>,
        calls: Vec<String>,
        epoch: u64,
        page_size: usize,
        fail_on: Option<String>,
    }

    type Race = Box<dyn FnOnce(&FakeGoogle)>;

    struct FakeGoogle {
        g: RefCell<Inner>,
        race: RefCell<Option<Race>>,
    }

    fn ok<T>(v: T) -> std::result::Result<T, ApiError> {
        Ok(v)
    }

    impl FakeGoogle {
        fn new() -> FakeGoogle {
            let g = Inner {
                page_size: 2,
                next_id: 100,
                groups: vec![
                    ("contactGroups/myContacts".into(), "myContacts".into(), "SYSTEM_CONTACT_GROUP".into()),
                    ("contactGroups/starred".into(), "starred".into(), "SYSTEM_CONTACT_GROUP".into()),
                    ("contactGroups/friends".into(), "friends".into(), "SYSTEM_CONTACT_GROUP".into()),
                ],
                ..Default::default()
            };
            FakeGoogle { g: RefCell::new(g), race: RefCell::new(None) }
        }

        fn calls(&self) -> Vec<String> {
            self.g.borrow().calls.clone()
        }
        fn mutations(&self) -> Vec<String> {
            self.calls().into_iter().filter(|c| c.starts_with("create") || c.starts_with("update") || c.starts_with("delete") || c.starts_with("set_photo")).collect()
        }
        fn clear_calls(&self) {
            self.g.borrow_mut().calls.clear();
        }
        fn check_fail(&self, name: &str) -> std::result::Result<(), ApiError> {
            let mut g = self.g.borrow_mut();
            g.calls.push(name.to_string());
            if g.fail_on.as_deref().map(|f| name.starts_with(f)).unwrap_or(false) {
                return Err(ApiError::Other("boom".into()));
            }
            Ok(())
        }
        fn add_group(&self, name: &str) -> String {
            let mut g = self.g.borrow_mut();
            g.next_id += 1;
            let res = format!("contactGroups/{:x}", g.next_id + 0xa000);
            g.groups.push((res.clone(), name.into(), USER_GROUP.into()));
            res
        }
        fn rename_group(&self, res: &str, name: &str) {
            for gr in self.g.borrow_mut().groups.iter_mut() {
                if gr.0 == res {
                    gr.1 = name.into();
                }
            }
        }
        fn remove_group(&self, res: &str) {
            self.g.borrow_mut().groups.retain(|gr| gr.0 != res);
        }

        /// Adds a person as another device would. `body` holds name etc.; returns the resource name.
        fn remote_add(&self, body: Value) -> String {
            let mut g = self.g.borrow_mut();
            g.ver += 1;
            g.next_id += 1;
            let rn = format!("people/c{}", g.next_id);
            let mut p = body;
            p["resourceName"] = json!(rn);
            p["etag"] = json!(format!("e{}", g.ver));
            p["metadata"] = json!({"sources": [{"type": "CONTACT", "id": rn.clone(), "etag": format!("s{}", g.ver)}]});
            if p.get("memberships").is_none() {
                p["memberships"] = json!([{"contactGroupMembership": {"contactGroupResourceName": "contactGroups/myContacts"}}]);
            }
            let ver = g.ver;
            g.changes.push((ver, rn.clone(), false));
            g.people.insert(rn.clone(), p);
            rn
        }
        fn remote_edit(&self, rn: &str, f: impl FnOnce(&mut Value)) {
            let mut g = self.g.borrow_mut();
            g.ver += 1;
            let ver = g.ver;
            let p = g.people.get_mut(rn).expect("person");
            f(p);
            p["etag"] = json!(format!("e{ver}"));
            g.changes.push((ver, rn.to_string(), false));
        }
        fn remote_delete(&self, rn: &str) {
            let mut g = self.g.borrow_mut();
            g.ver += 1;
            let ver = g.ver;
            g.people.remove(rn);
            g.changes.push((ver, rn.to_string(), true));
        }
        fn person(&self, rn: &str) -> Value {
            self.g.borrow().people.get(rn).cloned().expect("person")
        }
        fn rns(&self) -> Vec<String> {
            self.g.borrow().people.keys().cloned().collect()
        }
        fn expire_tokens(&self) {
            self.g.borrow_mut().epoch += 1;
        }
        fn set_page_size(&self, n: usize) {
            self.g.borrow_mut().page_size = n;
        }
        fn add_photo_bytes(&self, url: &str, bytes: &[u8]) {
            self.g.borrow_mut().photos.insert(url.into(), bytes.to_vec());
        }
    }

    impl PeopleApi for FakeGoogle {
        fn list_connections(&self, sync_token: Option<&str>, page_token: Option<&str>) -> std::result::Result<Page, ApiError> {
            self.check_fail("list")?;
            let g = self.g.borrow();
            let mut items: Vec<Value> = vec![];
            match sync_token {
                None => items.extend(g.people.values().cloned()),
                Some(t) => {
                    let parts: Vec<&str> = t.split('-').collect();
                    let (epoch, since): (u64, u64) = (parts.get(1).and_then(|x| x.parse().ok()).unwrap_or(0), parts.get(2).and_then(|x| x.parse().ok()).unwrap_or(0));
                    if parts.first() != Some(&"tok") || epoch != g.epoch {
                        return Err(ApiError::SyncTokenExpired);
                    }
                    let mut latest: BTreeMap<String, bool> = BTreeMap::new();
                    for (v, rn, del) in &g.changes {
                        if *v > since {
                            latest.insert(rn.clone(), *del);
                        }
                    }
                    for (rn, del) in latest {
                        if del {
                            items.push(json!({"resourceName": rn, "metadata": {"deleted": true, "objectType": "PERSON"}}));
                        } else if let Some(p) = g.people.get(&rn) {
                            items.push(p.clone());
                        }
                    }
                }
            }
            let start: usize = page_token.and_then(|t| t.parse().ok()).unwrap_or(0);
            let end = (start + g.page_size).min(items.len());
            let last = end >= items.len();
            ok(Page {
                people: items[start..end].to_vec(),
                next_page_token: (!last).then(|| end.to_string()),
                next_sync_token: last.then(|| format!("tok-{}-{}", g.epoch, g.ver)),
            })
        }

        fn list_groups(&self) -> std::result::Result<Vec<GroupInfo>, ApiError> {
            self.check_fail("list_groups")?;
            ok(self.g.borrow().groups.clone())
        }

        fn create_group(&self, name: &str) -> std::result::Result<GroupInfo, ApiError> {
            self.check_fail(&format!("create_group {name}"))?;
            if self.g.borrow().groups.iter().any(|g| g.1.eq_ignore_ascii_case(name)) {
                return Err(ApiError::Other("label exists".into()));
            }
            let res = self.add_group(name);
            ok((res, name.into(), USER_GROUP.into()))
        }

        fn get_contact(&self, rn: &str) -> std::result::Result<Value, ApiError> {
            self.check_fail(&format!("get {rn}"))?;
            if let Some(f) = self.race.borrow_mut().take() {
                f(self);
            }
            self.g.borrow().people.get(rn).cloned().ok_or(ApiError::NotFound)
        }

        fn create_contact(&self, person: &Value) -> std::result::Result<Value, ApiError> {
            self.check_fail("create")?;
            let rn = self.remote_add(person.clone());
            ok(self.person(&rn))
        }

        fn update_contact(&self, rn: &str, etag: &str, person: &Value, mask: &str) -> std::result::Result<Value, ApiError> {
            self.check_fail(&format!("update {rn}"))?;
            let mut g = self.g.borrow_mut();
            let Some(cur) = g.people.get(rn).cloned() else { return Err(ApiError::NotFound) };
            if cur["etag"] != json!(etag) {
                return Err(ApiError::Conflict);
            }
            if mask.split(',').any(|f| f == "memberships") && person.get("memberships").is_none() {
                return Err(ApiError::Other("memberships required".into()));
            }
            g.ver += 1;
            let ver = g.ver;
            let mut p = cur;
            for f in mask.split(',') {
                match person.get(f) {
                    Some(v) => p[f] = v.clone(),
                    None => {
                        if let Some(m) = p.as_object_mut() {
                            m.remove(f);
                        }
                    }
                }
            }
            p["etag"] = json!(format!("e{ver}"));
            g.changes.push((ver, rn.to_string(), false));
            g.people.insert(rn.to_string(), p.clone());
            ok(p)
        }

        fn delete_contact(&self, rn: &str) -> std::result::Result<(), ApiError> {
            self.check_fail(&format!("delete {rn}"))?;
            if !self.g.borrow().people.contains_key(rn) {
                return Err(ApiError::NotFound);
            }
            self.remote_delete(rn);
            ok(())
        }

        fn download_photo(&self, url: &str) -> std::result::Result<Vec<u8>, ApiError> {
            self.check_fail(&format!("download {url}"))?;
            self.g.borrow().photos.get(url).cloned().ok_or(ApiError::NotFound)
        }

        fn set_photo(&self, rn: &str, bytes: &[u8]) -> std::result::Result<Value, ApiError> {
            self.check_fail("set_photo")?;
            let url = format!("https://lh3.googleusercontent.com/p/{}-{}=s100", rn.trim_start_matches("people/"), self.g.borrow().ver + 1);
            self.add_photo_bytes(&sized(&url), bytes);
            self.remote_edit(rn, |p| p["photos"] = json!([{"url": url, "metadata": {"primary": true}}]));
            ok(self.person(rn))
        }
    }

    /// The URL the engine will ask for (mapping resizes it to 256).
    fn sized(url: &str) -> String {
        crate::google::mapping::sized_photo_url(url, 256)
    }

    // ── fixtures ──

    fn rig() -> (tempfile::TempDir, Store, FakeGoogle, SyncState) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store, FakeGoogle::new(), SyncState::default())
    }

    fn person(given: &str, family: &str, phone: &str) -> Value {
        json!({
            "names": [{"givenName": given, "familyName": family}],
            "phoneNumbers": [{"value": phone, "type": "mobile"}],
            "emailAddresses": [{"value": format!("{}@example.com", given.to_lowercase()), "type": "home"}],
        })
    }

    fn run(store: &mut Store, g: &FakeGoogle, st: &mut SyncState, two_way: bool) -> SyncReport {
        sync(store, g, st, &SyncOptions { two_way, ..Default::default() }).unwrap()
    }

    fn by_name<'a>(store: &'a Store, given: &str) -> &'a Contact {
        store.all().find(|c| c.name.given == given).unwrap_or_else(|| panic!("no contact {given}"))
    }

    fn backups(dir: &Path) -> usize {
        fs::read_dir(dir.join("backups")).map(|r| r.count()).unwrap_or(0)
    }

    // ── tests ──

    #[test]
    fn first_pull_creates_contacts_labels_photos_and_links() {
        let (dir, mut store, g, mut st) = rig();
        let fam = g.add_group("Family");
        for (n, f) in [("Ann", "A"), ("Bob", "B"), ("Cy", "C"), ("Di", "D"), ("Ed", "E")] {
            let mut p = person(n, f, "+1 555 0100");
            if n == "Ann" {
                p["memberships"] = json!([{"contactGroupMembership": {"contactGroupResourceName": "contactGroups/myContacts"}}, {"contactGroupMembership": {"contactGroupResourceName": "contactGroups/starred"}}, {"contactGroupMembership": {"contactGroupResourceName": fam}}]);
                p["photos"] = json!([{"url": "https://lh3.googleusercontent.com/contacts/ann=s100", "metadata": {"primary": true}}]);
            }
            g.remote_add(p);
        }
        g.add_photo_bytes("https://lh3.googleusercontent.com/contacts/ann=s256", &[0xff, 0xd8, 0xff, 1, 2, 3]);
        let r = run(&mut store, &g, &mut st, false);
        assert_eq!((r.pulled_new, r.pulled_updated, r.conflicts), (5, 0, 0));
        assert!(r.full_sync && r.warnings.is_empty(), "{:?}", r.warnings);
        assert_eq!(store.len(), 5);
        let ann = by_name(&store, "Ann");
        assert!(ann.favorite);
        assert_eq!(ann.categories, vec!["Family"]);
        assert!(ann.photo.as_deref().unwrap().starts_with("data:image/jpeg;base64,"));
        assert_eq!(ann.phones[0], Labeled::new("mobile", "+1 555 0100"));
        assert_eq!(st.links.len(), 5);
        assert!(st.sync_token.is_some());
        assert_eq!(st.groups.len(), 1);
        assert_eq!(backups(dir.path()), 1);
        // state was saved and reloads identically
        let loaded = SyncState::load(dir.path()).unwrap();
        assert_eq!(loaded.links, st.links);
        assert_eq!(loaded.id_of(&st.link(&ann.id).unwrap().resource_name), Some(ann.id.as_str()));
        assert!(g.mutations().is_empty());
    }

    #[test]
    fn incremental_pull_applies_only_changes_and_keeps_local_fields() {
        let (_d, mut store, g, mut st) = rig();
        let a = g.remote_add(person("Ann", "A", "1"));
        g.remote_add(person("Bob", "B", "2"));
        run(&mut store, &g, &mut st, false);
        // local-only data on Ann, timestamps of Bob
        let mut ann = by_name(&store, "Ann").clone();
        ann.timezone = "Europe/Kyiv".into();
        store.put_unchanged(ann.clone()).unwrap();
        let st_before = st.clone();
        g.clear_calls();
        g.remote_edit(&a, |p| p["phoneNumbers"] = json!([{"value": "999", "type": "work"}]));
        g.remote_add(person("Cy", "C", "3"));
        let bob_updated = by_name(&store, "Bob").updated;
        let r = run(&mut store, &g, &mut st, false);
        assert_eq!((r.pulled_new, r.pulled_updated, r.conflicts, r.full_sync), (1, 1, 0, false));
        let ann2 = by_name(&store, "Ann");
        assert_eq!(ann2.phones, vec![Labeled::new("work", "999")]);
        assert_eq!(ann2.timezone, "Europe/Kyiv");
        assert_eq!(ann2.updated, ann.updated, "a pull does not bump `updated`");
        assert_eq!(by_name(&store, "Bob").updated, bob_updated);
        assert_ne!(st.sync_token, st_before.sync_token);
        assert!(g.calls().iter().all(|c| c.starts_with("list")), "{:?}", g.calls());
        // nothing new: quiet
        let r = run(&mut store, &g, &mut st, false);
        assert!(r.is_quiet());
    }

    #[test]
    fn remote_delete_goes_to_trash_and_edited_local_is_kept() {
        let (_d, mut store, g, mut st) = rig();
        let a = g.remote_add(person("Ann", "A", "1"));
        let b = g.remote_add(person("Bob", "B", "2"));
        run(&mut store, &g, &mut st, false);
        let mut bob = by_name(&store, "Bob").clone();
        bob.notes = "my note".into();
        store.save(bob.clone()).unwrap();
        g.remote_delete(&a);
        g.remote_delete(&b);
        let r = run(&mut store, &g, &mut st, false);
        assert_eq!((r.pulled_deleted, r.conflicts), (1, 1));
        assert!(store.all().all(|c| c.name.given != "Ann"));
        assert_eq!(store.trash().len(), 1);
        assert_eq!(by_name(&store, "Bob").notes, "my note");
        assert_eq!(st.links.len(), 0, "both links are gone; Bob is now a plain local contact");
        // with two-way on, Bob is sent back as a new contact
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!(r.pushed_new, 1);
        assert_eq!(g.rns().len(), 1);
    }

    #[test]
    fn local_edit_is_pushed_keeping_google_only_data() {
        let (dir, mut store, g, mut st) = rig();
        let rn = g.remote_add(json!({
            "names": [{"givenName": "Ann", "familyName": "A"}],
            "organizations": [{"name": "Acme", "title": "Boss", "startDate": {"year": 2020}, "current": true}],
            "externalIds": [{"value": "emp-7", "type": "employee"}],
            "memberships": [{"contactGroupMembership": {"contactGroupResourceName": "contactGroups/myContacts"}}, {"contactGroupMembership": {"contactGroupResourceName": "contactGroups/friends"}}],
        }));
        run(&mut store, &g, &mut st, true);
        let mut ann = by_name(&store, "Ann").clone();
        ann.notes = "called today".into();
        ann.timezone = "UTC".into();
        store.save(ann).unwrap();
        g.clear_calls();
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!((r.pushed_updated, r.pushed_new, r.pulled_updated, r.conflicts), (1, 0, 0, 0), "{:?}", r.warnings);
        let remote = g.person(&rn);
        assert_eq!(remote["biographies"][0]["value"], "called today");
        assert_eq!(remote["organizations"][0]["startDate"], json!({"year": 2020}), "org data Kith does not model survives");
        assert_eq!(remote["externalIds"][0]["value"], "emp-7", "fields outside the mask survive");
        assert!(remote["memberships"].to_string().contains("contactGroups/friends"), "system group membership survives");
        assert!(!remote.to_string().contains("UTC"), "local-only fields never leave");
        assert!(dir.path().join("sync/google-originals.jsonl").exists());
        // echo: the pushed contact is not pulled/pushed again
        g.clear_calls();
        let r = run(&mut store, &g, &mut st, true);
        assert!(r.is_quiet(), "{r:?}");
        assert!(g.mutations().is_empty());
        let r = run(&mut store, &g, &mut st, true);
        assert!(r.is_quiet());
    }

    #[test]
    fn local_create_is_pushed_with_label_and_photo() {
        let (_d, mut store, g, mut st) = rig();
        run(&mut store, &g, &mut st, true);
        let mut c = Contact::new();
        c.name.given = "Zed".into();
        c.phones = vec![Labeled::new("home", "123")];
        c.categories = vec!["Climbers".into()];
        c.favorite = true;
        c.photo = Some(to_data_uri(&[0xff, 0xd8, 0xff, 9]));
        let c = store.save(c).unwrap();
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!((r.pushed_new, r.pulled_new), (1, 0), "{:?}", r.warnings);
        let rn = st.link(&c.id).unwrap().resource_name.clone();
        let remote = g.person(&rn);
        assert_eq!(remote["names"][0]["givenName"], "Zed");
        let ms = remote["memberships"].to_string();
        assert!(ms.contains("starred") && ms.contains("contactGroups/a"), "{ms}");
        assert!(g.g.borrow().groups.iter().any(|x| x.1 == "Climbers" && x.2 == USER_GROUP));
        assert!(g.mutations().iter().any(|m| m == "set_photo"));
        // everything settled: next syncs are quiet and the local copy is unchanged
        let before = store.get(&c.id).unwrap().clone();
        let r = run(&mut store, &g, &mut st, true);
        assert!(r.is_quiet(), "{r:?} {:?}", r.warnings);
        assert_eq!(store.get(&c.id).unwrap().phones, before.phones);
        assert_eq!(store.get(&c.id).unwrap().categories, vec!["Climbers"]);
        assert_eq!(g.rns().len(), 1);
    }

    #[test]
    fn one_way_pushes_nothing() {
        let (_d, mut store, g, mut st) = rig();
        g.remote_add(person("Ann", "A", "1"));
        run(&mut store, &g, &mut st, false);
        let mut ann = by_name(&store, "Ann").clone();
        ann.notes = "local edit".into();
        store.save(ann).unwrap();
        let mut c = Contact::new();
        c.name.given = "New".into();
        store.save(c).unwrap();
        let victim = by_name(&store, "Ann").id.clone();
        g.clear_calls();
        let r = run(&mut store, &g, &mut st, false);
        assert!(r.is_quiet());
        assert!(g.mutations().is_empty());
        assert!(g.calls().iter().all(|c| c.starts_with("list")));
        // deleting locally is not propagated either
        store.delete(&victim).unwrap();
        run(&mut store, &g, &mut st, false);
        assert_eq!(g.rns().len(), 1);
    }

    #[test]
    fn conflict_remote_wins_and_local_is_saved() {
        let (dir, mut store, g, mut st) = rig();
        let rn = g.remote_add(person("Ann", "A", "1"));
        run(&mut store, &g, &mut st, true);
        let mut ann = by_name(&store, "Ann").clone();
        ann.notes = "local note".into();
        store.save(ann.clone()).unwrap();
        g.remote_edit(&rn, |p| p["biographies"] = json!([{"value": "remote note"}]));
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!(r.conflicts, 1);
        assert_eq!(by_name(&store, "Ann").notes, "remote note");
        let files: Vec<_> = fs::read_dir(dir.path().join("conflicts")).unwrap().flatten().collect();
        assert_eq!(files.len(), 1);
        assert!(fs::read_to_string(files[0].path()).unwrap().contains("local note"));
        assert!(g.mutations().is_empty(), "the local version is not pushed over Google's");
        assert!(run(&mut store, &g, &mut st, true).is_quiet());
    }

    #[test]
    fn conflict_detected_at_push_time_when_google_moves_in_between() {
        let (dir, mut store, g, mut st) = rig();
        let rn = g.remote_add(person("Ann", "A", "1"));
        run(&mut store, &g, &mut st, true);
        let mut ann = by_name(&store, "Ann").clone();
        ann.notes = "local note".into();
        store.save(ann).unwrap();
        let rn2 = rn.clone();
        *g.race.borrow_mut() = Some(Box::new(move |g: &FakeGoogle| g.remote_edit(&rn2, |p| p["biographies"] = json!([{"value": "sneaky remote"}]))));
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!(r.conflicts, 1, "{:?}", r.warnings);
        assert_eq!(by_name(&store, "Ann").notes, "sneaky remote");
        assert_eq!(fs::read_dir(dir.path().join("conflicts")).unwrap().count(), 1);
        assert!(g.mutations().is_empty());
    }

    #[test]
    fn unmapped_google_change_does_not_block_a_push() {
        let (_d, mut store, g, mut st) = rig();
        let rn = g.remote_add(person("Ann", "A", "1"));
        run(&mut store, &g, &mut st, true);
        let mut ann = by_name(&store, "Ann").clone();
        ann.notes = "local note".into();
        store.save(ann).unwrap();
        // Google changes something Kith does not look at
        g.remote_edit(&rn, |p| p["externalIds"] = json!([{"value": "x"}]));
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!((r.conflicts, r.pushed_updated), (0, 1), "{:?}", r.warnings);
        assert_eq!(g.person(&rn)["biographies"][0]["value"], "local note");
        assert_eq!(g.person(&rn)["externalIds"][0]["value"], "x");
    }

    #[test]
    fn expired_sync_token_triggers_full_resync_and_notices_deletions() {
        let (_d, mut store, g, mut st) = rig();
        let a = g.remote_add(person("Ann", "A", "1"));
        g.remote_add(person("Bob", "B", "2"));
        g.remote_add(person("Cy", "C", "3"));
        run(&mut store, &g, &mut st, false);
        g.remote_delete(&a);
        g.remote_add(person("Di", "D", "4"));
        g.expire_tokens();
        let r = run(&mut store, &g, &mut st, false);
        assert!(r.full_sync);
        assert!(r.warnings.iter().any(|w| w.contains("expired")), "{:?}", r.warnings);
        assert_eq!((r.pulled_new, r.pulled_deleted, r.pulled_updated), (1, 1, 0));
        assert_eq!(store.len(), 3);
        assert_eq!(st.links.len(), 3);
        assert!(run(&mut store, &g, &mut st, false).is_quiet());
    }

    #[test]
    fn dry_run_changes_nothing() {
        let (dir, mut store, g, mut st) = rig();
        let a = g.remote_add(person("Ann", "A", "1"));
        g.remote_add(person("Bob", "B", "2"));
        run(&mut store, &g, &mut st, true);
        let n_backups = backups(dir.path());
        // changes on both sides
        g.remote_edit(&a, |p| p["biographies"] = json!([{"value": "r"}]));
        g.remote_add(person("Cy", "C", "3"));
        let mut bob = by_name(&store, "Bob").clone();
        bob.notes = "local".into();
        store.save(bob).unwrap();
        let mut d = Contact::new();
        d.name.given = "Di".into();
        store.save(d).unwrap();
        let (st_before, files_before, remote_before) = (st.clone(), store.export_vcf(None), g.g.borrow().people.clone());
        let state_file = fs::read(SyncState::path(dir.path())).unwrap();
        g.clear_calls();
        let r = sync(&mut store, &g, &mut st, &SyncOptions { two_way: true, allow_remote_deletes: true, dry_run: true }).unwrap();
        let kinds: Vec<ActionKind> = r.planned.iter().map(|p| p.kind).collect();
        assert!(kinds.contains(&ActionKind::PullNew) && kinds.contains(&ActionKind::PullUpdate) && kinds.contains(&ActionKind::PushNew) && kinds.contains(&ActionKind::PushUpdate), "{kinds:?}");
        assert!(g.mutations().is_empty());
        assert_eq!(st, st_before);
        assert_eq!(store.export_vcf(None), files_before);
        assert_eq!(g.g.borrow().people, remote_before);
        assert_eq!(fs::read(SyncState::path(dir.path())).unwrap(), state_file);
        assert_eq!(backups(dir.path()), n_backups);
        assert_eq!(r.pulled_new + r.pushed_new + r.pulled_updated + r.pushed_updated, 0);
        assert!(!dir.path().join("conflicts").exists());
    }

    #[test]
    fn backup_is_made_before_changes_and_not_when_quiet() {
        let (dir, mut store, g, mut st) = rig();
        let mut c = Contact::new();
        c.name.given = "Mine".into();
        store.save(c).unwrap();
        g.remote_add(person("Ann", "A", "1"));
        assert_eq!(backups(dir.path()), 0);
        run(&mut store, &g, &mut st, false);
        assert_eq!(backups(dir.path()), 1);
        let text = fs::read_to_string(fs::read_dir(dir.path().join("backups")).unwrap().next().unwrap().unwrap().path()).unwrap();
        assert!(text.contains("Mine") && !text.contains("Ann"), "the backup is the state before the sync");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        run(&mut store, &g, &mut st, false);
        assert_eq!(backups(dir.path()), 1, "a quiet sync does not rotate the backups away");
    }

    #[test]
    fn adopts_identical_local_contacts_instead_of_duplicating() {
        let (_d, mut store, g, mut st) = rig();
        g.remote_add(person("Ann", "A", "1"));
        g.remote_add(person("Bob", "B", "2"));
        // a local copy of Ann (as if imported from Google's export) and a different Bob
        let mut ann = Contact::new();
        ann.name.given = "Ann".into();
        ann.name.family = "A".into();
        ann.phones = vec![Labeled::new("mobile", "1")];
        ann.emails = vec![Labeled::new("home", "ann@example.com")];
        ann.timezone = "UTC".into();
        let ann = store.save(ann).unwrap();
        let mut bob = Contact::new();
        bob.name.given = "Bob".into();
        bob.name.family = "B".into();
        bob.phones = vec![Labeled::new("mobile", "other number")];
        store.save(bob).unwrap();
        let r = run(&mut store, &g, &mut st, false);
        assert_eq!((r.adopted, r.pulled_new), (1, 1));
        assert_eq!(store.len(), 3, "the near-match Bob is not merged");
        assert!(st.link(&ann.id).is_some());
        assert_eq!(store.get(&ann.id).unwrap().timezone, "UTC");
        // two-way afterwards: Ann is not pushed again, the local Bob is created on Google (duplicate by choice)
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!(r.pushed_new, 1);
        assert_eq!(g.rns().len(), 3);
    }

    #[test]
    fn local_delete_is_reported_or_deleted_on_request() {
        let (_d, mut store, g, mut st) = rig();
        g.remote_add(person("Ann", "A", "1"));
        g.remote_add(person("Bob", "B", "2"));
        run(&mut store, &g, &mut st, true);
        let ann = by_name(&store, "Ann").id.clone();
        store.delete(&ann).unwrap();
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!((r.pushed_deleted, r.skipped), (0, 1));
        assert!(r.warnings.iter().any(|w| w.contains("switched off")));
        assert_eq!(g.rns().len(), 2);
        let r = sync(&mut store, &g, &mut st, &SyncOptions { two_way: true, allow_remote_deletes: true, dry_run: false }).unwrap();
        assert_eq!(r.pushed_deleted, 1);
        assert_eq!(g.rns().len(), 1);
        assert!(st.link(&ann).is_none());
        let originals = fs::read_to_string(store.root().join("sync/google-originals.jsonl")).unwrap();
        assert!(originals.contains("Ann"), "the deleted Google version was saved");
        assert!(run(&mut store, &g, &mut st, true).is_quiet());
    }

    #[test]
    fn mass_deletions_are_refused() {
        let (_d, mut store, g, mut st) = rig();
        g.set_page_size(50);
        for i in 0..14 {
            g.remote_add(person(&format!("P{i}"), "X", &format!("{i}")));
        }
        run(&mut store, &g, &mut st, true);
        assert_eq!(store.len(), 14);
        // all contacts vanish locally (e.g. wrong folder): Google must not be emptied
        let ids: Vec<String> = store.all().map(|c| c.id.clone()).collect();
        for id in ids {
            store.delete(&id).unwrap();
        }
        let r = sync(&mut store, &g, &mut st, &SyncOptions { two_way: true, allow_remote_deletes: true, dry_run: false }).unwrap();
        assert_eq!(r.pushed_deleted, 0);
        assert!(r.warnings.iter().any(|w| w.contains("looks wrong")));
        assert_eq!(g.rns().len(), 14);
        // and the other way: an expired token plus an empty answer must not empty the book
        let (_d2, mut store2, g2, mut st2) = rig();
        for i in 0..14 {
            g2.remote_add(person(&format!("Q{i}"), "X", &format!("{i}")));
        }
        g2.set_page_size(50);
        run(&mut store2, &g2, &mut st2, false);
        for rn in g2.rns() {
            g2.remote_delete(&rn);
        }
        g2.expire_tokens();
        let r = run(&mut store2, &g2, &mut st2, false);
        assert_eq!(r.pulled_deleted, 0);
        assert_eq!(store2.len(), 14);
        assert!(r.warnings.iter().any(|w| w.contains("looks wrong")));
    }

    #[test]
    fn api_error_midway_keeps_state_consistent_and_a_rerun_finishes() {
        let (dir, mut store, g, mut st) = rig();
        for i in 0..6 {
            g.remote_add(person(&format!("P{i}"), "X", &format!("{i}")));
        }
        // page size 2: the first two list pages succeed, the third fails
        struct Flaky<'a> {
            inner: &'a FakeGoogle,
            list_calls: std::cell::Cell<u32>,
            fail_at: u32,
        }
        impl PeopleApi for Flaky<'_> {
            fn list_connections(&self, s: Option<&str>, p: Option<&str>) -> std::result::Result<Page, ApiError> {
                self.list_calls.set(self.list_calls.get() + 1);
                if self.list_calls.get() == self.fail_at {
                    return Err(ApiError::Other("network down".into()));
                }
                self.inner.list_connections(s, p)
            }
            fn list_groups(&self) -> std::result::Result<Vec<GroupInfo>, ApiError> {
                self.inner.list_groups()
            }
            fn create_group(&self, n: &str) -> std::result::Result<GroupInfo, ApiError> {
                self.inner.create_group(n)
            }
            fn get_contact(&self, r: &str) -> std::result::Result<Value, ApiError> {
                self.inner.get_contact(r)
            }
            fn create_contact(&self, p: &Value) -> std::result::Result<Value, ApiError> {
                self.inner.create_contact(p)
            }
            fn update_contact(&self, r: &str, e: &str, p: &Value, m: &str) -> std::result::Result<Value, ApiError> {
                self.inner.update_contact(r, e, p, m)
            }
            fn delete_contact(&self, r: &str) -> std::result::Result<(), ApiError> {
                self.inner.delete_contact(r)
            }
            fn download_photo(&self, u: &str) -> std::result::Result<Vec<u8>, ApiError> {
                self.inner.download_photo(u)
            }
            fn set_photo(&self, r: &str, b: &[u8]) -> std::result::Result<Value, ApiError> {
                self.inner.set_photo(r, b)
            }
        }
        let flaky = Flaky { inner: &g, list_calls: std::cell::Cell::new(0), fail_at: 3 };
        let err = sync(&mut store, &flaky, &mut st, &SyncOptions::default()).unwrap_err();
        assert!(matches!(err, Error::Api(ApiError::Other(_))));
        assert_eq!(store.len(), 4, "two pages were applied");
        assert_eq!(st.links.len(), 4);
        assert_eq!(st.sync_token, None, "the cursor did not move");
        assert_eq!(SyncState::load(dir.path()).unwrap().links.len(), 4, "progress was saved");
        let mut st2 = SyncState::load(dir.path()).unwrap();
        let r = run(&mut store, &g, &mut st2, false);
        assert_eq!(store.len(), 6, "no duplicates after the retry");
        assert_eq!(r.pulled_new, 2);
        assert!(st2.sync_token.is_some());
    }

    #[test]
    fn label_rename_and_delete_on_google_follow_to_contacts() {
        let (_d, mut store, g, mut st) = rig();
        let fam = g.add_group("Family");
        let work = g.add_group("Work");
        let mk = |res: &[&str]| {
            let mut ms = vec![json!({"contactGroupMembership": {"contactGroupResourceName": "contactGroups/myContacts"}})];
            ms.extend(res.iter().map(|r| json!({"contactGroupMembership": {"contactGroupResourceName": r}})));
            json!(ms)
        };
        let mut a = person("Ann", "A", "1");
        a["memberships"] = mk(&[&fam, &work]);
        g.remote_add(a);
        run(&mut store, &g, &mut st, true);
        assert_eq!(by_name(&store, "Ann").categories, vec!["Family", "Work"]);
        g.rename_group(&fam, "Relatives");
        g.remove_group(&work);
        g.clear_calls();
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!(by_name(&store, "Ann").categories, vec!["Relatives"], "{:?}", r.warnings);
        assert_eq!(r.warnings.len(), 2);
        assert!(g.mutations().is_empty(), "following Google's label change is not a local edit to push");
        assert!(run(&mut store, &g, &mut st, true).is_quiet());
    }

    #[test]
    fn label_created_by_another_device_during_pull_is_picked_up() {
        let (_d, mut store, g, mut st) = rig();
        g.remote_add(person("Seed", "S", "0"));
        run(&mut store, &g, &mut st, false);
        // group appears after our group listing: contact refers to an unknown label
        let late = g.add_group("Late");
        let mut p = person("Ann", "A", "1");
        p["memberships"] = json!([{"contactGroupMembership": {"contactGroupResourceName": late}}]);
        g.remote_add(p);
        let r = run(&mut store, &g, &mut st, false);
        assert_eq!(r.pulled_new, 1);
        assert_eq!(by_name(&store, "Ann").categories, vec!["Late"]);
    }

    #[test]
    fn contact_missing_on_google_is_recreated_on_update() {
        let (_d, mut store, g, mut st) = rig();
        let rn = g.remote_add(person("Ann", "A", "1"));
        run(&mut store, &g, &mut st, true);
        // Google forgets it without telling us (no change record)
        g.g.borrow_mut().people.remove(&rn);
        let mut ann = by_name(&store, "Ann").clone();
        ann.notes = "edit".into();
        store.save(ann).unwrap();
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!(r.pushed_new, 1, "{:?}", r.warnings);
        assert!(r.warnings.iter().any(|w| w.contains("again")));
        assert_eq!(g.rns().len(), 1);
    }

    #[test]
    fn deleted_locally_but_changed_on_google_comes_back() {
        let (_d, mut store, g, mut st) = rig();
        let rn = g.remote_add(person("Ann", "A", "1"));
        run(&mut store, &g, &mut st, false);
        let id = by_name(&store, "Ann").id.clone();
        store.delete(&id).unwrap();
        g.remote_edit(&rn, |p| p["biographies"] = json!([{"value": "new"}]));
        let r = run(&mut store, &g, &mut st, false);
        assert_eq!(r.pulled_updated, 1);
        assert_eq!(store.get(&id).unwrap().notes, "new");
    }

    #[test]
    fn org_only_contacts_become_company_cards_and_blank_people_are_skipped() {
        let (_d, mut store, g, mut st) = rig();
        g.remote_add(json!({"organizations": [{"name": "Pizza Place"}], "phoneNumbers": [{"value": "555"}]}));
        g.remote_add(json!({}));
        let r = run(&mut store, &g, &mut st, false);
        assert_eq!((r.pulled_new, r.skipped), (1, 1));
        let c = store.all().next().unwrap();
        assert!(c.is_org());
        assert_eq!(c.orgs, vec![Org { company: "Pizza Place".into(), ..Org::default() }]);
    }

    #[test]
    fn failed_photo_download_is_a_warning() {
        let (_d, mut store, g, mut st) = rig();
        let mut p = person("Ann", "A", "1");
        p["photos"] = json!([{"url": "https://lh3.googleusercontent.com/contacts/missing=s100"}]);
        g.remote_add(p);
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!(r.pulled_new, 1);
        assert!(r.warnings.iter().any(|w| w.contains("Photo")));
        assert!(by_name(&store, "Ann").photo.is_none());
        assert!(g.mutations().is_empty());
    }

    #[test]
    fn remote_photo_change_replaces_local_photo_and_local_photo_is_pushed() {
        let (_d, mut store, g, mut st) = rig();
        let mut p = person("Ann", "A", "1");
        p["photos"] = json!([{"url": "https://lh3.googleusercontent.com/contacts/v1=s100"}]);
        let rn = g.remote_add(p);
        g.add_photo_bytes("https://lh3.googleusercontent.com/contacts/v1=s256", &[0xff, 0xd8, 0xff, 1]);
        g.add_photo_bytes("https://lh3.googleusercontent.com/contacts/v2=s256", &[0xff, 0xd8, 0xff, 2]);
        run(&mut store, &g, &mut st, true);
        let first = by_name(&store, "Ann").photo.clone().unwrap();
        g.remote_edit(&rn, |p| p["photos"] = json!([{"url": "https://lh3.googleusercontent.com/contacts/v2=s100"}]));
        let r = run(&mut store, &g, &mut st, true);
        assert_eq!(r.pulled_updated, 1);
        let second = by_name(&store, "Ann").photo.clone().unwrap();
        assert_ne!(first, second);
        // now a local photo change goes up
        let mut ann = by_name(&store, "Ann").clone();
        ann.photo = Some(to_data_uri(&[0xff, 0xd8, 0xff, 7, 7]));
        store.save(ann).unwrap();
        g.clear_calls();
        let r = run(&mut store, &g, &mut st, true);
        assert!(g.mutations().iter().any(|m| m == "set_photo"), "{:?} {:?}", g.calls(), r.warnings);
        assert!(run(&mut store, &g, &mut st, true).is_quiet());
        assert!(g.mutations().len() == 1 || g.calls().iter().filter(|c| *c == "set_photo").count() == 1);
    }

    #[test]
    fn state_file_round_trip_and_damage() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(SyncState::load(dir.path()).unwrap().links.len(), 0);
        let mut s = SyncState { sync_token: Some("t".into()), ..Default::default() };
        s.groups.insert("contactGroups/1".into(), "Family".into());
        s.set_link("id1", Link { resource_name: "people/c1".into(), etag: "e".into(), hash: "h".into(), remote_hash: "r".into(), photo_url: Some("u".into()), photo_hash: "p".into() });
        s.save(dir.path()).unwrap();
        assert!(!dir.path().join("sync/google.json.tmp").exists());
        let l = SyncState::load(dir.path()).unwrap();
        assert_eq!(l.links, s.links);
        assert_eq!(l.id_of("people/c1"), Some("id1"));
        assert_eq!((l.sync_token.as_deref(), l.groups.len()), (Some("t"), 1));
        fs::write(SyncState::path(dir.path()), "{not json").unwrap();
        assert!(matches!(SyncState::load(dir.path()), Err(Error::State(_))));
    }

    #[test]
    fn content_hash_ignores_local_only_things_and_label_order() {
        let mut a = Contact::new();
        a.name.given = "A".into();
        a.categories = vec!["x".into(), "y".into()];
        let mut b = a.clone();
        b.categories.reverse();
        b.timezone = "UTC".into();
        b.id = "other".into();
        b.updated += 100;
        b.photo = Some("data:x".into());
        assert_eq!(content_hash(&a), content_hash(&b));
        b.phones = vec![Labeled::new("mobile", "1")];
        assert_ne!(content_hash(&a), content_hash(&b));
        assert_eq!(photo_hash(None), "");
        assert_ne!(photo_hash(Some("x")), "");
    }

    #[test]
    fn data_uri_round_trip() {
        let png = [0x89, b'P', b'N', b'G', 1, 2];
        let uri = to_data_uri(&png);
        assert!(uri.starts_with("data:image/png;base64,"));
        assert_eq!(from_data_uri(&uri).unwrap(), png);
        assert_eq!(from_data_uri("data:image/jpeg;base64,!!!"), None);
        assert_eq!(from_data_uri("http://x"), None);
    }
}
