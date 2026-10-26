<p align="center"><img src="docs/icon.png" width="96" alt="Kith"></p>

# Kith

A fast, modern address book for Linux that syncs with **Google Contacts**. Built with Rust, Tauri 2 and Vue 3.

Kith is for people whose contacts live on many devices (iPhone, iPad, Mac, Android, two Linux laptops…) and who want one
address book they own: plain files on disk, Google as the shared hub, no extra services.

## Features

- **Plain files.** One `.vcf` file per contact in `~/.local/share/kith/contacts`. Back it up with `cp`, read it with anything.
- **32 fields, all switchable.** Names (title, middle, suffix, maiden, phonetic), nicknames, companies and jobs, phones, emails,
  addresses, websites, messengers, social profiles, PGP keys, birthday, anniversary, other dates, related people, gender,
  pronouns, languages, time zone, location, interests, notes, labels, your own custom fields.
  **Templates** (Basic, Personal, Business, Everything, your own) choose which ones the form shows. A field that holds data
  is never hidden.
- **Google sync** with your own OAuth client. The first sync only reads; writing back to Google is a switch you turn on once the
  result looks right. A backup of everything is written before every sync. See [docs/google-setup.md](docs/google-setup.md).
- **Import and export** vCard 2.1 / 3.0 / 4.0, including Apple, Google, Android and Thunderbird habits (quoted-printable,
  `item1.X-ABLabel`, photos, years of 1604…). Unknown vCard lines are kept as they were.
- **Search** that ignores accents and finds phone numbers however they are written.
- **Duplicates and merge**, **upcoming birthdays**, **labels**, favorites, trash (30 days), photos.
- Dark and light themes (follows the system).

## Install

```bash
git clone https://github.com/shatyrko/kith && cd kith
tools/install.sh          # builds and installs ~/.local/bin/kith + a launcher entry
```

Needs Rust, Node.js and the Tauri 2 libraries (on Arch: `webkit2gtk-4.1 gtk3 libsoup3`).

## Develop

```bash
npm install
npm run dev                          # the UI in a browser, with invented demo people (no real data)
cargo test -p kith-core              # the engine: vCard, store, search, Google mapping and sync
cargo run -p kith-core --example demo -- /tmp/kith-demo
KITH_DATA_DIR=/tmp/kith-demo cargo run -p kith     # the real app on demo data
```

Layout: `crates/kith-core` (engine, no UI), `src-tauri` (the thin Tauri shell), `src` (Vue UI; `src/mock.js` is the
browser-only mock backend), `docs`, `tools`.

## Data and privacy

Everything stays on your computer. The only network traffic is to Google when you sign in and sync. The Google refresh
token is kept in the system keyring (Secret Service); the OAuth client secret you create is stored in
`~/.config/kith/settings.json`, readable only by you.

## License

GPL-3.0-or-later.
