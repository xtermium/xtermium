// Dashboard Icons (dashboardicons.com, github.com/homarr-labs/dashboard-icons): the catalogue is
// dashicons.json (scripts/dashicons.mjs refreshes it); the pictures come from jsDelivr, shown in
// <img> (tauri.conf.json lets img-src have the CDN) and, once picked, kept as a `data:` URL through
// fetch_icon like any other image.
import catalog from "./dashicons.json" with { type: "json" };

/** `ops`: for sysadmins, network and devops people (shown before a search). `dark`/`light`: it has a
 *  drawing for that theme (`<name>-dark`, `<name>-light`); the plain one suits the other. */
export type DashIcon = { name: string; aliases: string[]; png: boolean; dark: boolean; light: boolean; ops: boolean };

export const DASH: DashIcon[] = (catalog as [string, string, number][]).map(([name, aliases, f]) => ({
  name,
  aliases: aliases ? aliases.split("|") : [],
  png: !!(f & 1),
  dark: !!(f & 2),
  ops: !!(f & 4),
  light: !!(f & 8),
}));

const CDN = "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons";

/** Where the picture is, in the drawing for the theme when it has one. */
export function dashUrl(i: DashIcon, dark: boolean): string {
  const ext = i.png ? "png" : "svg";
  const variant = dark && i.dark ? "-dark" : !dark && i.light ? "-light" : "";
  return `${CDN}/${ext}/${i.name}${variant}.${ext}`;
}

/** A name as the catalogue spells them: lower case, words joined by dashes. */
const norm = (s: string) => s.toLowerCase().trim().replace(/[\s_]+/g, "-").replace(/[^a-z0-9.-]/g, "");

/** The icon for something called `name`: the whole name, then each label of a host name, then each
 *  dash-separated piece of four letters or more ("ba-jenkins-slave-1" runs Jenkins), matched on the
 *  icons' names and aliases. */
export function dashFind(name: string): DashIcon | undefined {
  const n = norm(name);
  const labels = n.split(".").filter(Boolean);
  const pieces = labels.flatMap((l) => l.split("-")).filter((p) => p.length >= 4);
  for (const c of [n, ...labels, ...pieces]) {
    const hit = DASH.find((i) => i.name === c || i.aliases.includes(c));
    if (hit) return hit;
  }
}
