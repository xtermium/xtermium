<script lang="ts">
  // Import: session files other apps keep on this computer (one click), or files you pick.
  // Encrypted .xtermium exports ask for their passphrase.
  import { _ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { FORMATS, type Format, type Store } from "./api";
  import Icon from "./Icon.svelte";
  import Modal from "./Modal.svelte";
  import { app } from "./state.svelte";

  let { onclose }: { onclose: () => void } = $props();

  type Source = { format: Format; path: string };
  let sources = $state<Source[] | null>(null);
  invoke<Source[]>("import_sources").then((s) => (sources = s));

  /** An encrypted export waiting for its passphrase. */
  let locked = $state<File | null>(null);
  let pass = $state("");
  let busy = $state(false);
  let error = $state("");
  let done = $state<string[]>([]);

  const formatName = (f: Format) => _(FORMATS.find((x) => x.id === f)?.name ?? f).replace(/ \(.*/, "");

  async function run(label: string, call: () => Promise<[Store, string]>) {
    [busy, error] = [true, ""];
    try {
      const [store, msg] = await call();
      app.store = store;
      done.push(`${label}: ${msg}`);
      app.status = msg;
    } catch (e) {
      error = `${label}: ${e}`;
    }
    busy = false;
  }

  async function files(list: FileList | null) {
    for (const f of list ?? []) {
      const data = new Uint8Array(await f.arrayBuffer());
      if (new TextDecoder().decode(data.subarray(0, 4)) === "XTMX") {
        [locked, pass] = [f, ""];
        continue;
      }
      await run(f.name, () => invoke("import_file", { name: f.name, data: Array.from(data), passphrase: "" }));
    }
  }

  async function unlock() {
    const f = locked!;
    const data = Array.from(new Uint8Array(await f.arrayBuffer()));
    await run(f.name, () => invoke("import_file", { name: f.name, data, passphrase: pass }));
    if (!error) locked = null;
  }
</script>

<Modal title={_("Import sessions")} width={610} {onclose}>
  <p>{_("Sessions, folders and tunnels come across; passwords don't. Sessions you already have (same name and folder) are kept as they are.")}</p>

  <h3>{_("Found on this computer")}</h3>
  {#if sources === null}
    <p class="row weak"><Icon name="loader-alt" spin />{_("Looking…")}</p>
  {:else if !sources.length}
    <p class="weak">{_("No ~/.ssh/config or MobaXterm settings in their usual places.")}</p>
  {:else}
    <div class="sources">
      {#each sources as s (s.path)}
        <div class="row source">
          <Icon name={s.format === "openssh" ? "lock-alt" : "import"} />
          <span class="grow"><strong>{formatName(s.format)}</strong> <code class="weak">{s.path}</code></span>
          <button disabled={busy} onclick={() => run(formatName(s.format), () => invoke("import_path", { format: s.format, path: s.path }))}>{_("Import")}</button>
        </div>
      {/each}
    </div>
  {/if}

  <h3>{_("From a file")}</h3>
  <p class="weak">{_("An xtermium export (.xtermium), a MobaXterm session export (.mxtsessions) or MobaXterm.ini, or an OpenSSH config file.")}</p>
  <input type="file" multiple disabled={busy} onchange={(e) => files((e.target as HTMLInputElement).files)} />

  {#if locked}
    <div class="form">
      <span>{_("Passphrase for {name}", { name: locked.name })}</span>
      <input type="password" bind:value={pass} {@attach (el) => el.focus()} onkeydown={(e) => e.key === "Enter" && pass && unlock()} />
    </div>
    <div class="row"><button class="primary" disabled={!pass || busy} onclick={unlock}>{#if busy}<Icon name="loader-alt" spin />{/if}{_("Import")}</button></div>
  {/if}
  {#if error}<p class="error">{error}</p>{/if}
  {#each done as d, i (i)}<p class="row ok"><Icon name="check-circle" />{d}</p>{/each}
</Modal>

<style>
  p {
    margin: 0;
  }
  .sources {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--line);
  }
  .source {
    padding: 5px 5px 5px 8px;
    border-bottom: 1px solid var(--line);
  }
  .source code {
    font-size: var(--t-s);
    word-break: break-all;
  }
</style>
