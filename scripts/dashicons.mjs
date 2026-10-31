// Refreshes ui/lib/dashicons.json, the catalogue of Dashboard Icons (dashboardicons.com,
// github.com/homarr-labs/dashboard-icons) that the icon picker searches: `node scripts/dashicons.mjs`.
// An entry is [name, aliases joined by "|", flags]: 1 = PNG only, 2 = has a dark-theme drawing
// (`<name>-dark`), 8 = a light-theme one (`<name>-light`), 4 = for the people xtermium is for
// (sysadmins, network and devops people), by category; those show before anything is searched.
// The pictures load from jsDelivr when shown.
import { writeFileSync } from "node:fs";

const OPS = new Set([
  "network", "networking", "networking-tools", "security", "monitoring", "monitoring-tools", "automation", "storage", "database", "databases",
  "server-panels", "cloud-computing", "operating-systems", "linux-distributions", "containerization-&-orchestration", "version-control-systems",
  "devops", "vpn", "infrastructure", "hardware", "self-hosted", "password-managers",
]);
/** A name as the catalogue spells them: lower case, words joined by dashes. */
const norm = (s) => s.toLowerCase().trim().replace(/[\s_]+/g, "-").replace(/[^a-z0-9.-]/g, "");

const meta = await (await fetch("https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/metadata.json")).json();
const list = Object.entries(meta)
  .sort(([a], [b]) => a.localeCompare(b))
  .map(([name, m]) => {
    const aliases = [...new Set((m.aliases ?? []).map(norm).filter((a) => a && a !== name))].join("|");
    const ops = (m.categories ?? []).some((c) => OPS.has(c.toLowerCase()));
    const flags = (m.base === "png" ? 1 : 0) | (m.colors?.dark === `${name}-dark` ? 2 : 0) | (ops ? 4 : 0) | (m.colors?.light === `${name}-light` ? 8 : 0);
    return [name, aliases, flags];
  });
writeFileSync(new URL("../ui/lib/dashicons.json", import.meta.url), JSON.stringify(list) + "\n");
console.log(`${list.length} icons, ${list.filter((i) => i[2] & 4).length} for operations`);
