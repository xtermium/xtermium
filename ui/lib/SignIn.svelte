<script lang="ts">
  // Signing in to sync (docs/sync.md). There is no password: the server gives a code, the
  // browser opens on its page, the person signs in there with Google, GitHub or Discord and
  // approves the code, and the app polls until the token arrives. Signing in turns
  // sync on; nothing else in xtermium needs an account.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { openUrl } from "./api";
  import { _ } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { app, copyText, loadAccount, syncNow, type Account } from "./state.svelte";

  type Code = { device_code: string; user_code: string; verification_uri_complete: string; expires_in: number; interval: number };
  type Poll = { state: "pending" | "slow_down" } | { state: "token"; token: string };

  // Raw: the loop below compares `code` to the object it was given, and `$state` would hand back
  // a proxy that never equals it, so the poll would never start.
  let code = $state.raw<Code | null>(null);
  let error = $state("");
  /** False once the dialog is gone: the poll loop stops. */
  let open = true;

  async function start() {
    [code, error] = [null, ""];
    try {
      const mine = await invoke<Code>("account_device_start");
      code = mine;
      openUrl(mine.verification_uri_complete).catch(() => {});
      let wait = mine.interval;
      while (open && code === mine) {
        await new Promise((r) => setTimeout(r, wait * 1000));
        if (!open || code !== mine) return;
        const p = await invoke<Poll>("account_device_poll", { deviceCode: mine.device_code });
        if (p.state === "slow_down") wait += 5;
        if (p.state === "token") return signedIn(p.token);
      }
    } catch (e) {
      [code, error] = [null, String(e)];
    }
  }

  async function signedIn(token: string) {
    const s = app.settings!;
    const account = await invoke<Account>("account_info", { token });
    Object.assign(s, { sync: true, sync_token: token, sync_email: account.email, account_button: true });
    app.sync.account = account;
    // The backend syncs with the settings it has: hand them over before the first sync.
    await invoke("save_settings", { settings: $state.snapshot(s) });
    app.signIn = false;
    loadAccount();
    syncNow();
  }

  onMount(() => {
    start();
    return () => (open = false);
  });
</script>

<Modal title={_("Sign in")} width={377} onclose={() => (app.signIn = false)}>
  <p class="weak">
    {_("An account syncs your sessions, folders, tunnels and trusted server keys across your devices, encrypted before they leave this one. Without one, everything stays on this device.")}
  </p>
  {#if code}
    <p>{_("Your browser is opening xtermium's sign-in page. Sign in there with Google, GitHub or Discord and approve this code:")}</p>
    <p class="code">
      <strong>{code.user_code}</strong>
      <button class="flat icon-only" title={_("Copy")} aria-label={_("Copy the code")} onclick={() => copyText(code!.user_code)}><Icon name="copy" /></button>
    </p>
    <p class="row weak"><Icon name="loader-alt" spin />{_("Waiting for your approval…")}</p>
    <div class="row">
      <button onclick={() => openUrl(code!.verification_uri_complete).catch((e) => (error = String(e)))}><Icon name="world" />{_("Open the page again")}</button>
      <button class="flat" onclick={() => (app.signIn = false)}>{_("Cancel")}</button>
    </div>
  {:else if error}
    <p class="error">{error}</p>
    <div class="row">
      <button class="primary" {@attach (el) => el.focus()} onclick={start}>{_("Try again")}</button>
      <button class="flat" onclick={() => (app.signIn = false)}>{_("Cancel")}</button>
    </div>
  {:else}
    <p class="row weak"><Icon name="loader-alt" spin />{_("Asking the server for a code…")}</p>
  {/if}
  <p class="weak">{_("Your recovery passphrase (Settings › Security) stays separate and never leaves your devices.")}</p>
</Modal>

<style>
  p {
    margin: 0;
  }
  .code {
    display: flex;
    align-items: center;
    gap: var(--gap);
    justify-content: center;
    padding: 8px 0;
  }
  .code strong {
    font-family: var(--mono);
    font-size: var(--t-xxl);
    letter-spacing: 0.1em;
  }
</style>
