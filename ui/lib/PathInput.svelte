<script lang="ts">
  // A file or folder path: the disk's matching entries drop down as you type (folders end in a
  // separator, so the next level follows), and Browse opens the system's picker. `exec`: a
  // program, also found on PATH; `start`: where suggestions begin while the field is empty.
  import { _ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "./Icon.svelte";

  let {
    value = $bindable(),
    dirs = false,
    exec = false,
    start = "",
    placeholder = "",
  }: { value: string; dirs?: boolean; exec?: boolean; start?: string; placeholder?: string } = $props();

  const list = `paths-${Math.random().toString(36).slice(2)}`;
  let options = $state<string[]>([]);
  let timer = 0;

  function suggest() {
    clearTimeout(timer);
    timer = setTimeout(async () => {
      options = await invoke<string[]>("complete_path", { typed: value || start, dirs, exec }).catch(() => []);
    }, 80);
  }

  async function browse() {
    const picked = await invoke<string | null>("pick_path", { dirs, start: value || start }).catch(() => null);
    if (picked) value = picked;
  }
</script>

<div class="row path">
  <input class="grow" list={list} bind:value {placeholder} oninput={suggest} onfocus={suggest} autocomplete="off" spellcheck="false" />
  <datalist id={list}>{#each options as o (o)}<option value={o}></option>{/each}</datalist>
  <button class="icon-only" title={dirs ? _("Choose a folder") : _("Choose a file")} onclick={browse}><Icon name="folder-open" size={16} /></button>
</div>

<style>
  .path {
    gap: 5px;
  }
  .path input {
    min-width: 0;
  }
</style>
