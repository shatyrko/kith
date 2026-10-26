// The only place that talks to the backend. In the Tauri window it calls the Rust commands; in a plain browser
// (npm run dev) it uses mock.js with invented demo people, so the UI can be worked on without real data.
import { mockApi } from './mock.js';

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

async function real() {
  const { invoke } = await import('@tauri-apps/api/core');
  const call = (cmd, args) => invoke(cmd, args);
  return {
    list: (query) => call('list_contacts', { query }),
    get: (id) => call('get_contact', { id }),
    photo: (id) => call('get_photo', { id }),
    save: (contact) => call('save_contact', { contact }),
    remove: (id) => call('delete_contact', { id }),
    favorite: (id, on) => call('set_favorite', { id, on }),
    labels: () => call('labels'),
    renameLabel: (from, to) => call('rename_label', { from, to }),
    catalog: () => call('field_catalog'),
    settings: () => call('get_settings'),
    saveSettings: (settings) => call('save_settings', { settings }),
    importVcf: (path) => call('import_vcf', { path }),
    exportVcf: (path, ids) => call('export_vcf', { path, ids }),
    duplicates: () => call('duplicates'),
    merge: (ids) => call('merge_contacts', { ids }),
    trash: () => call('trash'),
    restore: (id) => call('restore_contact', { id }),
    emptyTrash: () => call('empty_trash'),
    stats: () => call('stats'),
    googleStatus: () => call('google_status'),
    googleLogin: () => call('google_login'),
    googleSync: (dryRun) => call('google_sync', { dryRun }),
    googleLogout: () => call('google_logout'),
    openUrl: async (url) => (await import('@tauri-apps/plugin-opener')).openUrl(url),
    pickOpen: async () => (await import('@tauri-apps/plugin-dialog')).open({ multiple: false, filters: [{ name: 'vCard', extensions: ['vcf'] }] }),
    pickSave: async (name) => (await import('@tauri-apps/plugin-dialog')).save({ defaultPath: name, filters: [{ name: 'vCard', extensions: ['vcf'] }] }),
  };
}

let impl = null;
export async function getApi() {
  if (!impl) impl = inTauri ? await real() : mockApi;
  return impl;
}
export const isMock = !inTauri;
