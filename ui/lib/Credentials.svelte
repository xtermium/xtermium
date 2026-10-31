<script lang="ts">
  // Logins kept once and used by many sessions: change a password here and every session using
  // it has the new one. Stored encrypted with the sessions, and synced with them.
  import { _ } from "./i18n.svelte";
  import { score } from "./actions";
  import type { Credential } from "./api";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import PathInput from "./PathInput.svelte";
  import SshPick from "./SshPick.svelte";
  import { app, saveStore } from "./state.svelte";
  import { openSettings } from "./Settings.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let editing = $state<{ index: number | null; c: Credential } | null>(null);
  let showPassword = $state(false);
  /** The list is searched and read 10 to a page. */
  const PER_PAGE = 10;
  let query = $state("");
  let page = $state(0);
  const found = $derived.by((): [Credential, number][] => {
    const all = app.store.credential.map((c, i): [Credential, number] => [c, i]);
    const q = query.trim();
    if (!q) return all;
    return all
      .map(([c, i]) => [c, i, score(q, `${c.name} ${c.user} ${c.gateway?.host ?? ""}`)] as const)
      .filter(([, , s]) => s >= 0)
      .sort((a, b) => b[2] - a[2])
      .map(([c, i]): [Credential, number] => [c, i]);
  });
  const pages = $derived(Math.max(1, Math.ceil(found.length / PER_PAGE)));
  $effect(() => {
    if (page >= pages) page = pages - 1; // the last login of the last page was deleted
  });

  /** Sessions and tunnels using a login. */
  function users(id: string): number {
    let n = app.store.tunnel.filter((t) => t.ssh.credential === id).length;
    for (const s of app.store.session) {
      const k = s.kind;
      if (k.type === "ssh") n += +(k.credential === id || k.jump?.credential === id);
      else if (k.type === "rdp" || k.type === "vnc" || k.type === "browser" || k.type === "kvm" || k.type === "winrm") n += +(k.credential === id);
    }
    return n;
  }

  const problem = $derived.by(() => {
    if (!editing) return "";
    const name = editing.c.name.trim();
    if (!name) return _("Give the login a name.");
    if (app.store.credential.some((c, i) => c.name === name && i !== editing!.index)) return _("Another login has that name.");
    if (editing.c.gateway && !editing.c.gateway.host.trim()) return _("Enter the SSH gateway's host.");
    return "";
  });

  function save() {
    if (!editing || problem) return;
    const c = $state.snapshot(editing.c);
    if (editing.index === null) app.store.credential.push(c);
    else app.store.credential[editing.index] = c;
    saveStore();
    editing = null;
  }

  function remove(i: number) {
    app.store.credential.splice(i, 1);
    // The backend drops the sessions' references to it; they use their own login again.
    saveStore();
  }

  const fresh = (): Credential => ({ id: crypto.randomUUID().replaceAll("-", ""), name: "", user: "", password: "", key: "" });
</script>

