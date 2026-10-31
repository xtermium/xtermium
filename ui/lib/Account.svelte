<script lang="ts">
  // The account button at the bottom of the sidebar: who's signed in to sync, or a plain avatar
  // when nobody is (sync needs an account; nothing else does). Signed out, it can be hidden.
  import { _ } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { menu as menuKeys } from "./menu";
  import Quota, { level } from "./Quota.svelte";
  import { openUrl } from "./api";
  import { app, CHECKOUT, loadAccount, openPortal, signOut, syncNow, tell } from "./state.svelte";

  let { onsettings }: { onsettings: () => void } = $props();

  const s = $derived(app.settings!);
  const signedIn = $derived(!!s.sync_token);
  const who = $derived(app.sync.account?.name || s.sync_email || _("your account"));
  /** Uploading needs Pro: the server said so, or the account says it has none. */
  const getPro = $derived(app.sync.needsPro || app.sync.account?.pro.active === false);
  /** The storage's state wears the avatar's ring: a warning when nearly full, an error when full. */
  const room = $derived(level(app.sync.quota));
  let menu = $state<{ x: number; bottom: number } | null>(null);

  function toggle(e: MouseEvent) {
    e.stopPropagation();
    if (menu) return void (menu = null);
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // Above the button, growing the way the text runs.
    menu = { x: document.documentElement.dir === "rtl" ? r.right : r.left, bottom: innerHeight - r.top + 4 };
    if (signedIn) loadAccount();
  }

  function hide() {
    s.account_button = false;
    tell(_("The account button is hidden: Settings › Account and sync brings it back."), "sync");
  }
</script>

<svelte:window onclick={() => (menu = null)} />

{#if signedIn || s.account_button}
  <button class="flat icon-only account" class:on={!!menu} title={signedIn ? _("Signed in as {who}", { who }) : _("Account: not signed in")} aria-label={_("Account")} onclick={toggle}>
    {#if signedIn}
      <span class="avatar" class:error={!!app.sync.error || room === "full"} class:warn={room === "nearly"}>{(who || "?")[0].toUpperCase()}</span>
    {:else}
      <Icon name="user-circle" size={16} />
    {/if}
  </button>
{/if}

{#if menu}
  <div class="menu" style:left="{menu.x}px" style:top="auto" style:bottom="{menu.bottom}px" {@attach menuKeys(() => (menu = null))}>
    {#if signedIn}
      <div class="head">
        <strong>{who}</strong>
        {#if app.sync.account?.name && s.sync_email}<span class="weak">{s.sync_email}</span>{/if}
        {#if app.sync.busy}
          <span class="weak">{_("Syncing…")}</span>
        {:else if app.sync.error}
          <span class="error">{app.sync.error}</span>
        {:else if app.sync.last}
          <span class="weak">{_("Synced at {time}", { time: app.sync.last.toLocaleTimeString() })}</span>
        {/if}
        <Quota />
      </div>
      <button disabled={app.sync.busy} onclick={() => syncNow()}><Icon name="refresh" />{_("Sync now")}</button>
      {#if getPro}
        <button onclick={() => openUrl(CHECKOUT).catch((e) => (app.status = String(e)))}><Icon name="world" />{_("Get Pro")}</button>
      {:else if app.sync.account?.pro.active}
        <button onclick={() => openPortal().catch((e) => (app.status = String(e)))}><Icon name="world" />{_("Manage the subscription")}</button>
      {/if}
      <button onclick={onsettings}><Icon name="cog" />{_("Account and sync settings")}</button>
      <button onclick={() => signOut()}><Icon name="log-out" />{_("Sign out")}</button>
      <button onclick={() => signOut(true)}>{_("Sign out everywhere")}</button>
    {:else}
      <div class="head">
        <strong>{_("Not signed in")}</strong>
        <span class="weak">{_("Sign in to sync your sessions across devices. Everything else works without an account.")}</span>
      </div>
      <button onclick={() => (app.signIn = true)}><Icon name="log-in" />{_("Sign in")}</button>
      <button onclick={hide}><Icon name="hide" />{_("Hide this button")}</button>
    {/if}
  </div>
{/if}

<style>
  .avatar {
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    font-size: var(--t-s);
    font-weight: 700;
    background: var(--gold);
    color: var(--gold-ink);
  }
  .avatar.error {
    box-shadow: 0 0 0 2px var(--error);
  }
  .avatar.warn {
    box-shadow: 0 0 0 2px var(--warn);
  }
  .menu {
    max-width: 280px;
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 5px 8px 5px;
    margin-bottom: 5px;
    border-bottom: 1px solid var(--line);
    font-size: var(--t-s);
    overflow-wrap: anywhere;
  }
</style>
