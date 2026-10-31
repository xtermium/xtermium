<script lang="ts">
  // A tunnel, drawn so it explains itself: where clients connect, the encrypted SSH tube, and
  // where the traffic comes out. Packets flow out on top and back below; their number and speed
  // follow real throughput. `demo` animates a preview (the new-tunnel form).
  import { _ } from "./i18n.svelte";
  import { human, type Tunnel, type TunnelStats } from "./api";
  import Icon from "./Icon.svelte";

  let { t, stats, demo = false }: { t: Tunnel; stats?: TunnelStats; demo?: boolean } = $props();

  const live = $derived(!!stats?.live);
  const rate = $derived(live ? (stats!.history.at(-1) ?? [0, 0]) : [0, 0]);
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let time = $state(0);

  $effect(() => {
    if (still || (!live && !demo)) return;
    let raf = requestAnimationFrame(function tick(ms) {
      time = ms / 1000;
      raf = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(raf);
  });

  // Layout, in viewBox units.
  const W = 800;
  const C = 70; // centre line
  const X = { start: 18, a: 160, b: 470, c: 715 };
  const R = 30; // node radius
  const LANE = 7;

  const or = (v: string | number, fallback: string) => (v ? String(v) : fallback);
  const sshAddr = $derived(`${or(t.ssh.host, _("server"))}:${t.ssh.port}`);
  const listen = $derived(`${or(t.bind, "127.0.0.1")}:${or(t.listen_port, _("port"))}`);
  const dest = $derived(`${or(t.dest_host, _("host"))}:${or(t.dest_port, _("port"))}`);

  type Node = { icon: string; role: string; addr: string };
  const scene = $derived.by((): { clients: string; nodes: [Node, Node, Node]; text: string } => {
    switch (t.kind) {
      case "Local":
        return {
          clients: _("your apps"),
          nodes: [
            { icon: "laptop", role: _("This computer"), addr: listen },
            { icon: "server", role: _("SSH server"), addr: sshAddr },
            { icon: "target-lock", role: _("Destination"), addr: dest },
          ],
          text: _("Apps on this computer connect to {listen}; the traffic goes encrypted to {ssh}, which passes it on to {dest}.", { listen, ssh: sshAddr, dest }),
        };
      case "Remote":
        return {
          clients: _("anyone"),
          nodes: [
            { icon: "world", role: _("SSH server"), addr: _("{host} port {port}", { host: or(t.ssh.host, _("server")), port: or(t.listen_port, "?") }) },
            { icon: "laptop", role: _("This computer"), addr: _("via SSH") },
            { icon: "target-lock", role: _("Destination"), addr: dest },
          ],
          text: _("Connections to port {port} on {host} come back through SSH to this computer, which passes them on to {dest}.", {
            port: or(t.listen_port, "?"),
            host: or(t.ssh.host, _("the server")),
            dest,
          }),
        };
      case "Dynamic":
        return {
          clients: _("your apps"),
          nodes: [
            { icon: "laptop", role: _("SOCKS5 proxy"), addr: listen },
            { icon: "server", role: _("SSH server"), addr: sshAddr },
            { icon: "globe", role: _("Any host"), addr: stats?.target || _("per request") },
          ],
          text: _("Apps set to use the SOCKS5 proxy at {listen} can reach any host from {ssh}; the traffic goes there encrypted.", { listen, ssh: sshAddr }),
        };
    }
  });

  /** Packet positions (x) on one lane: count and speed grow with throughput. */
  function packets(r: number, back: boolean): number[] {
    let n: number, speed: number;
    if (live && r > 0) {
      const mag = Math.log10(Math.max(r, 1));
      n = Math.min(7, Math.max(1, Math.floor(1 + mag / 1.5)));
      speed = 0.12 + 0.05 * mag;
    } else if (demo) {
      [n, speed] = [2, 0.16];
    } else if (live && !back) {
      [n, speed] = [1, 0.07]; // open but quiet: one slow keep-alive
    } else return [];
    const span = X.c - X.start;
    return Array.from({ length: n }, (_, i) => {
      const f = (time * speed + i / n) % 1;
      return back ? X.c - f * span : X.start + f * span;
    });
  }

  const inPipe = (x: number) => x > X.a + R && x < X.b - R;
  const short = (s: string, n = 26) => (s.length > n ? s.slice(0, n - 1) + "…" : s);
  const uid = Math.random().toString(36).slice(2);
  const on = $derived(live || demo);

  const peak = $derived(Math.max(1, ...(stats?.history ?? []).flat()));
  function spark(lane: 0 | 1): string {
    const h = stats?.history ?? [];
    return h.map((v, i) => `${((60 - h.length + i) / 59) * 100},${40 - (v[lane] / peak) * 36}`).join(" ");
  }
</script>

<div class="flow" class:on>
  <p class="caption">{scene.text}</p>
  <svg viewBox="0 0 {W} 150" role="img" aria-label={scene.text} style:direction="ltr">
    <defs>
      <marker id="a{uid}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
        <path d="M0,0 L8,4 L0,8 z" fill="context-stroke" />
      </marker>
      <linearGradient id="g{uid}" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0" stop-color="var(--gold)" stop-opacity="0.26" />
        <stop offset="1" stop-color="var(--gold)" stop-opacity="0.08" />
      </linearGradient>
    </defs>

    <!-- where clients come from -->
    <text x={X.start} y={C - 18} class="small">{scene.clients}</text>
    <line x1={X.start} x2={X.a - R - 4} y1={C - LANE} y2={C - LANE} class="lane fwd" marker-end="url(#a{uid})" />
    <line x1={X.a - R - 4} x2={X.start} y1={C + LANE} y2={C + LANE} class="lane back" marker-end="url(#a{uid})" />

    <!-- the SSH tube -->
    <rect x={X.a + R - 6} y={C - 21} width={X.b - X.a - 2 * R + 12} height="42" rx="21" class="pipe" fill={on ? `url(#g${uid})` : "rgba(var(--deep-rgb), 1)"} />
    <text x={(X.a + X.b) / 2} y={C + 40} text-anchor="middle" class="small">{_("encrypted inside SSH")}</text>

    <!-- out to the destination -->
    <line x1={X.b + R + 4} x2={X.c - R - 4} y1={C - LANE} y2={C - LANE} class="lane fwd" marker-end="url(#a{uid})" />
    <line x1={X.c - R - 4} x2={X.b + R + 4} y1={C + LANE} y2={C + LANE} class="lane back" marker-end="url(#a{uid})" />
    {#if t.kind === "Dynamic"}
      {#each [-26, 0, 26] as dy (dy)}
        <line x1={X.c + R + 2} y1={C + dy * 0.4} x2={W - 6} y2={C + dy} class="ray" />
      {/each}
    {/if}

    <!-- traffic: open packets outside, sealed capsules inside the tube -->
    {#each [[0, rate[0]], [1, rate[1]]] as [lane, r] (lane)}
      {@const y = lane === 0 ? C - LANE : C + LANE}
      {#each packets(r, lane === 1) as x, k (k)}
        {#if inPipe(x)}
          <rect x={x - 8} y={y - 3.5} width="16" height="7" rx="3.5" class:fwd-dot={lane === 0} class:back-dot={lane === 1} />
        {:else}
          <circle cx={x} cy={y} r="3.6" class:fwd-dot={lane === 0} class:back-dot={lane === 1} />
        {/if}
      {/each}
    {/each}

    <!-- lock on the tube -->
    <circle cx={(X.a + X.b) / 2} cy={C - 21} r="13" class="badge" />
    <foreignObject x={(X.a + X.b) / 2 - 8} y={C - 29} width="16" height="16">
      <Icon name="lock-alt" size={16} color={on ? "var(--gold)" : "var(--weak)"} />
    </foreignObject>

    <!-- stations, drawn last so traffic passes through them -->
    {#each scene.nodes as n, i (i)}
      {@const x = [X.a, X.b, X.c][i]}
      <circle cx={x} cy={C} r={R} class="node" />
      <foreignObject x={x - 12} y={C - 12} width="24" height="24"><Icon name={n.icon} size={24} /></foreignObject>
      <text {x} y={C + R + 22} text-anchor="middle" class="role">{n.role}</text>
      <text {x} y={C + R + 40} text-anchor="middle" class="addr"><title>{n.addr}</title>{short(n.addr)}</text>
    {/each}
  </svg>

  {#if live}
    <div class="row wrap stats">
      <span class="dot" style:background="var(--fwd)"></span>{_("{rate}/s out", { rate: human(rate[0]) })}
      <span class="dot" style:background="var(--back)"></span>{_("{rate}/s back", { rate: human(rate[1]) })}
      <span class="weak"
        >{_("{open} open of {total} connections, {sent} sent, {received} received", {
          open: stats!.active,
          total: stats!.total,
          sent: human(stats!.sent),
          received: human(stats!.received),
        })}</span
      >
    </div>
    <div class="spark">
      <svg viewBox="0 0 100 44" preserveAspectRatio="none">
        <polyline points={spark(0)} fill="none" stroke="var(--fwd)" stroke-width="1.5" vector-effect="non-scaling-stroke" />
        <polyline points={spark(1)} fill="none" stroke="var(--back)" stroke-width="1.5" vector-effect="non-scaling-stroke" />
      </svg>
      <span class="weak">{_("last minute, peak {rate}/s", { rate: human(peak) })}</span>
    </div>
  {:else if !demo}
    <div class="weak">{_("Stopped. Start it to open the tunnel.")}</div>
  {/if}
</div>

<style>
  .flow {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .caption {
    margin: 0;
    max-width: 75ch;
  }
  svg {
    width: 100%;
    max-height: 170px;
    overflow: visible;
  }
  text {
    fill: var(--fg);
    font-family: var(--ui);
  }
  .small {
    font-size: var(--t-s);
    fill: var(--weak);
  }
  .role {
    font-size: var(--t-l);
    font-weight: 600;
  }
  .addr {
    font-size: var(--t);
    fill: var(--weak);
    font-variation-settings: "MONO" 1;
  }
  .lane {
    stroke-width: 2;
    stroke: var(--line);
  }
  .on .lane.fwd {
    stroke: color-mix(in srgb, var(--fwd) 55%, transparent);
  }
  .on .lane.back {
    stroke: color-mix(in srgb, var(--back) 55%, transparent);
  }
  .ray {
    stroke: var(--line);
    stroke-width: 1.5;
    stroke-dasharray: 3 5;
  }
  .pipe {
    stroke: var(--line);
    stroke-width: 1.5;
    stroke-dasharray: 6 6;
  }
  .on .pipe {
    stroke: color-mix(in srgb, var(--gold) 70%, transparent);
    stroke-dasharray: none;
  }
  .badge {
    fill: rgba(var(--panel-rgb), 1);
    stroke: var(--line);
    stroke-width: 1.5;
  }
  .on .badge {
    stroke: var(--gold);
  }
  .node {
    fill: rgba(var(--deep-rgb), 1);
    stroke: var(--line);
    stroke-width: 1.5;
  }
  .on .node {
    stroke: color-mix(in srgb, var(--fwd) 60%, transparent);
  }
  .fwd-dot {
    fill: var(--fwd);
  }
  .back-dot {
    fill: var(--back);
  }
  .stats {
    font-size: var(--t);
    gap: 5px;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    margin-left: 5px;
  }
  .spark {
    position: relative;
    height: 44px;
    border-radius: 5px;
    background: rgba(var(--deep-rgb), 1);
  }
  .spark svg {
    width: 100%;
    height: 100%;
  }
  .spark span {
    position: absolute;
    left: 8px;
    top: 3px;
    font-size: var(--t-s);
  }
</style>
