<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type ClipboardEntry = {
  id: string;
  content: string;
  preview: string;
  createdAt: number;
  characterCount: number;
  lineCount: number;
};

type ClipboardEventPayload = {
  items: ClipboardEntry[];
};

const history = ref<ClipboardEntry[]>([]);
const search = ref("");
const selectedId = ref<string>("");
const copiedId = ref<string>("");
const busyId = ref<string>("");
const isLoading = ref(true);
const errorMessage = ref("");
const tauriAvailable = isTauri();

const filteredHistory = computed(() => {
  const keyword = search.value.trim().toLowerCase();
  if (!keyword) {
    return history.value;
  }

  return history.value.filter((entry) =>
    `${entry.preview}\n${entry.content}`.toLowerCase().includes(keyword),
  );
});

const selectedEntry = computed(() => {
  const source = filteredHistory.value.length ? filteredHistory.value : history.value;
  return (
    source.find((entry) => entry.id === selectedId.value) ??
    source[0] ??
    null
  );
});

const stats = computed(() => ({
  total: history.value.length,
  visible: filteredHistory.value.length,
  textBlocks: history.value.filter((item) => item.lineCount > 1).length,
}));

function ensureSelectedEntry() {
  const candidate = filteredHistory.value[0]?.id ?? history.value[0]?.id ?? "";
  const existsInView = filteredHistory.value.some((entry) => entry.id === selectedId.value);
  const existsInHistory = history.value.some((entry) => entry.id === selectedId.value);

  if (!selectedId.value || (!existsInView && !existsInHistory)) {
    selectedId.value = candidate;
  }
}

function formatTime(timestamp: number) {
  return new Intl.DateTimeFormat("vi-VN", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    day: "2-digit",
    month: "2-digit",
  }).format(new Date(timestamp));
}

async function loadHistory() {
  isLoading.value = true;
  errorMessage.value = "";

  if (!tauriAvailable) {
    errorMessage.value =
      "App dang duoc mo bang trinh duyet thuong. Hay chay bang `yarn tauri dev` de dung duoc Tauri API.";
    isLoading.value = false;
    return;
  }

  try {
    history.value = await invoke<ClipboardEntry[]>("get_history");
    ensureSelectedEntry();
  } catch (error) {
    errorMessage.value = String(error);
  } finally {
    isLoading.value = false;
  }
}

async function copyEntry(id: string) {
  if (!tauriAvailable) {
    return;
  }

  busyId.value = id;
  errorMessage.value = "";

  try {
    await invoke("copy_entry", { id });
    copiedId.value = id;
    selectedId.value = id;
    setTimeout(() => {
      if (copiedId.value === id) {
        copiedId.value = "";
      }
    }, 1400);
    await loadHistory();
  } catch (error) {
    errorMessage.value = String(error);
  } finally {
    busyId.value = "";
  }
}

async function removeEntry(id: string) {
  if (!tauriAvailable) {
    return;
  }

  busyId.value = id;
  errorMessage.value = "";

  try {
    await invoke("delete_entry", { id });
    history.value = history.value.filter((entry) => entry.id !== id);
    ensureSelectedEntry();
  } catch (error) {
    errorMessage.value = String(error);
  } finally {
    busyId.value = "";
  }
}

async function clearAll() {
  if (!tauriAvailable) {
    return;
  }

  const confirmed = window.confirm("Xoa toan bo lich su clipboard?");
  if (!confirmed) {
    return;
  }

  errorMessage.value = "";

  try {
    await invoke("clear_history");
    history.value = [];
    selectedId.value = "";
  } catch (error) {
    errorMessage.value = String(error);
  }
}

function mergeIncomingEntry(entry: ClipboardEntry) {
  history.value = [entry, ...history.value.filter((item) => item.id !== entry.id)];
  ensureSelectedEntry();
}

let unlistenClipboard: UnlistenFn | undefined;

onMounted(async () => {
  await loadHistory();

  if (!tauriAvailable) {
    return;
  }

  unlistenClipboard = await listen<ClipboardEventPayload>("clipboard://updated", (event) => {
    const incoming = event.payload.items?.[0];
    if (!incoming) {
      return;
    }
    mergeIncomingEntry(incoming);
  });
});

onBeforeUnmount(() => {
  unlistenClipboard?.();
});
</script>

