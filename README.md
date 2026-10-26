# Kith

A fast, modern address book for Linux (Tauri + Rust + Vue), that syncs with Google Contacts.
Work in progress.

- One plain `.vcf` file per contact in `~/.local/share/kith/contacts`: easy to back up, nothing locked in.
- 30+ fields, each one switchable on or off, with templates (Basic, Personal, Business, Everything, your own).
- Import and export vCard (2.1, 3.0, 4.0; Apple, Google and Android quirks included).
- Google sync through your own OAuth client; the first sync only reads.

License: GPL-3.0-or-later.
