import { reactive, computed } from 'vue';
import { getApi, isMock } from './api.js';

export const state = reactive({
  ready: false,
  mock: isMock,
  list: [],
  total: 0,
  labels: [],
  query: { text: '', group: 'all' },
  selectedId: null,
  contact: null, // the contact shown on the right
  mode: 'view', // 'view' | 'edit' | 'new'
  draft: null,
  catalog: { fields: [], templates: [] },
  settings: null,
  photos: {}, // id -> data URI | null (loaded lazily for the list)
  toast: null,
  dialog: null, // 'settings' | 'fields' | 'duplicates' | 'trash' | 'google' | null
  google: { configured: false, signed_in: false, account: '', two_way: false, last_sync: 0 },
  syncing: false,
});

let api;
let toastTimer;
let seq = 0;

export function notify(msg, kind = 'info') {
  state.toast = { msg, kind };
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (state.toast = null), 3800);
}

const fail = (e) => notify(String(e?.message || e), 'error');

export function blankContact() {
  return {
    id: '', name: { prefix: '', given: '', middle: '', family: '', suffix: '', maiden: '', phonetic_given: '', phonetic_middle: '', phonetic_family: '' },
    nicknames: [], orgs: [], phones: [], emails: [], urls: [], ims: [], social: [], addresses: [], dates: [], relations: [], keys: [],
    notes: '', photo: null, gender: '', pronouns: '', languages: [], timezone: '', geo: '', interests: [], categories: [], favorite: false,
    kind: '', custom: [], extra: [], created: 0, updated: 0,
  };
}

export async function init() {
  api = await getApi();
  const [catalog, settings] = await Promise.all([api.catalog(), api.settings()]);
  state.catalog = catalog;
  state.settings = settings;
  applyTheme();
  await refresh();
  api.googleStatus().then((g) => (state.google = g)).catch(() => {});
  if (!isMock) {
    // a background sync finished: reload the list
    import('@tauri-apps/api/event').then(({ listen }) => listen('contacts-changed', () => { refresh(); refreshGoogle(); }));
  }
  state.ready = true;
  if (state.list.length) select(state.list[0].id);
}

export function applyTheme() {
  const t = state.settings?.theme || 'system';
  if (t === 'system') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', t);
}

export async function refresh() {
  const my = ++seq;
  const q = { ...state.query, sort_family: state.settings?.sort_by === 'family', family_first: state.settings?.display_order === 'family' };
  try {
    const [list, labels, stats] = await Promise.all([api.list(q), api.labels(), api.stats()]);
    if (my !== seq) return;
    state.list = list;
    state.labels = labels;
    state.total = stats.total;
  } catch (e) {
    fail(e);
  }
}

let searchTimer;
export function setSearch(text) {
  state.query.text = text;
  clearTimeout(searchTimer);
  searchTimer = setTimeout(refresh, 90);
}

export async function setGroup(group) {
  state.query.group = group;
  state.query.text = '';
  await refresh();
}

export async function select(id) {
  if (state.mode !== 'view') state.mode = 'view';
  state.selectedId = id;
  try {
    state.contact = id ? await api.get(id) : null;
  } catch (e) {
    fail(e);
  }
}

export async function photoFor(id) {
  if (id in state.photos) return state.photos[id];
  state.photos[id] = null;
  try {
    state.photos[id] = await api.photo(id);
  } catch {
    /* no photo */
  }
  return state.photos[id];
}

export function startNew() {
  state.draft = blankContact();
  state.selectedId = null;
  state.contact = null;
  state.mode = 'new';
}

export function startEdit() {
  if (!state.contact) return;
  state.draft = JSON.parse(JSON.stringify(state.contact));
  state.mode = 'edit';
}

export function cancelEdit() {
  state.mode = 'view';
  state.draft = null;
  if (!state.contact && state.list.length) select(state.list[0].id);
}

export async function saveDraft() {
  try {
    const saved = await api.save(state.draft);
    delete state.photos[saved.id];
    state.mode = 'view';
    state.draft = null;
    await refresh();
    await select(saved.id);
    notify('Saved');
  } catch (e) {
    fail(e);
  }
}

export async function removeContact(id) {
  try {
    await api.remove(id);
    state.contact = null;
    state.selectedId = null;
    await refresh();
    if (state.list.length) await select(state.list[0].id);
    notify('Moved to Trash');
  } catch (e) {
    fail(e);
  }
}

export async function toggleFavorite(id, on) {
  try {
    await api.favorite(id, on);
    if (state.contact?.id === id) state.contact.favorite = on;
    await refresh();
  } catch (e) {
    fail(e);
  }
}

export async function saveSettings(s) {
  try {
    state.settings = await api.saveSettings(s);
    state.catalog = await api.catalog();
    applyTheme();
    await refresh();
  } catch (e) {
    fail(e);
  }
}

export const activeTemplate = computed(() => {
  const all = state.catalog.templates;
  const t = all.find((x) => x.name === state.settings?.active_template) || all[0];
  return t || { name: '', fields: [] };
});

export async function exportAll() {
  try {
    const path = await api.pickSave('kith-contacts.vcf');
    if (!path) return;
    await api.exportVcf(path, null);
    notify('Exported all contacts');
  } catch (e) {
    fail(e);
  }
}

export async function exportOne(id, name) {
  try {
    const path = await api.pickSave((name || 'contact').replace(/[^\p{L}\p{N}_-]+/gu, '_') + '.vcf');
    if (!path) return;
    await api.exportVcf(path, [id]);
    notify('Exported');
  } catch (e) {
    fail(e);
  }
}

export async function importFile() {
  try {
    const path = await api.pickOpen();
    if (!path) return;
    const r = await api.importVcf(path);
    await refresh();
    notify(`Imported: ${r.added} new, ${r.updated} updated, ${r.skipped} unchanged`);
  } catch (e) {
    fail(e);
  }
}

export async function refreshGoogle() {
  try {
    state.google = await api.googleStatus();
  } catch (e) {
    fail(e);
  }
}

export async function googleSync(dryRun = false) {
  state.syncing = true;
  try {
    const r = await api.googleSync(dryRun);
    await refresh();
    await refreshGoogle();
    return r;
  } catch (e) {
    fail(e);
  } finally {
    state.syncing = false;
  }
}

export function theApi() {
  return api;
}
