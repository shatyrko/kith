//! Kith shell: thin Tauri commands over `kith-core`. The frontend only ever sends contact ids, contacts and
//! file paths chosen in a dialog; everything is checked and written by the core.

use kith_core::fields::{FieldDef, Template, FIELDS};
use kith_core::google::api::{ApiError, HttpPeopleApi};
use kith_core::google::oauth::{self, TokenStore};
use kith_core::google::sync::{self, SyncOptions, SyncReport, SyncState};
use kith_core::settings::Settings;
use kith_core::store::{summarize, ImportReport, Query, Store, Summary};
use kith_core::Contact;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

type Cmd<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub struct AppState {
    store: Mutex<Store>,
    settings: Mutex<Settings>,
    /// One sync at a time (the engine does not guard against two).
    sync_lock: Mutex<()>,
}

#[derive(Serialize)]
struct LabelCount {
    name: String,
    count: usize,
}

#[derive(Serialize)]
struct Catalog {
    fields: &'static [FieldDef],
    templates: Vec<Template>,
}

#[derive(Serialize)]
struct Stats {
    total: usize,
}

// ───── contacts ─────

#[tauri::command]
async fn list_contacts(state: State<'_, AppState>, query: Query) -> Cmd<Vec<Summary>> {
    Ok(state.store.lock().map_err(err)?.list(&query))
}

#[tauri::command]
async fn get_contact(state: State<'_, AppState>, id: String) -> Cmd<Contact> {
    state.store.lock().map_err(err)?.get(&id).cloned().ok_or_else(|| "That contact no longer exists.".to_string())
}

#[tauri::command]
async fn get_photo(state: State<'_, AppState>, id: String) -> Cmd<Option<String>> {
    Ok(state.store.lock().map_err(err)?.get(&id).and_then(|c| c.photo.clone()))
}

#[tauri::command]
async fn save_contact(state: State<'_, AppState>, contact: Contact) -> Cmd<Contact> {
    state.store.lock().map_err(err)?.save(contact).map_err(err)
}

#[tauri::command]
async fn delete_contact(state: State<'_, AppState>, id: String) -> Cmd<()> {
    state.store.lock().map_err(err)?.delete(&id).map_err(err)
}

