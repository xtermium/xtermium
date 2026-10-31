<script lang="ts">
  // A tab at a glance, under its pane: where it's connected, for how long, and for SSH the cipher
  // and the round trip (pinged every few seconds by the backend). Terminals also show their size.
  import { _ } from "./i18n.svelte";
  import { onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { host, isDesktop, type Session } from "./api";
  import Backdrop from "./Backdrop.svelte";
  import SessionIcon from "./SessionIcon.svelte";
  import { folderTags, lookOf } from "./state.svelte";

  type Link = { cipher: string; kex: string; host_key: string; compression: boolean; rtt_ms: number | null };
  /** `waiting`: a desktop not showing its picture yet. */
  type Info = { since: number; alive: boolean; cols: number; rows: number; link: Link | null; waiting?: boolean };

  let {
    tab,
    broadcast,
    visible = true,
  }: {
    tab: { id: number; session: Session; alive: boolean; connected?: boolean } | undefined;
    /** Broadcast types into this tab. */
    broadcast: boolean;
    /** Shown in a pane: hidden tabs don't poll. */
    visible?: boolean;
  } = $props();

  let info = $state<Info | null>(null);
  let now = $state(Date.now());
  /** When a desktop tab's picture first showed, as its "connected since". */
  const opened = new Map<number, number>();

  async function poll() {
    now = Date.now();
    const t = tab;
    if (!visible) return;
    if (!t?.id) return void (info = null);
    if (isDesktop(t.session.kind)) {
      if (!t.connected) opened.delete(t.id);
      else if (!opened.has(t.id)) opened.set(t.id, Math.floor(Date.now() / 1000));
      info = { since: opened.get(t.id) ?? 0, alive: t.alive, cols: 0, rows: 0, link: null, waiting: !t.connected };
    } else info = await invoke<Info | null>("tab_info", { id: t.id }).catch(() => null);
  }
  $effect(() => {
    void tab?.id;
    void tab?.alive;
    void tab?.connected;
    void visible;
    poll();
  });
  const timer = setInterval(poll, 2000);
  onDestroy(() => clearInterval(timer));

  /** 42 s, 5 min, 1 h 05 min, 2 d 3 h. */
  function age(secs: number): string {
    const s = Math.max(0, Math.floor(secs));
    if (s < 60) return _("{s} s", { s });
    const m = Math.floor(s / 60);
    if (m < 60) return _("{m} min", { m });
    const h = Math.floor(m / 60);
    if (h < 24) return _("{h} h {m} min", { h, m: String(m % 60).padStart(2, "0") });
    return _("{d} d {h} h", { d: Math.floor(h / 24), h: h % 24 });
  }

  const look = $derived(tab ? lookOf(tab.session) : null);
  /** Where the session lives: its folder and the folder's tags, so a production box says so under its pane. */
  const tags = $derived(tab ? folderTags(tab.session.folder) : []);
  const connecting = $derived(!!info?.alive && (!!info.waiting || (!!info.link && !info.link.cipher)));
</script>

<footer class="bar row">
  <!-- The tab bar's background (Settings › Appearance) runs along the bottom bar too. -->
  <Backdrop place="bar" tint={(a) => `rgba(var(--panel-rgb), ${a})`} />
  {#if tab && look}
    <span class="item" title={tab.session.name}><SessionIcon icon={look.icon} color={look.color} size={14} />{host(tab.session.kind) || tab.session.name}</span>
    {#if tab.session.folder}
      <span class="item" title={_("Folder")}>{tab.session.folder}{#each tags as t (t)}<span class="tag">{t}</span>{/each}</span>
    {/if}
    {#if info}
      <span class="item">
        <span class="dot" class:up={info.alive && !connecting} class:wait={connecting} class:down={!info.alive}></span>
        {#if !info.alive}{_("stopped")}{:else if connecting}{_("connecting…")}{:else}{_("up {time}", { time: age(now / 1000 - info.since) })}{/if}
      </span>
      {#if info.link?.cipher && info.alive}
        <span class="item" title={_("Key exchange {kex}, host key {key}", { kex: info.link.kex, key: info.link.host_key })}>{info.link.cipher}{info.link.compression ? " + zlib" : ""}</span>
        {#if info.link.rtt_ms !== null}
          <span class="item" class:slow={info.link.rtt_ms > 200} title={_("Round trip of the last ping")}>{info.link.rtt_ms ? `${info.link.rtt_ms} ms` : "<1 ms"}</span>
        {/if}
      {/if}
    {/if}
    <span class="grow"></span>
    {#if broadcast}<span class="item broadcast">{_("broadcast")}</span>{/if}
    {#if info && info.cols}<span class="item" title={_("Columns × rows")}>{info.cols} × {info.rows}</span>{/if}
  {:else}
    <span class="item weak">{_("No tab")}</span>
  {/if}
</footer>

<style>
  .bar {
    position: relative;
    isolation: isolate; /* its Backdrop goes under the text */
    flex: none;
    height: var(--row);
    gap: 13px;
    padding: 0 8px;
    font-size: var(--t-s);
    color: var(--weak);
    border-top: 1px solid var(--line);
    background: rgba(var(--panel-rgb), var(--chrome-alpha));
    white-space: nowrap;
    overflow: hidden;
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--weak);
  }
  .dot.up {
    background: var(--ok);
  }
  .dot.wait {
    background: var(--warn);
  }
  .dot.down {
    background: var(--error);
  }
  .slow {
    color: var(--warn);
  }
  .broadcast {
    color: var(--broadcast);
  }
</style>
