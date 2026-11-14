<script lang="ts">
  // A Terminal Co-op ring of up to three people, drawn as one (Coop.svelte lists a bigger one): the
  // owner in the middle and the others at its ends, each with their picture, their name and what
  // they send and receive a second clear of the ring, and packets going round while people are
  // connected, more and faster with the traffic. `onpick` (the owner's dialog) makes the others'
  // seats buttons: a click or a right-click asks what to do with that person, and `picked` is whose
  // menu is open.
  import { _ } from "./i18n.svelte";
  import type { CoopMember } from "./api";
  import { pace, traffic, type RingFlow } from "./coop";
  import CoopPic from "./CoopPic.svelte";
  import { coopColor } from "./state.svelte";
  import Icon from "./Icon.svelte";

  let {
    people,
    flows = {},
    picked = "",
    onpick,
  }: { people: CoopMember[]; flows?: Record<string, RingFlow>; picked?: string; onpick?: (m: CoopMember, x: number, y: number) => void } = $props();

  // Layout, in viewBox units (the dialog's body is 351px wide, so they are pixels there).
  const W = 351;
  const [RX, RY] = [130, 26];
  const PIC = 34;
  /** Room for a name and a line of traffic. */
  const TAG = 34;
  const GAP = 3;
  const H = 2 * (RY + GAP + TAG);
  /** Where the text starts, from the far edge, in percent of the height. */
  const EDGE = ((H / 2 + RY + GAP) / H) * 100;
  /** The most packets on the ring, a quarter of a lap apart: slots that light up, never move over. */
  const SLOTS = 4;
  /** Where each person sits, in the order they come (the owner first): the owner in the middle of
   *  the ring, where everyone's connection meets, with their text over it; the second person at its
   *  left end and the third at its right, their text under it. `x` is in ring widths from the middle. */
  const SEATS = [
    { x: 0, above: true, side: "middle" },
    { x: -1, above: false, side: "left" },
    { x: 1, above: false, side: "right" },
  ];
  const seats = $derived(people.slice(0, SEATS.length).map((m, i) => ({ m, ...SEATS[i] })));

  /** Two people or more are connected: the ring is lit and packets go round. */
  const linked = $derived(people.filter((m) => m.online !== false).length > 1);
  /** The busiest person's bytes a second (the owner's, who carries everyone's). */
  const busy = $derived(Math.max(0, ...Object.values(flows).map((f) => f.up + f.down)));
  /** How many packets show: the first, then the one opposite, then the two between. */
  const count = $derived(pace(busy)[0]);
  const shown = (slot: number) => slot === 0 || (slot === 2 ? count > 1 : count > 2);

  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  /** How far round the packets are, 0 to 1. */
  let lap = $state(0);
  $effect(() => {
    if (still || !linked) return;
    // They start from rest and take a moment to reach a new speed, so a figure that changes every
    // second never jerks them.
    let [last, speed] = [performance.now(), 0];
    let raf = requestAnimationFrame(function tick(ms) {
      const dt = Math.min(0.1, Math.max(0, ms - last) / 1000);
      last = ms;
      speed += (pace(busy)[1] - speed) * (1 - Math.exp(-dt / 0.8));
      lap = (lap + dt * speed) % 1;
      raf = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(raf);
  });

  function pick(e: MouseEvent, m: CoopMember) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // A key press has no pointer: the menu opens under the picture.
    onpick?.(m, e.clientX || r.left, e.clientY || r.bottom);
  }
</script>

{#snippet figure(bytes: number, down: boolean)}
  {@const [n, unit] = traffic(bytes).split(" ")}
  <span class="fig"><span class="arrow" class:down><Icon name="up-arrow-alt" size={13} /></span><span class="n">{n}</span><span class="unit">{unit}/s</span></span>
{/snippet}

<div class="ring" class:on={linked} style:aspect-ratio="{W} / {H}" style:--edge="{EDGE}%" role="group" aria-label={_("In the ring")}>
  <svg viewBox="0 0 {W} {H}" aria-hidden="true">
    <ellipse cx={W / 2} cy={H / 2} rx={RX} ry={RY} class="track" />
    <ellipse cx={W / 2} cy={H / 2} rx={RX} ry={RY} class="lit" />
    {#if !still}
      {#each Array.from({ length: SLOTS }, (_, k) => k) as slot (slot)}
        {@const a = 2 * Math.PI * (lap + slot / SLOTS)}
        <circle cx={W / 2 + RX * Math.cos(a)} cy={H / 2 + RY * Math.sin(a)} r="3" class="packet" class:shown={linked && shown(slot)} />
      {/each}
    {/if}
  </svg>
  {#each seats as p (p.m.id)}
    {@const f = flows[p.m.id]}
    {@const left = `${((W / 2 + RX * p.x) / W) * 100}%`}
    {#if onpick && !p.m.owner}
      <button
        class="seat"
        class:off={p.m.online === false}
        class:picked={picked === p.m.id}
        style:left
        title={_("Rename or kick {name}", { name: p.m.name })}
        aria-label={_("Rename or kick {name}", { name: p.m.name })}
        aria-haspopup="menu"
        aria-expanded={picked === p.m.id}
        onclick={(e) => pick(e, p.m)}
        oncontextmenu={(e) => pick(e, p.m)}
      >
        <CoopPic name={p.m.name} image={p.m.image} size={PIC} color={coopColor(p.m.id)} />
      </button>
    {:else}
      <span class="seat" class:off={p.m.online === false} style:left><CoopPic name={p.m.name} image={p.m.image} size={PIC} color={coopColor(p.m.id)} /></span>
    {/if}
    <div class="who {p.side}" class:above={p.above}>
      <span class="name" title={p.m.name}>{p.m.you ? _("{name} (you)", { name: p.m.name }) : p.m.name}</span>
      {#if f}
        <span
          class="flow"
          role="img"
          aria-label={_("Sending {up} a second, receiving {down} a second", { up: traffic(f.up), down: traffic(f.down) })}
          title={_("{sent} sent, {received} received", { sent: traffic(f.sent), received: traffic(f.received) })}
        >
          {@render figure(f.up, false)}{@render figure(f.down, true)}
        </span>
      {/if}
    </div>
  {/each}
</div>

<style>
  /* Seats are placed by the ring's own geometry, the same in a right-to-left language. */
  .ring {
    position: relative;
    flex: none;
    direction: ltr;
  }
  svg {
    display: block;
    width: 100%;
    height: 100%;
  }
  /* The ring alone is a dashed line; with people connected an amber one comes up over it. */
  ellipse {
    fill: none;
    stroke-width: 2;
    transition: opacity 0.3s var(--ease-out);
  }
  .track {
    stroke: var(--line);
    stroke-dasharray: 3 5;
  }
  .lit {
    stroke: color-mix(in srgb, var(--gold-fill) 70%, transparent);
    opacity: 0;
  }
  .on .track {
    opacity: 0;
  }
  .on .lit {
    opacity: 1;
  }
  /* A packet comes and goes where it is; the others never move over for it. */
  .packet {
    fill: var(--gold-fill);
    opacity: 0;
    transition: opacity 0.5s var(--ease-out);
  }
  .packet.shown {
    opacity: 1;
  }
  .seat {
    position: absolute;
    top: 50%;
    translate: -50% -50%;
    display: grid;
    min-height: 0;
    padding: 0;
    border: 0;
    border-radius: 50%;
    /* the ring and its packets pass behind a seat, a dimmed one too */
    background: rgba(var(--panel-rgb), 1);
    /* the third person glides over to the left end when the second leaves, and a new one comes up in its own place */
    transition: left 0.3s var(--ease-out);
    animation: seat-in var(--glide) var(--ease-out) backwards;
  }
  @keyframes seat-in {
    from {
      opacity: 0;
      scale: 0.8;
    }
  }
  .seat > :global(*) {
    transition: opacity var(--glide) var(--ease-out);
  }
  /* someone let in whose app isn't connected now */
  .seat.off > :global(*) {
    opacity: 0.45;
  }
  button.seat:hover:not(:disabled) {
    background: rgba(var(--panel-rgb), 1);
    outline: 1px solid var(--weak);
    outline-offset: 2px;
  }
  button.seat:focus-visible:not(:disabled),
  button.seat.picked:not(:disabled) {
    outline: 1px solid var(--gold);
    outline-offset: 2px;
  }
  .who {
    position: absolute;
    display: flex;
    flex-direction: column;
    top: var(--edge);
    max-width: calc(50% - 3px);
    line-height: 1.3;
    animation: fade-in var(--glide) var(--ease-out) backwards;
  }
  .who.above {
    top: auto;
    bottom: var(--edge);
    flex-direction: column-reverse;
  }
  .who.left {
    left: 0;
    align-items: flex-start;
  }
  .who.right {
    right: 0;
    align-items: flex-end;
  }
  .who.middle {
    left: 50%;
    translate: -50% 0;
    align-items: center;
  }
  .name {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Two figures that hold still: every number and unit has its own fixed room, so a figure that
     gains a digit moves nothing beside it. */
  .flow {
    display: flex;
    gap: 8px;
    color: var(--weak);
    font-family: var(--mono);
    font-size: var(--t-s);
    white-space: nowrap;
  }
  .fig {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .arrow {
    display: grid;
  }
  .arrow.down {
    rotate: 180deg;
  }
  .n {
    min-width: 4ch;
    padding-inline-end: 0.5ch;
    color: var(--fg);
    text-align: end;
  }
  .unit {
    min-width: 4ch;
  }
</style>
