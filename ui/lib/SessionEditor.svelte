<script lang="ts">
  import { _, N_ } from "./i18n.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { AGENT_EFFORTS, AGENT_MODES, AGENTS, broadcasts, EFFORT_TOOLS, here, KIND_COLOR, MCP_TOOLS, KIND_ICON, KVM_VENDORS, newEndpoint, newKind, SKILL_TOOLS, type Kind, type KindName, type Session } from "./api";
  import Appearance from "./Appearance.svelte";
  import Ciphers from "./Ciphers.svelte";
  import Endpoint from "./Endpoint.svelte";
  import Hint from "./Hint.svelte";
  import Icon from "./Icon.svelte";
  import Login from "./Login.svelte";
  import Modal from "./Modal.svelte";
  import PathInput from "./PathInput.svelte";
  import QualityBar from "./QualityBar.svelte";
  import { app, folderColor, saveStore, siteIcon } from "./state.svelte";

  /** `index`: the session being edited, or null for a new one. `onconnect` opens a saved one. */
  let { session, index, onclose, onconnect }: { session: Session; index: number | null; onclose: () => void; onconnect?: (s: Session) => void } = $props();

  // An editable copy: the stored session changes only on Save.
  // svelte-ignore state_referenced_locally
  let s = $state(structuredClone($state.snapshot(session)));
  /** Closing with changes asks first: a stray Escape mustn't lose a half-typed session. */
  const start = JSON.stringify($state.snapshot(s));
  let discarding = $state(false);
  const close = () => (JSON.stringify($state.snapshot(s)) === start ? onclose() : (discarding = true));
  let ports = $state<string[]>([]);
  let fonts = $state<string[]>([]);
  invoke<string[]>("fonts").then((f) => (fonts = f));
  $effect(() => {
    if (s.kind.type === "serial") invoke<string[]>("serial_ports").then((p) => (ports = p));
  });

  /** The MCP servers and skills the agent's tool would load in its folder (src/agent_mcp.rs); null while looking. */
  let servers = $state<string[] | null>(null);
  let skills = $state<string[] | null>(null);
  $effect(() => {
    const k = s.kind;
    if (k.type !== "agent") return;
    const [tool, dir] = [k.tool, here(k.dir)];
    servers = skills = null;
    // the folder is typed a letter at a time: look once it rests
    const t = setTimeout(() => {
      if (MCP_TOOLS.includes(tool)) invoke<string[]>("agent_mcp_servers", { tool, dir }).then((l) => (servers = l), () => (servers = []));
      if (SKILL_TOOLS.includes(tool)) invoke<string[]>("agent_skills", { tool, dir }).then((l) => (skills = l), () => (skills = []));
    }, 250);
    return () => clearTimeout(t);
  });
  /** Unticked: the server or skill stays off in this session; ticked again, it's on. */
  function serverOn(off: string[], name: string, on: boolean) {
    const at = off.indexOf(name);
    if (on && at >= 0) off.splice(at, 1);
    else if (!on && at < 0) off.push(name);
  }
  /** Enter in "another by name": that name is turned off, for ones the list can't know of. */
  function offByName(e: KeyboardEvent, off: string[]) {
    if (e.key !== "Enter") return;
    e.preventDefault();
    e.stopPropagation();
    const el = e.currentTarget as HTMLInputElement;
    const name = el.value.trim();
    if (name) serverOn(off, name, false);
    el.value = "";
  }

  const KINDS: [KindName, string][] = [["ssh", "SSH"], ["shell", N_("Shell")], ["serial", N_("Serial")], ["rdp", "RDP"], ["vnc", "VNC"], ["kvm", N_("BMC console")], ["telnet", "Telnet"], ["winrm", "WinRM"], ["browser", N_("Browser")], ["agent", N_("AI agent")]];

  /** Where a session connects: its host, or a website's host name. */
  const hostOf = (k: Kind) => ("host" in k ? k.host.trim() : k.type === "browser" ? (/^https?:\/\/([^\s/:]+)/.exec(k.url.trim())?.[1] ?? "") : "");
  /** What the session is called if Name stays empty: where it connects. */
  /** The name a session gets when none is typed: its host, serial port, or agent tool (a custom command's first word). */
  const fallback = $derived.by(() => {
    const k = s.kind;
    if (k.type === "serial") return k.port.trim();
    if (k.type === "agent") return k.tool === "custom" ? (k.args.trim().split(/\s+/)[0] ?? "") : (AGENTS.find(([id]) => id === k.tool)?.[1] ?? "");
    return hostOf(k);
  });

  /** Another type keeps the host and user typed so far, where it has them. */
  function setKind(t: KindName) {
    const host = hostOf(s.kind);
    const user = "user" in s.kind ? s.kind.user : "";
    const next = newKind(t);
    if ("host" in next) next.host = host;
    if ("user" in next) next.user = user;
    if (next.type === "browser" && host) next.url = `https://${host}`;
    s.kind = next;
  }

  /** Every folder there is, parents too: the Folder field suggests them. */
  const folders = $derived.by(() => {
    const all = new Set<string>();
    for (const p of [...app.store.session.map((x) => x.folder), ...app.store.folder.map((f) => f.path)]) {
      const parts = p.split("/").filter(Boolean);
      for (let i = 1; i <= parts.length; i++) all.add(parts.slice(0, i).join("/"));
    }
    return [...all].sort();
  });

  /** Why the session can't be saved yet; "" when it can. Where to connect comes first: the name
   *  follows from it. */
  const problem = $derived.by(() => {
    const k = s.kind;
    if ("host" in k && !k.host.trim()) return _("Enter the host to connect to.");
    if ("host" in k && !(k.port >= 1 && k.port <= 65535)) return _("Use a port from 1 to 65535.");
    if (k.type === "ssh" && k.jump && !k.jump.host.trim()) return _("Enter the jump host, or untick it.");
    if (k.type === "serial" && !k.port.trim()) return _("Choose a serial port.");
    if (k.type === "agent" && k.tool === "custom" && !k.args.trim()) return _("Enter the command to run.");
    if (k.type === "browser" && !/^https?:\/\/[^\s/]+/.test(k.url.trim())) return _("Enter a web address starting with https://");
    if (!s.name.trim() && !fallback) return _("Give the session a name.");
    return "";
  });

  function save(connect = false) {
    const v = $state.snapshot(s);
    v.name = v.name.trim() || fallback;
    // a new session goes on the end
    const at = index ?? app.store.session.length;
    app.store.session[at] = v;
    saveStore();
    siteIcon(v);
    // Props read the parent's state, which closing clears: what to open is settled first.
    const go = connect ? onconnect : undefined;
    const saved = app.store.session[at];
    onclose();
    go?.(saved);
  }