<template>
  <main class="shell">
    <section class="hero">
      <div>
        <p class="eyebrow">Clipboard Diary</p>
        <h1>Luu lich su sao chep tren desktop</h1>
        <p class="subtext">
          App se tu dong nhat noi dung clipboard, giup ban tim lai doan text vua copy,
          copy lai chi voi mot lan bam va giu du lieu ngay trong may.
        </p>
      </div>

      <div class="hero-stats">
        <article>
          <strong>{{ stats.total }}</strong>
          <span>Muc da luu</span>
        </article>
        <article>
          <strong>{{ stats.visible }}</strong>
          <span>Dang hien thi</span>
        </article>
        <article>
          <strong>{{ stats.textBlocks }}</strong>
          <span>Noi dung nhieu dong</span>
        </article>
      </div>
    </section>

    <section class="workspace">
      <aside class="sidebar">
        <div class="toolbar">
          <input
            v-model="search"
            class="search"
            type="search"
            placeholder="Tim trong lich su clipboard..."
          />
          <button class="ghost danger" type="button" @click="clearAll" :disabled="!history.length">
            Xoa tat ca
          </button>
        </div>

        <p v-if="errorMessage" class="error">{{ errorMessage }}</p>
        <p v-else-if="isLoading" class="status">Dang tai lich su...</p>
        <p v-else-if="!filteredHistory.length" class="status">
          Chua co muc nao phu hop. Hay thu copy mot doan text de bat dau.
        </p>

        <div v-else class="history-list">
          <button
            v-for="entry in filteredHistory"
            :key="entry.id"
            type="button"
            class="history-card"
            :class="{ active: selectedEntry?.id === entry.id }"
            @click="selectedId = entry.id"
          >
            <div class="history-card__top">
              <p>{{ entry.preview || "Noi dung rong" }}</p>
              <span>{{ formatTime(entry.createdAt) }}</span>
            </div>
            <div class="history-card__meta">
              <small>{{ entry.characterCount }} ky tu</small>
              <small>{{ entry.lineCount }} dong</small>
            </div>
          </button>
        </div>
      </aside>

      <section class="detail">
        <template v-if="selectedEntry">
          <div class="detail-header">
            <div>
              <p class="eyebrow">Chi tiet muc dang chon</p>
              <h2>{{ selectedEntry.preview || "Noi dung clipboard" }}</h2>
            </div>
            <div class="detail-actions">
              <button
                class="primary"
                type="button"
                @click="copyEntry(selectedEntry.id)"
                :disabled="busyId === selectedEntry.id"
              >
                {{ copiedId === selectedEntry.id ? "Da copy lai" : "Copy lai" }}
              </button>
              <button
                class="ghost"
                type="button"
                @click="removeEntry(selectedEntry.id)"
                :disabled="busyId === selectedEntry.id"
              >
                Xoa muc nay
              </button>
            </div>
          </div>

          <div class="detail-metadata">
            <span>{{ formatTime(selectedEntry.createdAt) }}</span>
            <span>{{ selectedEntry.characterCount }} ky tu</span>
            <span>{{ selectedEntry.lineCount }} dong</span>
          </div>

          <pre class="content-preview">{{ selectedEntry.content }}</pre>
        </template>

        <div v-else class="empty-state">
          <p class="eyebrow">San sang ghi nho</p>
          <h2>Clipboard history se hien o day</h2>
          <p>
            Sau khi app dang chay, moi doan text ban copy trong Windows se duoc them vao danh sach.
          </p>
        </div>
      </section>
    </section>
  </main>
</template>

