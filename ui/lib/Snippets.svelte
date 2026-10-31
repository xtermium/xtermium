<script lang="ts">
  // Saved commands: run one in the focused terminal (and every ticked tab with broadcast on),
  // add, edit, delete. They live with the sessions, so sync carries them.
  import { _ } from "./i18n.svelte";
  import { placeholders, score } from "./actions";
  import type { Snippet } from "./api";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { app, saveStore } from "./state.svelte";

  let {
    target,
    broadcast,
    onrun,
    onclose,
  }: {
    /** The tab a snippet would run in; "" when no terminal is focused. */
    target: string;
    broadcast: boolean;
    onrun: (s: Snippet) => void;
    onclose: () => void;
  } = $props();

  let filter = $state("");
  /** The snippet being edited: its index, or null for a new one. */
  let editing = $state<{ index: number | null; s: Snippet } | null>(null);

  const shown = $derived(
    app.store.snippet
      .map((s, i) => [s, i] as const)
      .filter(([s]) => !filter.trim() || score(filter, `${s.name} ${s.text}`) >= 0),
  );

  const problem = $derived.by(() => {
    if (!editing) return "";
    if (!editing.s.text.trim()) return _("Write the command to type.");
    const name = editing.s.name.trim();
    if (app.store.snippet.some((s, i) => s.name === name && i !== editing!.index)) return _("Another snippet has that name.");
    return "";
  });

  function save() {
    if (!editing || problem) return;
    const s = $state.snapshot(editing.s);
    if (editing.index === null) app.store.snippet.push(s);
    else app.store.snippet[editing.index] = s;
    saveStore();
    editing = null;
  }

  function remove(i: number) {
    app.store.snippet.splice(i, 1);
    saveStore();
  }
</script>

<Modal title={_("Snippets")} width={610} {onclose}>
  {#if editing}
    <div class="form">
      <span>{_("Name")}</span>
      <input bind:value={editing.s.name} placeholder={_("e.g. Disk usage")} {@attach (el) => el.focus()} />
      <span>{_("Command")}</span>
      <textarea class="code" rows="5" bind:value={editing.s.text} placeholder={"df -h\nsudo journalctl -u {{service}} -n 50"} spellcheck="false"></textarea>
      <span></span>
      <label class="check"><input type="checkbox" bind:checked={editing.s.run} />{_("Press Enter at the end (runs it)")}</label>
    </div>
    <!-- Markup in translated text: the locale files ship with the app. -->
    <p class="weak">{@html _("Each line is typed and ends with Enter. Write <code>{{name}}</code> for a value to ask for when it runs.")}</p>
    <div class="row">
      <button class="primary" disabled={!!problem} onclick={save}>{_("Save")}</button>
      <button onclick={() => (editing = null)}>{_("Cancel")}</button>
      {#if problem}<span class="weak">{problem}</span>{/if}
    </div>
  {:else}
    <div class="row">
      <!-- Enter runs the first snippet shown, as its Run button would; the key itself goes no
           further, or it would reach the terminal that takes the keyboard back. -->
      <input
        class="grow"
        bind:value={filter}
        placeholder={_("Filter")}
        {@attach (el) => el.focus()}
        onkeydown={(e) => e.key === "Enter" && target && shown.length && (e.preventDefault(), onrun(shown[0][0]))}
      />
      <button class="primary" onclick={() => (editing = { index: null, s: { name: "", text: "", run: true } })}><Icon name="plus" />{_("New snippet")}</button>
    </div>
    <p class="weak">
      {#if !target}{_("Focus a terminal tab to run a snippet in it.")}
      {:else if broadcast}{_("Runs in every ticked tab (broadcast is on).")}
      {:else}{_("Runs in {tab}.", { tab: target })}{/if}
    </p>
    <div class="list">
      {#each shown as [s, i] (i)}
        <div class="item" class:first={filter.trim() && target && i === shown[0][1]}>
          <div class="what">
            <strong>{s.name}</strong>
            <code class="weak" title={s.text}>{s.text.replaceAll("\n", " ⏎ ")}{s.run ? " ⏎" : ""}</code>
          </div>
          <button class="primary" disabled={!target} title={placeholders(s.text).length ? _("Asks for its values, then runs") : _("Type it into the terminal")} onclick={() => onrun(s)}>
            <Icon name="play" />{_("Run")}
          </button>
          <button class="flat icon-only" title={_("Edit")} onclick={() => (editing = { index: i, s: $state.snapshot(s) })}><Icon name="edit" /></button>
          <button class="flat icon-only" title={_("Delete")} onclick={() => remove(i)}><Icon name="trash" /></button>
        </div>
      {:else}
        <p class="weak none">{app.store.snippet.length ? _("Nothing matches.") : _("No snippets yet. Save the commands you type often.")}</p>
      {/each}
    </div>
  {/if}
</Modal>

<style>
  .code {
    font-family: var(--mono);
    font-size: var(--t-s);
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 420px;
    overflow-y: auto;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 5px;
    border-radius: var(--r-control);
  }
  .item:hover,
  .item.first {
    background: var(--hover);
  }
  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .what code {
    font-family: var(--mono);
    font-size: var(--t-s);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .none {
    padding: 8px 5px;
  }
</style>
