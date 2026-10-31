// Run with `npm test`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { DASH, dashFind, dashUrl } from "./dashicons.ts";

test("a name finds its Dashboard Icon", () => {
  assert.equal(dashFind("Proxmox")?.name, "proxmox");
  assert.equal(dashFind("gitlab.home.arpa")?.name, "gitlab", "a host name's first label");
  assert.equal(dashFind("ba-jenkins-slave-1")?.name, "jenkins", "a piece of a dashed name");
  assert.equal(dashFind("laptop.home.arpa (rdp)")?.name, undefined);
  assert.equal(dashFind(""), undefined);
});

test("pictures come from the CDN in the theme's drawing", () => {
  const github = DASH.find((i) => i.name === "github")!;
  assert.ok(github.ops && github.light && !github.dark, "GitHub's plain drawing is for dark themes");
  assert.equal(dashUrl(github, true), "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/github.svg");
  assert.equal(dashUrl(github, false), "https://cdn.jsdelivr.net/gh/homarr-labs/dashboard-icons/svg/github-light.svg");
  const png = DASH.find((i) => i.png)!;
  assert.match(dashUrl(png, true), /\/png\/[^/]+\.png$/);
});
