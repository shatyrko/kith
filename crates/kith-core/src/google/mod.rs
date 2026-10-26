//! Google Contacts sync (People API v1).
//!
//! The module is split in four layers so each can be tested alone:
//!
//! * [`mapping`]: pure functions between Kith's [`Contact`](crate::model::Contact) and the People API `Person` JSON.
//! * [`api`]: the [`api::PeopleApi`] trait and its blocking HTTP implementation ([`api::HttpPeopleApi`]).
//! * [`oauth`]: sign-in (OAuth 2.0 for installed apps, loopback redirect with PKCE), token refresh and token storage.
//! * [`sync`]: the sync engine ([`sync::sync`]) working against any `PeopleApi`, so it is tested with an in-memory fake.
//!
//! Typical use from the app:
//!
//! ```ignore
//! let login = google::oauth::start(&settings.google.client_id, &settings.google.client_secret)?;
//! open_in_browser(&login.auth_url);
//! let tokens = login.wait(Duration::from_secs(300))?;      // also stores the refresh token
//! // later, on every sync:
//! let g = settings.google.clone();
//! let api = google::api::HttpPeopleApi::new(move || google::oauth::access_token(&g).map_err(google::api::ApiError::from));
//! let mut state = google::sync::SyncState::load(store.root())?;
//! let report = google::sync::sync(&mut store, &api, &mut state, &google::sync::SyncOptions::default())?;
//! ```

pub mod api;
pub mod mapping;
pub mod oauth;
pub mod sync;

#[cfg(test)]
mod testutil;
