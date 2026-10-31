<script lang="ts">
  // Command palette: actions, saved sessions, open tabs and tunnels, fuzzy-searched. An address
  // ("me@host:2222", "rdp 192.0.2.19") offers to connect to it directly.
  import { _ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ACTIONS, keysOf, quickConnect, score } from "./actions";
  import { addressSession, host, KIND_COLOR, KIND_ICON, type SavedLayout, type Session, type Snippet, type TunnelStats } from "./api";
  import Icon from "./Icon.svelte";
  import SessionIcon from "./SessionIcon.svelte";
  import { app, lookOf } from "./state.svelte";

  let {
    tabs,
    onaction,
    onsnippet,
    onlayout,
    onopen,
    onshow,
    onclose,
  }: {
    tabs: { key: number; title: string; session: Session }[];
    onaction: (id: string) => void;
    onsnippet: (s: Snippet) => void;
    onlayout: (l: SavedLayout) => void;
    onopen: (s: Session) => void;
    onshow: (key: number) => void;
    onclose: () => void;
  } = $props();

  type Item = { label: string; hint: string; icon: string; color: string; keys?: string; run: () => void };

  let query = $state("");
  let sel = $state(0);
  let list: HTMLDivElement;
  let live = $state<Record<string, TunnelStats>>({});
  invoke<Record<string, TunnelStats>>("tunnel_stats").then((s) => (live = s));

  function tunnel(name: string, on: boolean) {
    invoke(on ? "tunnel_stop" : "tunnel_start", { name }).catch((e) => (app.status = String(e)));
  }

  const items = $derived.by((): Item[] => {
    const shortcuts = app.settings?.shortcuts ?? {};
    return [
      ...tabs.map((t) => ({ label: t.title, hint: _("Open tab"), ...lookOf(t.session), run: () => onshow(t.key) })),
      // the sessions before the actions: the keyboard's front door is reached for a machine first
      ...app.store.session.map((s) => ({
        label: s.folder ? `${s.folder}/${s.name}` : s.name,
        hint: host(s.kind),
        ...lookOf(s),
        run: () => onopen(s),
      })),
      // every action but opening the palette, which is open
      ...ACTIONS.filter((a) => a.id !== "palette").map((a) => ({ label: _(a.label), hint: "", icon: "play", color: "var(--weak)", keys: keysOf(a.id, shortcuts), run: () => onaction(a.id) })),
      ...app.store.layout.map((l) => ({ label: _("Open layout: {name}", { name: l.name }), hint: l.pane.map((s) => s.name).join(", "), icon: "grid-alt", color: "var(--gold)", run: () => onlayout(l) })),
      ...app.store.snippet.map((s) => ({ label: _("Run snippet: {name}", { name: s.name }), hint: s.text.split("\n")[0], icon: "play", color: "var(--gold)", run: () => onsnippet(s) })),
      ...app.store.tunnel.map((t) => {
        const on = !!live[t.name]?.live;
        return { label: on ? _("Stop tunnel {name}", { name: t.name }) : _("Start tunnel {name}", { name: t.name }), hint: `${t.kind} :${t.listen_port}`, icon: "transfer-alt", color: "var(--gold)", run: () => tunnel(t.name, on) };
      }),
    ];
  });

  const shown = $derived.by((): Item[] => {
    const q = query.trim();
    const found = q
      ? items
          .map((it) => [it, score(q, `${it.label} ${it.hint}`)] as const)
          .filter(([, s]) => s >= 0)
          .sort((a, b) => b[1] - a[1])
          .map(([it]) => it)
      : items;
    const addr = quickConnect(q);
    if (!addr) return found.slice(0, 60);
    const label = `${addr.user ? addr.user + "@" : ""}${addr.host}:${addr.port}`;
    const connect = () => onopen(addressSession(addr));
    return [{ label: _("Connect to {address}", { address: label }), hint: addr.type.toUpperCase(), icon: KIND_ICON[addr.type], color: KIND_COLOR[addr.type], run: connect }, ...found.slice(0, 59)];
  });

  function pick(it: Item | undefined) {
    if (!it) return;
    onclose();
    it.run();
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(0, Math.min(shown.length - 1, sel + (e.key === "ArrowDown" ? 1 : -1)));
      requestAnimationFrame(() => list.querySelector(".on")?.scrollIntoView({ block: "nearest" }));
    } else if (e.key === "Enter") {
      pick(shown[sel]);
    } else if (e.key === "Escape") {
      onclose();
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="palette" role="dialog" aria-label={_("Command palette")} tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <label class="query">
      <Icon name="search" />
      <!-- Focused outright: Svelte's autofocus leaves focus alone while the terminal has it. -->
      <input bind:value={query} oninput={() => (sel = 0)} onkeydown={keydown} {@attach (el) => el.focus()} placeholder={_("Type a command, a session, or user@host to connect")} />
    </label>
    <div class="list" bind:this={list}>
      {#each shown as it, i (i)}
        <button class="flat item" class:on={i === sel} onclick={() => pick(it)} onpointermove={() => (sel = i)}>
          <SessionIcon icon={it.icon} color={it.color} />
          <span class="label">{it.label}</span>
          {#if it.hint}<span class="hint">{it.hint}</span>{/if}
          {#if it.keys}<kbd>{it.keys}</kbd>{/if}
        </button>
      {:else}
        <p class="weak none">{_("Nothing matches.")}</p>
      {/each}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
    background: rgba(8, 10, 12, 0.32);
    animation: fade-in 0.16s ease-out;
  }
  .palette {
    width: min(610px, calc(100vw - 2 * 21px));
    max-height: calc(100vh / var(--phi));
    display: flex;
    flex-direction: column;
    background: rgba(var(--panel-rgb), 0.98);
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
    box-shadow: var(--shadow-dialog);
    overflow: hidden;
    /* it hangs from the top, so it drops into place */
    --from: -5px;
    animation: float-in 0.16s var(--ease-out);
  }
  .query {
    position: relative;
    display: flex;
    align-items: center;
    padding: 5px;
    color: var(--weak);
  }
  .query :global(.icon) {
    position: absolute;
    inset-inline-start: 13px;
    pointer-events: none;
  }
  .query input {
    flex: 1;
    padding-inline-start: calc(8px + 16px + 5px);
  }
  .list {
    overflow: auto;
    display: flex;
    flex-direction: column;
    padding: 0 5px 5px;
  }
  .item {
    justify-content: flex-start;
    gap: 8px;
    min-height: var(--control);
    padding: 3px 8px;
    border-radius: var(--r-control);
    text-align: start;
    border: 0;
  }
  /* the picked row (the global `.on` would turn the text gold, and `.flat` hides its background) */
  .item.on {
    color: var(--fg);
    background: var(--gold-soft);
  }
  /* on the gold wash the weak grey loses contrast: the hint takes the row's colour */
  .item.on .hint {
    color: inherit;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hint {
    flex-shrink: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--t-s);
    color: var(--weak);
  }
  kbd {
    margin-inline-start: auto;
    flex: none;
  }
  .none {
    margin: 8px 13px;
  }
</style>