<Modal title={_("Shared logins")} width={610} {onclose}>
  {#if editing}
    <div class="form">
      <span>{_("Name")}</span>
      <input bind:value={editing.c.name} placeholder={_("e.g. Production admin")} {@attach (el) => el.focus()} />
      <span>{_("User")}</span>
      <input bind:value={editing.c.user} placeholder={_("root, or DOMAIN\\user for RDP")} />
      <span>{_("Password")}</span>
      <div class="row">
        <input class="grow" type={showPassword ? "text" : "password"} bind:value={editing.c.password} autocomplete="off" />
        <button class="flat" onclick={() => (showPassword = !showPassword)}>{showPassword ? _("Hide") : _("Show")}</button>
      </div>
      <span>{_("Private key")}</span>
      <PathInput bind:value={editing.c.key} start="~/.ssh/" placeholder={_("SSH only. Leave empty to use your SSH agent, keys in ~/.ssh or a password")} />
      <span>{_("SSH gateway")}</span>
      <label class="check">
        <input
          type="checkbox"
          checked={!!editing.c.gateway}
          onchange={(e) => (editing!.c.gateway = (e.target as HTMLInputElement).checked ? { host: "", port: 22, user: "", password: "", key: "" } : null)}
        />{_("Connect through an SSH gateway")}
      </label>
      {#if editing.c.gateway}
        {@const gw = editing.c.gateway}
        <SshPick
          onpick={(k) => {
            // A session on a shared login: that login's user, key and password.
            const c = app.store.credential.find((c) => c.id === k.credential);
            Object.assign(gw, { host: k.host, port: k.port, user: c?.user ?? k.user, key: c ? c.key : k.key, password: c?.password ?? "" });
          }}
        />
        <span>{_("Gateway host / port")}</span>
        <div class="row"><input bind:value={gw.host} style:width="220px" placeholder="bastion.example.com" /><input type="number" min="1" max="65535" bind:value={gw.port} /></div>
        <span>{_("Gateway user")}</span>
        <input bind:value={gw.user} />
        <span>{_("Gateway password")}</span>
        <div class="row">
          <input class="grow" type={showPassword ? "text" : "password"} bind:value={gw.password} autocomplete="off" placeholder={_("empty: use a key, or ask")} />
          <button class="flat" onclick={() => (showPassword = !showPassword)}>{showPassword ? _("Hide") : _("Show")}</button>
        </div>
        <span>{_("Gateway private key")}</span>
        <PathInput bind:value={gw.key} start="~/.ssh/" placeholder={_("empty: your SSH agent, keys in ~/.ssh or a password")} />
        <span></span>
        <p class="weak">{_("SSH sessions without a jump host of their own use it as one; RDP, VNC and website sessions reach their host through it.")}</p>
      {/if}
    </div>
    <div class="row">
      <button class="primary" disabled={!!problem} onclick={save}>{_("Save")}</button>
      <button onclick={() => (editing = null)}>{_("Cancel")}</button>
      {#if problem}<span class="weak">{problem}</span>{/if}
    </div>
  {:else}
    <p class="weak">
      {_("Save a username and password here once, then pick it in any SSH, RDP or VNC session or tunnel. Change the password here and they all use the new one. Logins are stored encrypted and sync with your sessions.")}
      {_("A login can also include an SSH gateway: every session that uses the login, websites too, connects through it.")}
    </p>
    {#if app.store.credential.length}
      <input type="search" placeholder={_("Search logins")} aria-label={_("Search logins")} bind:value={query} oninput={() => (page = 0)} />
    {/if}
    <div class="list">
      {#each found.slice(page * PER_PAGE, (page + 1) * PER_PAGE) as [c, i] (c.id)}
        {@const n = users(c.id)}
        <div class="item">
          <Icon name="lock-alt" />
          <div class="what">
            <strong>{c.name}</strong>
            <span class="weak">{c.user || _("no user")}{c.password ? ` · ${_("password")}` : ""}{c.key ? ` · ${_("key")}` : ""}{c.gateway ? ` · ${_("via {host}", { host: c.gateway.host })}` : ""} · {_("used by {n} sessions", { n })}</span>
          </div>
          <button class="flat icon-only" title={_("Edit")} onclick={() => ((showPassword = false), (editing = { index: i, c: $state.snapshot(c) }))}><Icon name="edit" /></button>
          <button class="flat icon-only" title={n ? _("Delete (sessions using it go back to their own username and password)") : _("Delete")} onclick={() => remove(i)}><Icon name="trash" /></button>
        </div>
      {:else}
        <p class="weak">{app.store.credential.length ? _("No login matches.") : _("No shared logins yet.")}</p>
      {/each}
    </div>
    {#if pages > 1}
      <div class="row pager">
        <button class="flat" disabled={page === 0} onclick={() => page--}>{_("Previous")}</button>
        <span class="weak">{_("Page {n} of {total} · {logins} logins", { n: page + 1, total: pages, logins: found.length })}</span>
        <button class="flat" disabled={page >= pages - 1} onclick={() => page++}>{_("Next")}</button>
      </div>
    {/if}
    <div><button class="primary" onclick={() => ((showPassword = false), (editing = { index: null, c: fresh() }))}><Icon name="plus" />{_("New login")}</button></div>
    <p class="weak">
      {_("Passwords remembered for one host when you connect are in")}
      <button class="link" onclick={() => (onclose(), openSettings("logins"))}>{_("Settings › Saved logins")}</button>
    </p>
  {/if}
</Modal>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .pager {
    justify-content: space-between;
    padding-top: 5px;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 5px;
    border-radius: var(--r-control);
  }
  .item:hover {
    background: var(--hover);
  }
  .what {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
</style>