</script>

<!-- An agent's MCP servers or skills, ticked on: unticked ones stay off in this session. Shut
     until opened, as every section below the main fields, so the dialog stays short. -->
{#snippet toggles(title: string, found: string[] | null, off: string[], note: string, tip: (n: string) => string = (n) => n)}
  {@const all = [...new Set([...(found ?? []), ...off])]}
  <details>
    <summary>{title} <span class="count">{found === null ? _("Looking…") : _("{on} of {all} on", { on: all.length - off.length, all: all.length })}</span></summary>
    <div class="toggles">
      <!-- Dozens of servers, plugins' among them: one click for the lot, then tick the few wanted. -->
      <div class="all">
        <button class="link" disabled={!off.length} onclick={() => off.splice(0, off.length)}>{_("All on")}</button>
        <button class="link" disabled={all.length === off.length} onclick={() => all.forEach((n) => serverOn(off, n, false))}>{_("All off")}</button>
      </div>
      <div class="list">
        {#each all as n (n)}
          <label class="check" title={tip(n)}>
            <input type="checkbox" checked={!off.includes(n)} onchange={(e) => serverOn(off, n, e.currentTarget.checked)} /><span class="name">{n}</span>
          </label>
        {/each}
      </div>
      <input placeholder={_("Turn off another by name, then Enter")} onkeydown={(e) => offByName(e, off)} />
      <p class="weak">{note}</p>
    </div>
  </details>
{/snippet}

<!-- Where it connects is one row for every type, as an SSH session's (Endpoint). -->
{#snippet hostPort(k: { host: string; port: number }, placeholder = "")}
  <span>{_("Host / port")}</span>
  <div class="row"><input class="grow" bind:value={k.host} {placeholder} /><input type="number" min="1" max="65535" bind:value={k.port} /></div>
{/snippet}

<!-- Hung from the top: switching types or opening a section grows it downwards, so the type picker stays under the pointer. -->
<Modal title={index === null ? _("New session") : _("Edit {name}", { name: session.name })} width={610} top onclose={close}>
  {#snippet foot()}
    <button class="primary" disabled={!!problem} onclick={() => save()}>{_("Save")}</button>
    {#if onconnect}<button disabled={!!problem} onclick={() => save(true)}>{_("Save and connect")}</button>{/if}
    <button onclick={close}>{_("Cancel")}</button>
    {#if problem}<span class="weak">{problem}</span>{/if}
  {/snippet}
  <!-- The type first: it decides every field below. -->
  <div class="kinds" role="group" aria-label={_("Type")}>
    {#each KINDS as [k, label] (k)}
      <button class:selected={s.kind.type === k} aria-pressed={s.kind.type === k} title={_(label)} style:--kind={KIND_COLOR[k]} onclick={() => s.kind.type !== k && setKind(k)}>
        <span class="glyph"><Icon name={KIND_ICON[k]} size={16} /></span><span class="name">{_(label)}</span>
      </button>
    {/each}
  </div>
  <div class="form">
    {#if s.kind.type === "ssh"}
      {@const ssh = s.kind}
      <Endpoint e={ssh} hint />
      <span>{_("Command")}</span><input bind:value={ssh.command} placeholder={_("run after login")} />
      <span>{_("Jump host")}</span>
      <label class="check">
        <input type="checkbox" checked={!!ssh.jump} onchange={(e) => (ssh.jump = (e.target as HTMLInputElement).checked ? newEndpoint() : null)} />
        {_("via SSH gateway")}
      </label>
      {#if ssh.jump}<Endpoint e={ssh.jump} />{/if}
    {:else if s.kind.type === "shell"}
      {@const sh = s.kind}
      <span>{_("Program")}</span><PathInput bind:value={sh.program} exec placeholder={_("default shell")} />
      <span>{_("Working folder")}</span><PathInput bind:value={() => sh.dir ?? "", (v) => (sh.dir = v)} dirs placeholder={_("your home folder")} />
    {:else if s.kind.type === "agent"}
      {@const a = s.kind}
      <span>{_("Tool")}</span>
      <select bind:value={a.tool}>{#each AGENTS as [id, label] (id)}<option value={id}>{_(label)}</option>{/each}</select>
      {#if a.tool === "custom"}
        <span>{_("Command")}</span><input bind:value={a.args} placeholder="aider --model sonnet" />
      {:else}
        <span>{_("Mode")}</span>
        <select bind:value={a.mode}>
          <option value="">{_("The tool's default")}</option>
          {#each AGENT_MODES as [id, label] (id)}<option value={id}>{_(label)}</option>{/each}
        </select>
        {#if EFFORT_TOOLS.includes(a.tool)}
          <span>{_("Effort")}</span>
          <select bind:value={a.effort}>
            <option value="">{_("The tool's default")}</option>
            {#each AGENT_EFFORTS as e (e)}<option value={e}>{e}</option>{/each}
          </select>
        {/if}
        <span>{_("Model")}</span><input bind:value={a.model} placeholder={_("the tool's default, or a name it knows")} />
        <span>{_("Arguments")}</span><input bind:value={a.args} placeholder={_("optional, e.g. --continue")} />
      {/if}
      <span>{_("Working folder")}</span><PathInput bind:value={a.dir} dirs placeholder={_("your home folder")} />
      <span>{_("Environment")}</span>
      <textarea bind:value={a.env} rows="2" placeholder={_("NAME=value, one a line")} spellcheck="false"></textarea>
      <span></span>
      <p class="weak">
        {_("Runs the tool in a local terminal. Install it first: its command has to be on your PATH.")}
        {_("Nothing here changes the tool's own settings: every choice holds for this session only.")}
        {#if a.tool === "codex"}{_("Codex runs without its shared background server, so the session's servers and settings stay its own.")}{/if}
        {#if a.tool === "omp"}{_("omp takes MCP servers only from its own mcp.json: add xtermium's there yourself, with the token from Settings › MCP.")}{/if}
      </p>
    {:else if s.kind.type === "serial"}
      <span>{_("Port")}</span>
      <input list="serial-ports" bind:value={s.kind.port} />
      <datalist id="serial-ports">{#each ports as p (p)}<option value={p}></option>{/each}</datalist>
      <span>{_("Baud")}</span><input type="number" min="50" bind:value={s.kind.baud} />
    {:else if s.kind.type === "rdp"}
      {@const rdp = s.kind}
      {@render hostPort(rdp)}
      <Login bind:value={() => rdp.credential ?? "", (v) => (rdp.credential = v)} />
      {#if !rdp.credential}<span>{_("User")}</span><input bind:value={rdp.user} placeholder={_("DOMAIN\\user; leave empty to be asked")} />{/if}
    {:else if s.kind.type === "browser"}
      {@const web = s.kind}
      <span>{_("URL")}</span><input bind:value={web.url} />
      <!-- Only the login's SSH gateway is used: the pages go through it. -->
      <Login bind:value={() => web.credential ?? "", (v) => (web.credential = v)} />
      {#if app.store.credential.find((c) => c.id === web.credential)?.gateway}
        <Hint id="gateway-logins">
          {_("Websites opened through an SSH gateway don't share sign-ins with other website tabs, so you may have to sign in again in each one.")}
        </Hint>
      {/if}
      <!-- On Windows the page hands xtermium's shortcuts back (src/web.rs). -->
      {#if app.os !== "windows"}
        <Hint id="page-keys">
          {_("While a website has the keyboard, xtermium's shortcuts are paused: click outside the page (the tab bar or the address bar) to use them again.")}
        </Hint>
      {/if}
    {:else if s.kind.type === "kvm"}
      {@const k = s.kind}
      {@render hostPort(k, "192.0.2.20")}
      <span>{_("Vendor")}</span>
      <select bind:value={k.vendor}>{#each KVM_VENDORS as [id, name] (id)}<option value={id}>{name}</option>{/each}</select>
      <Login bind:value={() => k.credential ?? "", (v) => (k.credential = v)} />
      <span></span>
      <p class="weak">
        {_("Opens the BMC's own HTML5 console, signed in with the shared login: Supermicro X11 and newer, iDRAC 7 and 8, iLO 4 (firmware 2.70 or newer) and 5, and boards with AMI's MegaRAC SP-X web interface. The console tab can paste your clipboard as key presses.")}
      </p>
    {:else if s.kind.type === "winrm"}
      {@const w = s.kind}
      {@render hostPort(w)}
      <Login bind:value={() => w.credential ?? "", (v) => (w.credential = v)} />
      {#if !w.credential}<span>{_("User")}</span><input bind:value={w.user} placeholder={_("DOMAIN\\user; leave empty to use your Windows login")} />{/if}
      <span></span>
      <label class="check">
        <input
          type="checkbox"
          checked={w.https}
          onchange={(e) => {
            w.https = (e.target as HTMLInputElement).checked;
            if (w.port === (w.https ? 5985 : 5986)) w.port = w.https ? 5986 : 5985;
          }}
        />
        {_("HTTPS (WinRMS)")}
      </label>
      {#if w.https}
        <span></span>
        <label class="check"><input type="checkbox" bind:checked={w.insecure} />{_("Don't check the server's certificate (self-signed)")}</label>
      {/if}
      <span></span>
      <p class="weak">
        {_("Opens PowerShell remoting with the local PowerShell. xtermium asks for the password once and can remember it. Over HTTP, hosts outside your domain must be in WinRM's TrustedHosts.")}
      </p>
    {:else if s.kind.type === "telnet" || s.kind.type === "vnc"}
      {@const k = s.kind}
      {@render hostPort(k)}
      {#if k.type === "vnc"}<Login bind:value={() => k.credential ?? "", (v) => (k.credential = v)} />{/if}
      <span></span>
      <p class="weak">
        {_("{protocol} isn't encrypted: use it on networks you trust, or through an SSH tunnel.", { protocol: s.kind.type === "vnc" ? "VNC" : "Telnet" })}
        {#if s.kind.type === "vnc"}{_("The password is asked for when the server wants one.")}{/if}
      </p>
    {/if}
    <span>{_("Name")}</span><input bind:value={s.name} placeholder={fallback} />
    <span>{_("Folder")}</span>
    <input list="editor-folders" bind:value={s.folder} placeholder="Prod/DB" />
    <datalist id="editor-folders">{#each folders as f (f)}<option value={f}></option>{/each}</datalist>
  </div>
  {#if s.kind.type === "agent" && MCP_TOOLS.includes(s.kind.tool)}
    {@render toggles(
      _("MCP servers"),
      servers,
      s.kind.mcp_off,
      _("A server turned off isn't started, so its tools cost the agent no tokens. The tool's own settings don't change."),
      (n) => (n === "xtermium" ? _("xtermium's own tools, there while Settings › MCP is on") : n),
    )}
  {/if}
  {#if s.kind.type === "agent" && SKILL_TOOLS.includes(s.kind.tool)}
    {@render toggles(_("Skills"), skills, s.kind.skills_off, _("A skill turned off isn't listed to the agent, so it costs no tokens. The tool's own settings don't change."))}
  {/if}
  {#if s.kind.type === "ssh" || s.kind.type === "rdp" || broadcasts(s.kind)}
    <details>
      <summary>{_("Advanced")}</summary>
      <div class="form">
        {#if s.kind.type === "ssh"}
          {@const ssh = s.kind}
          <span>{_("Ciphers")}</span>
          <div class="picture">
            <label class="check">
              <input
                type="checkbox"
                checked={ssh.ciphers.length > 0}
                onchange={(e) => (ssh.ciphers = e.currentTarget.checked ? [...(app.settings?.ssh_ciphers ?? app.ciphers)] : [])}
              />
              {_("Use its own list instead of the one in Settings › Defaults")}
            </label>
            <Ciphers bind:value={() => (ssh.ciphers.length ? ssh.ciphers : (app.settings?.ssh_ciphers ?? [])), (v) => (ssh.ciphers = v)} disabled={!ssh.ciphers.length} />
          </div>
          <span>{_("Keepalive")}</span>
          <div class="row">
            <input
              type="number"
              min="0"
              max="3600"
              placeholder={String(app.settings?.ssh_keepalive ?? 30)}
              value={ssh.keepalive ?? ""}
              oninput={(e) => (ssh.keepalive = e.currentTarget.value === "" ? null : Math.max(0, Math.round(+e.currentTarget.value)))}
            />{_("seconds; empty uses Settings › Defaults, 0 turns it off")}
          </div>
          <span>{_("Compression")}</span>
          <select
            value={ssh.compression === null ? "" : String(ssh.compression)}
            onchange={(e) => (ssh.compression = e.currentTarget.value === "" ? null : e.currentTarget.value === "true")}
          >
            <option value="">{app.settings?.ssh_compression ? _("Default (on)") : _("Default (off)")}</option>
            <option value="true">{_("On")}</option>
            <option value="false">{_("Off")}</option>
          </select>
          <span>{_("Agent")}</span>
          <label class="check" title={_("The server's admins can use your keys while you're connected: only for servers you trust")}>
            <input type="checkbox" bind:checked={ssh.agent_forward} />{_("Forward my SSH agent (for git or hopping on from this server; trusted servers only)")}
          </label>
        {:else if s.kind.type === "rdp"}
          {@const rdp = s.kind}
          <span>{_("Picture")}</span>
          <div class="picture">
            <label class="check">
              <input
                type="checkbox"
                checked={rdp.quality !== null}
                onchange={(e) => (rdp.quality = (e.target as HTMLInputElement).checked ? (app.settings?.rdp_quality ?? 0) : null)}
              />
              {_("Use its own setting instead of the one in Settings › Defaults")}
            </label>
            <QualityBar bind:value={() => rdp.quality ?? app.settings?.rdp_quality ?? 0, (v) => (rdp.quality = v)} disabled={rdp.quality === null} />
          </div>
          <span>{_("Shared folders")}</span>
          <div class="picture">
            <label class="check"><input type="checkbox" bind:checked={() => !!rdp.no_shares, (v) => (rdp.no_shares = v)} />{_("Turn them off for this session: the file panel doesn't open them")}</label>
            <select
              disabled={!!rdp.no_shares}
              value={rdp.smb_login == null ? "" : rdp.smb_login ? "yes" : "no"}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                rdp.smb_login = v === "" ? null : v === "yes";
              }}
            >
              <option value="">{app.settings?.smb_auto_login ? _("Open with the desktop's login (as in Settings)") : _("Ask which account (as in Settings)")}</option>
              <option value="yes">{_("Open with the desktop's login")}</option>
              <option value="no">{_("Ask which account")}</option>
            </select>
          </div>
        {/if}
        {#if broadcasts(s.kind)}
          <span>{_("Font")}</span>
          <input list="editor-fonts" bind:value={s.style.font} placeholder={_("system family, e.g. FiraCode Nerd Font Mono")} />
          <datalist id="editor-fonts">{#each fonts as f (f)}<option value={f}></option>{/each}</datalist>
          <span>{_("Font size")}</span><input type="number" min="6" max="48" step="0.5" bind:value={s.style.font_size} />
          <span>TERM</span><input bind:value={s.style.term} />
        {/if}
      </div>
    </details>
  {/if}
  <details>
    <summary>{_("Icon, colour and notes")}</summary>
    <div class="form">
      <Appearance
        bind:color={s.color}
        bind:icon={s.icon}
        bind:still={() => !!s.still, (v) => (s.still = v)}
        fallbackIcon={KIND_ICON[s.kind.type]}
        fallbackColor={folderColor(s.folder)}
        iconColor={KIND_COLOR[s.kind.type]}
        label={_("Name colour")}
        name={s.name.trim() || fallback}
        site={s.kind.type === "browser" && /^https?:\/\/[^\s/]+/.test(s.kind.url.trim()) ? s.kind.url.trim() : ""}
      />
      <span>{_("Notes")}</span>
      <textarea bind:value={s.notes} rows="3" placeholder={_("Anything worth remembering: owner, purpose, ticket")}></textarea>
    </div>
  </details>
</Modal>

{#if discarding}
  <!-- Escape (or ×) keeps editing; so does Enter, which lands on Keep editing. -->
  <Modal title={_("Discard changes?")} width={377} onclose={() => (discarding = false)}>
    <p>{_("What you changed in this session isn't saved.")}</p>
    <div class="row">
      <button class="primary danger" onclick={onclose}>{_("Discard")}</button>
      <button {@attach (el) => el.focus()} onclick={() => (discarding = false)}>{_("Keep editing")}</button>
    </div>
  </Modal>
{/if}

<style>
  /* two even rows of five, whatever the language: the names end in … rather than reflowing */
  .kinds {
    display: grid;
    grid-template-columns: repeat(5, minmax(0, 1fr));
    gap: var(--gap);
  }
  .kinds button {
    min-width: 0;
    justify-content: start;
    padding: 1px;
    padding-inline-end: 5px;
  }
  .kinds .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* the button's curve, less the room around the tile */
  .kinds .glyph {
    border-radius: calc(var(--r-control) - 2px);
  }
  .picture {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  summary {
    cursor: pointer;
    color: var(--weak);
  }
  summary:hover {
    color: var(--fg);
  }
  details[open] > summary {
    margin-bottom: 8px;
  }
  /* a note under the fields sits a row's gap away, like a field, not a paragraph's */
  .form > p {
    margin: 0;
  }
  /* how many are on, beside the closed section's name */
  .count {
    margin-inline-start: 5px;
    font-size: var(--t-s);
  }
  .toggles {
    display: flex;
    flex-direction: column;
    gap: var(--gap);
    min-width: 0;
  }
  .toggles .all {
    display: flex;
    gap: 13px;
    font-size: var(--t-s);
  }
  /* columns of names, scrolling past six rows: some people have dozens */
  .toggles .list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(calc(var(--row) * 9), 1fr));
    gap: 2px 8px;
    max-height: calc(var(--row) * 6);
    overflow-y: auto;
  }
  .toggles .check {
    min-width: 0;
  }
  .toggles .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