#[tauri::command]
async fn set_favorite(state: State<'_, AppState>, id: String, on: bool) -> Cmd<()> {
    let mut s = state.store.lock().map_err(err)?;
    if let Some(mut c) = s.get(&id).cloned() {
        c.favorite = on;
        s.save(c).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
async fn labels(state: State<'_, AppState>) -> Cmd<Vec<LabelCount>> {
    Ok(state.store.lock().map_err(err)?.labels().into_iter().map(|(name, count)| LabelCount { name, count }).collect())
}

#[tauri::command]
async fn rename_label(state: State<'_, AppState>, from: String, to: String) -> Cmd<()> {
    state.store.lock().map_err(err)?.rename_label(&from, &to).map_err(err)
}

#[tauri::command]
async fn stats(state: State<'_, AppState>) -> Cmd<Stats> {
    Ok(Stats { total: state.store.lock().map_err(err)?.len() })
}

// ───── fields, templates, settings ─────

#[tauri::command]
async fn field_catalog(state: State<'_, AppState>) -> Cmd<Catalog> {
    Ok(Catalog { fields: FIELDS, templates: state.settings.lock().map_err(err)?.templates() })
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Cmd<Settings> {
    Ok(state.settings.lock().map_err(err)?.clone())
}

#[tauri::command]
async fn save_settings(state: State<'_, AppState>, settings: Settings) -> Cmd<Settings> {
    settings.save().map_err(err)?;
    *state.settings.lock().map_err(err)? = settings.clone();
    Ok(settings)
}

// ───── import, export, duplicates, trash ─────

#[tauri::command]
async fn import_vcf(state: State<'_, AppState>, path: String) -> Cmd<ImportReport> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Could not read {}: {}", path, e))?;
    let mut s = state.store.lock().map_err(err)?;
    s.backup(10).map_err(err)?; // a restore point before a big import
    s.import_vcf(&text).map_err(err)
}

#[tauri::command]
async fn export_vcf(state: State<'_, AppState>, path: String, ids: Option<Vec<String>>) -> Cmd<usize> {
    let s = state.store.lock().map_err(err)?;
    let text = s.export_vcf(ids.as_deref());
    std::fs::write(&path, &text).map_err(|e| format!("Could not write {}: {}", path, e))?;
    Ok(text.matches("BEGIN:VCARD").count())
}

#[tauri::command]
async fn duplicates(state: State<'_, AppState>) -> Cmd<Vec<Vec<Summary>>> {
    let s = state.store.lock().map_err(err)?;
    Ok(s.duplicates().into_iter().map(|g| g.iter().filter_map(|id| s.get(id)).map(|c| summarize(c, false, "")).collect()).collect())
}

#[tauri::command]
async fn merge_contacts(state: State<'_, AppState>, ids: Vec<String>) -> Cmd<Contact> {
    let mut s = state.store.lock().map_err(err)?;
    s.backup(10).map_err(err)?;
    s.merge(&ids).map_err(err)
}

#[tauri::command]
async fn trash(state: State<'_, AppState>) -> Cmd<Vec<Summary>> {
    Ok(state.store.lock().map_err(err)?.trash().iter().map(|c| summarize(c, false, "")).collect())
}

#[tauri::command]
async fn restore_contact(state: State<'_, AppState>, id: String) -> Cmd<()> {
    state.store.lock().map_err(err)?.restore(&id).map_err(err)
}

#[tauri::command]
async fn empty_trash(state: State<'_, AppState>) -> Cmd<()> {
    state.store.lock().map_err(err)?.empty_trash();
    Ok(())
}

// ───── Google ─────

#[derive(Serialize, Default)]
struct GoogleStatus {
    configured: bool,
    signed_in: bool,
    account: String,
    two_way: bool,
    allow_deletes: bool,
    last_sync: u64,
}

fn status_of(settings: &Settings) -> GoogleStatus {
    GoogleStatus {
        configured: !settings.google.client_id.is_empty() && !settings.google.client_secret.is_empty(),
        signed_in: matches!(TokenStore::new().load(), Ok(Some(_))),
        account: settings.google.account.clone(),
        two_way: settings.google.two_way,
        allow_deletes: settings.google.allow_deletes,
        last_sync: settings.google.last_sync,
    }
}

#[tauri::command]
async fn google_status(state: State<'_, AppState>) -> Cmd<GoogleStatus> {
    Ok(status_of(&*state.settings.lock().map_err(err)?))
}

/// Opens the browser for the Google consent page and waits (up to 5 minutes) for the answer.
#[tauri::command]
async fn google_login(app: AppHandle, state: State<'_, AppState>) -> Cmd<GoogleStatus> {
    let (id, secret) = {
        let s = state.settings.lock().map_err(err)?;
        (s.google.client_id.trim().to_string(), s.google.client_secret.trim().to_string())
    };
    if id.is_empty() || secret.is_empty() {
        return Err("Enter the client ID and the client secret first (see the setup guide).".into());
    }
    let handle = oauth::start(&id, &secret).map_err(err)?;
    app.opener().open_url(handle.auth_url.clone(), None::<&str>).map_err(err)?;
    let tokens = tauri::async_runtime::spawn_blocking(move || handle.wait(Duration::from_secs(300))).await.map_err(err)?.map_err(err)?;
    let mut s = state.settings.lock().map_err(err)?;
    if !s.google.account.is_empty() && s.google.account != tokens.email {
        // another account: the links to the old account's contacts must not be reused
        let root = state.store.lock().map_err(err)?.root().to_path_buf();
        let _ = std::fs::remove_file(SyncState::path(&root));
    }
    s.google.account = tokens.email.clone();
    s.save().map_err(err)?;
    Ok(status_of(&s))
}

fn run_sync(app: &AppHandle, dry_run: bool) -> Result<SyncReport, String> {
    let state = app.state::<AppState>();
    let _one_at_a_time = state.sync_lock.lock().map_err(err)?;
    let g = state.settings.lock().map_err(err)?.google.clone();
    if !matches!(TokenStore::new().load(), Ok(Some(_))) {
        return Err("Not connected to Google. Connect your account in Settings first.".into());
    }
    let api = {
        let g = g.clone();
        HttpPeopleApi::new(move || oauth::access_token(&g).map_err(ApiError::from))
    };
    let mut store = state.store.lock().map_err(err)?;
    let mut sync_state = SyncState::load(store.root()).map_err(err)?;
    let opts = SyncOptions { two_way: g.two_way, allow_remote_deletes: g.two_way && g.allow_deletes, dry_run };
    let report = sync::sync(&mut store, &api, &mut sync_state, &opts).map_err(err)?;
    drop(store);
    if !dry_run {
        if let Ok(mut s) = state.settings.lock() {
            s.google.last_sync = kith_core::now();
            let _ = s.save();
        }
    }
    Ok(report)
}

#[tauri::command]
async fn google_sync(app: AppHandle, dry_run: bool) -> Cmd<SyncReport> {
    let handle = app.clone();
    let report = tauri::async_runtime::spawn_blocking(move || run_sync(&handle, dry_run)).await.map_err(err)??;
    if !dry_run {
        let _ = app.emit("contacts-changed", ());
    }
    Ok(report)
}

#[tauri::command]
async fn google_logout(state: State<'_, AppState>) -> Cmd<()> {
    let g = state.settings.lock().map_err(err)?.google.clone();
    // revokes the token at Google and deletes it here; the contacts stay where they are
    let _ = tauri::async_runtime::spawn_blocking(move || oauth::sign_out(&g)).await;
    let mut s = state.settings.lock().map_err(err)?;
    s.google.account.clear();
    s.google.last_sync = 0;
    s.save().map_err(err)?;
    let root = state.store.lock().map_err(err)?.root().to_path_buf();
    let _ = std::fs::remove_file(SyncState::path(&root));
    Ok(())
}

/// Background sync every `auto_sync_minutes` (0 = off), while the app runs.
fn start_auto_sync(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last = std::time::Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let minutes = match app.state::<AppState>().settings.lock() {
                Ok(s) => s.google.auto_sync_minutes,
                Err(_) => 0,
            };
            if minutes == 0 || last.elapsed() < Duration::from_secs(minutes as u64 * 60) {
                continue;
            }
            last = std::time::Instant::now();
            if run_sync(&app, false).is_ok() {
                let _ = app.emit("contacts-changed", ());
            }
        }
    });
}

pub fn run() {
    let root = std::env::var("KITH_DATA_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| kith_core::store::default_root());
    let store = Store::open(&root).unwrap_or_else(|e| panic!("Cannot open the address book at {}: {}", root.display(), e));
    let state = AppState { store: Mutex::new(store), settings: Mutex::new(Settings::load()), sync_lock: Mutex::new(()) };
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            list_contacts,
            get_contact,
            get_photo,
            save_contact,
            delete_contact,
            set_favorite,
            labels,
            rename_label,
            stats,
            field_catalog,
            get_settings,
            save_settings,
            import_vcf,
            export_vcf,
            duplicates,
            merge_contacts,
            trash,
            restore_contact,
            empty_trash,
            google_status,
            google_login,
            google_sync,
            google_logout,
        ])
        .setup(|app| {
            start_auto_sync(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Kith");
}