<style scoped>
:global(:root) {
  color: #1f2328;
  background:
    radial-gradient(circle at top left, rgba(255, 210, 124, 0.35), transparent 35%),
    radial-gradient(circle at top right, rgba(82, 170, 255, 0.2), transparent 28%),
    linear-gradient(180deg, #f7f1e8 0%, #efe8dd 100%);
  font-family: "Segoe UI", "SF Pro Display", sans-serif;
  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

:global(*) {
  box-sizing: border-box;
}

:global(body) {
  margin: 0;
}

:global(button),
:global(input) {
  font: inherit;
}

.shell {
  min-height: 100vh;
  padding: 28px;
}

.hero {
  display: grid;
  grid-template-columns: 1.4fr 1fr;
  gap: 20px;
  align-items: end;
  margin-bottom: 22px;
}

.eyebrow {
  margin: 0 0 10px;
  color: #b4572d;
  text-transform: uppercase;
  letter-spacing: 0.18em;
  font-size: 12px;
  font-weight: 700;
}

h1,
h2,
.subtext {
  margin: 0;
}

h1 {
  max-width: 12ch;
  font-size: clamp(2.4rem, 4vw, 4.4rem);
  line-height: 0.95;
}

.subtext {
  max-width: 62ch;
  margin-top: 14px;
  color: #5f584f;
  font-size: 1.02rem;
}

.hero-stats {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 14px;
}

.hero-stats article,
.sidebar,
.detail {
  border: 1px solid rgba(83, 67, 53, 0.12);
  border-radius: 24px;
  background: rgba(255, 251, 245, 0.82);
  box-shadow: 0 18px 60px rgba(86, 65, 40, 0.08);
  backdrop-filter: blur(12px);
}

.hero-stats article {
  padding: 18px;
}

.hero-stats strong {
  display: block;
  font-size: 2rem;
}

.hero-stats span {
  color: #6d675f;
}

.workspace {
  display: grid;
  grid-template-columns: minmax(320px, 420px) 1fr;
  gap: 18px;
  min-height: 68vh;
}

.sidebar,
.detail {
  padding: 18px;
}

.toolbar {
  display: flex;
  gap: 10px;
  margin-bottom: 14px;
}

.search {
  flex: 1;
  border: 1px solid rgba(100, 85, 72, 0.16);
  border-radius: 16px;
  padding: 14px 16px;
  background: rgba(255, 255, 255, 0.8);
}

.history-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-height: calc(68vh - 80px);
  overflow: auto;
  padding-right: 4px;
}

.history-card {
  width: 100%;
  border: 1px solid transparent;
  border-radius: 18px;
  padding: 14px;
  text-align: left;
  background: #fffaf4;
  cursor: pointer;
  transition:
    transform 180ms ease,
    border-color 180ms ease,
    box-shadow 180ms ease;
}

.history-card:hover,
.history-card.active {
  transform: translateY(-1px);
  border-color: rgba(180, 87, 45, 0.35);
  box-shadow: 0 10px 24px rgba(180, 87, 45, 0.1);
}

.history-card__top {
  display: flex;
  justify-content: space-between;
  gap: 10px;
}

.history-card__top p {
  margin: 0;
  color: #1f2328;
  font-weight: 600;
  line-height: 1.35;
}

.history-card__top span,
.history-card__meta,
.detail-metadata {
  color: #6a645b;
  font-size: 0.88rem;
}

.history-card__meta,
.detail-metadata {
  display: flex;
  gap: 12px;
  margin-top: 10px;
  flex-wrap: wrap;
}

.detail {
  display: flex;
  flex-direction: column;
}

.detail-header {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  align-items: start;
}

.detail-header h2 {
  max-width: 24ch;
  font-size: clamp(1.6rem, 2vw, 2.4rem);
  line-height: 1.05;
}

.detail-actions {
  display: flex;
  gap: 10px;
}

.primary,
.ghost {
  border-radius: 999px;
  padding: 12px 18px;
  border: 1px solid rgba(100, 85, 72, 0.16);
  cursor: pointer;
  transition:
    transform 180ms ease,
    opacity 180ms ease,
    border-color 180ms ease;
}

.primary {
  background: #b4572d;
  color: #fff8f3;
  border-color: #b4572d;
}

.ghost {
  background: rgba(255, 248, 240, 0.88);
  color: #43352c;
}

.danger {
  color: #b23a2f;
}

.primary:hover,
.ghost:hover {
  transform: translateY(-1px);
}

.primary:disabled,
.ghost:disabled {
  opacity: 0.45;
  cursor: not-allowed;
  transform: none;
}

.content-preview {
  margin: 18px 0 0;
  padding: 18px;
  border-radius: 22px;
  background: #1f2328;
  color: #f7efe2;
  white-space: pre-wrap;
  word-break: break-word;
  flex: 1;
  overflow: auto;
  line-height: 1.55;
}

.status,
.error,
.empty-state p {
  color: #665f56;
}

.error {
  margin: 0 0 12px;
  color: #b23a2f;
}

.empty-state {
  margin: auto 0;
  max-width: 52ch;
}

@media (max-width: 980px) {
  .hero,
  .workspace {
    grid-template-columns: 1fr;
  }

  .hero-stats {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .detail-header {
    flex-direction: column;
  }
}

@media (max-width: 640px) {
  .shell {
    padding: 18px;
  }

  .hero-stats {
    grid-template-columns: 1fr;
  }

  .toolbar,
  .detail-actions {
    flex-direction: column;
  }
}
</style>
