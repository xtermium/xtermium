<script lang="ts">
  import { _ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { BANNER, GITHUB, HOMEPAGE, openUrl, type Update } from "./api";
  import Icon from "./Icon.svelte";
  import { app } from "./state.svelte";
  // Settings › About: the version, build and kind of build, and the latest release on GitHub.

  type About = { version: string; build: number; kind: "debug" | "beta" | "release" };
  const KINDS = { debug: _("debug build"), beta: _("beta"), release: _("release") };
  let about = $state<About | null>(null);
  invoke<About>("about").then((a) => (about = a));

  let update = $state<{ busy: boolean; latest: Update | null; error: string }>({ busy: false, latest: null, error: "" });
  async function check() {
    Object.assign(update, { busy: true, error: "" });
    try {
      update.latest = await invoke<Update>("update_check");
    } catch (e) {
      update.error = String(e);
    }
    update.busy = false;
  }

  // The licences of the code xtermium is built from, in the system's viewer for text.
  let noticesError = $state("");
  function notices() {
    noticesError = "";
    invoke("open_notices").catch((e) => (noticesError = String(e)));
  }
</script>

<div class="about">
  <pre class="banner">{BANNER}</pre>
  <div class="version">
    {_("version {version}", { version: app.version })}
    {#if about}<span class="weak">{about.build ? _("build {build}", { build: about.build }) : ""} ({KINDS[about.kind]})</span>{/if}
  </div>
  <em>{_("co-created with Claude by Sotirios Roussis")}</em>
  <div class="row">
    <button onclick={() => openUrl(HOMEPAGE)}><Icon name="world" />xtermium.com</button>
    <button onclick={() => openUrl(GITHUB)}><Icon name="github" />GitHub</button>
  </div>

  <div class="update">
    {#if update.latest?.newer}
      {@const u = update.latest}
      <strong>{_("xtermium {version} is out.", { version: u.version })}</strong>
      <div class="row">
        <button class="primary" onclick={() => openUrl(u.download ?? u.page)}><Icon name="download" />{u.download ? _("Download for this computer") : _("Open the release")}</button>
        <button class="link quiet" onclick={() => openUrl(u.page)}>{_("What's new")}</button>
      </div>
      <p class="weak">{_("Updating is up to you: close xtermium, run the new installer, start it again. Your sessions and settings stay as they are.")}</p>
    {:else if update.latest}
      <span class="row"><Icon name="check-circle" color="var(--ok)" />{_("You have the latest release.")}</span>
    {:else if update.error}
      <span class="row error"><Icon name="x-circle" />{update.error}</span>
    {/if}
    <div class="row">
      <button disabled={update.busy} onclick={check}>{#if update.busy}<Icon name="loader-alt" spin />{/if}{_("Check for updates")}</button>
    </div>
    {#if app.settings}
      <label class="check"><input type="checkbox" bind:checked={app.settings.update_notify} />{_("Tell me when a new version is out")}</label>
    {/if}
  </div>

  <strong>{_("Built with")}</strong>
  <div class="weak">Tauri, Svelte, wezterm-term, russh, russh-sftp, IronRDP, portable-pty, serialport.<br />{_("Icons from Boxicons, type set in Recursive.")}</div>
  <div class="weak">Apache License 2.0 · <button class="link" onclick={notices}>{_("Third-party notices")}</button></div>
  {#if noticesError}<span class="row error"><Icon name="x-circle" />{noticesError}</span>{/if}
</div>

<style>
  .about {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
  }
  .banner {
    margin: 0;
    font-size: var(--t);
    line-height: 1.2;
    color: var(--gold);
    text-align: start;
  }
  .version {
    font-size: var(--t-l);
  }
  .version .weak {
    font-size: var(--t);
  }
  /* the release check, a panel of its own */
  .update {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 8px 13px;
    border: 1px solid var(--line);
    border-radius: var(--r-menu);
  }
  .update p {
    margin: 0;
    max-width: 55ch;
  }
</style>
