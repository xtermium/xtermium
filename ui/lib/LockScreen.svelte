<script lang="ts">
  // The lock screen (src/lock.rs): over everything in this window while xtermium is locked, and
  // over Settings to confirm it's you before the lock changes. It keeps itself the page's last
  // `.backdrop`, so terminals, remote desktops and dialogs under it never take a key (Modal.svelte
  // `dialogTakesKeys`) and website views step aside (Web.svelte).
  import { invoke } from "@tauri-apps/api/core";
  import { _ } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import { passkeyAnswer, passkeysWork } from "./passkey";
  import { loadLock, lock } from "./state.svelte";

  const shown = $derived(lock.locked || !!lock.confirm);
  let mode = $state<"password" | "pin">("password");
  let secret = $state("");
  let busy = $state(false);
  let error = $state("");

  $effect(() => {
    if (!shown) return;
    [secret, error, busy] = ["", "", false];
    loadLock().then(() => (mode = lock.info && !lock.info.password && lock.info.pin ? "pin" : "password"));
  });

  async function unlock(proof: object | Promise<object>) {
    [busy, error] = [true, ""];
    try {
      await invoke("lock_unlock", { proof: await proof });
      secret = "";
      lock.confirm?.(true);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    busy = false;
  }
  const typed = () => secret && !busy && unlock({ with: mode, secret });

  /** Stays the last thing in the page: a dialog or menu opened under it can't take the keyboard. */
  function keepLast(el: HTMLElement) {
    const last = () => {
      if (document.body.lastElementChild === el) return;
      const had = document.activeElement;
      document.body.append(el);
      if (had instanceof HTMLElement && el.contains(had)) had.focus();
    };
    last();
    const watch = new MutationObserver(last);
    watch.observe(document.body, { childList: true });
    return () => (watch.disconnect(), el.remove());
  }
</script>

{#if shown}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="backdrop lockscreen" {@attach keepLast} onkeydown={(e) => e.key === "Escape" && lock.confirm?.(false)}>
    <div class="box" role="dialog" aria-modal="true" aria-labelledby="lock-title">
      <Icon name="lock-alt" size={34} color="var(--gold)" />
      <h2 id="lock-title">{lock.confirm ? _("Confirm it's you") : _("xtermium is locked")}</h2>
      {#if !lock.info}
        <Icon name="loader-alt" spin />
      {:else}
        {@const i = lock.info}
        {#if i.password || i.pin}
          {#if i.password && i.pin}
            <div class="row" role="group" aria-label={_("Unlock with")}>
              <button class="flat" class:on={mode === "password"} onclick={() => ((mode = "password"), (secret = ""))}>{_("Password")}</button>
              <button class="flat" class:on={mode === "pin"} onclick={() => ((mode = "pin"), (secret = ""))}>{_("PIN")}</button>
            </div>
          {/if}
          <div class="row field">
            {#key mode}
              <input
                class="grow"
                type="password"
                inputmode={mode === "pin" ? "numeric" : "text"}
                bind:value={secret}
                placeholder={mode === "pin" ? _("PIN") : _("Password")}
                aria-label={mode === "pin" ? _("PIN") : _("Password")}
                autocomplete="off"
                {@attach (el) => el.focus()}
                onkeydown={(e) => e.key === "Enter" && typed()}
              />
            {/key}
            <button class="primary" disabled={!secret || busy} onclick={typed}>{#if busy}<Icon name="loader-alt" spin />{/if}{lock.confirm ? _("Confirm") : _("Unlock")}</button>
          </div>
        {/if}
        {#if (i.passkey && passkeysWork()) || (i.biometrics && i.hello)}
          <div class="row">
            {#if i.passkey && passkeysWork()}
              <button disabled={busy} onclick={() => unlock(passkeyAnswer(i.passkey!))}><Icon name="key" />{_("Use your passkey")}</button>
            {/if}
            {#if i.biometrics && i.hello}
              <button disabled={busy} onclick={() => unlock({ with: "biometrics" })}><Icon name="identity" />{_("Windows Hello")}</button>
            {/if}
          </div>
        {/if}
      {/if}
      {#if error}<p class="error">{error}</p>{/if}
      {#if lock.confirm}
        <button class="flat" onclick={() => lock.confirm?.(false)}>{_("Cancel")}</button>
      {:else}
        <p class="weak">{_("Your sessions keep running.")}</p>
      {/if}
    </div>
  </div>
{/if}

<style>
  /* Over everything, dialogs included, and opaque: nothing behind it shows. */
  .lockscreen {
    position: fixed;
    inset: 0;
    z-index: 10000;
    display: grid;
    place-items: center;
    background: rgb(var(--window-rgb));
    animation: fade-in 0.16s ease-out;
  }
  .box {
    width: min(377px, calc(100vw - 2 * 21px));
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 13px;
    text-align: center;
  }
  h2 {
    margin: 0;
    font-size: var(--t-xl);
    font-weight: 600;
  }
  .field {
    align-self: stretch;
  }
  .on {
    color: var(--gold);
  }
  p {
    margin: 0;
  }
</style>
