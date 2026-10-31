// Release notes for a build: what reached main since the previous release, one section per
// merged pull request with its commits' bullet lists, then the commits made on main directly.
//
//   node scripts/changelog.mjs [ref] [previous tag]
//
// ref defaults to HEAD; the previous tag to the last release tag (v<semver>) before it ("" for none:
// everything up to ref). Prints the markdown; in GitHub Actions it's also the step output `body`
// (release.yml passes it to tauri-action as the release description).
import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const REPO = "https://github.com/xtermium/xtermium";

const git = (...args) => execFileSync("git", args, { encoding: "utf8", maxBuffer: 64 << 20 }).trim();

/** Commits git log lists for `args`: hash, parents, subject, body. */
function commits(...args) {
  return git("log", "--format=%H%x1f%P%x1f%s%x1f%b%x1e", ...args)
    .split("\x1e")
    .map((c) => c.replace(/^\n+/, ""))
    .filter(Boolean)
    .map((c) => {
      const [hash, parents, subject, body = ""] = c.split("\x1f");
      return { hash, parents: parents.split(" "), subject, body };
    });
}

/** A commit body as markdown list items: a wrapped line joins its item, a paragraph becomes one,
 *  an indented "- " stays nested, trailers (Co-Authored-By and the like) are dropped. */
export function items(body) {
  const out = [];
  let open = false; // the last item can take more lines
  for (const line of body.split("\n")) {
    const text = line.trim();
    if (!text || /^[\w-]+: .*<[^>]*@[^>]*>$/.test(text)) {
      open = false;
      continue;
    }
    const bullet = /^(\s*)[-*] (.*)$/.exec(line);
    if (bullet) {
      out.push(`${bullet[1].length >= 2 ? "  " : ""}- ${bullet[2].trim()}`);
      open = true;
    } else if (open) out[out.length - 1] += ` ${text}`;
    else {
      out.push(`- ${text}`);
      open = true;
    }
  }
  return out;
}

const short = (hash) => `[\`${hash.slice(0, 7)}\`](${REPO}/commit/${hash})`;
const indent = (lines) => lines.map((l) => `  ${l}`);

/** One commit as an item: its subject, with its body's items under it. */
const entry = (c) => [`- **${c.subject}** ${short(c.hash)}`, ...indent(items(c.body))];

/** Release notes longer than this list only the pull requests' titles: tauri-action takes the
 *  notes as an input, which reaches it as an environment variable, and Windows holds 32767
 *  characters in one. The full notes can still go on the release afterwards (`gh release edit`). */
export const LIMIT = 30000;

/** The markdown for `ref` since `prev` ("" for the whole history); `brief`: titles only. */
export function changelog(ref, prev, brief = false) {
  const range = prev ? [`${prev}..${ref}`] : [ref];
  const sections = [];
  const direct = [];
  for (const c of commits("--first-parent", ...range)) {
    if (c.parents.length < 2) {
      direct.push(c);
      continue;
    }
    // A merge: GitHub puts the pull request's title in its body.
    const pr = /^Merge pull request #(\d+)/.exec(c.subject)?.[1];
    const title = `${c.body.split("\n")[0].trim() || c.subject}${pr ? ` ([#${pr}](${REPO}/pull/${pr}))` : ""}`;
    if (brief) {
      sections.push(`- ${title}`);
      continue;
    }
    const inside = commits("--no-merges", "--reverse", `${c.parents[0]}..${c.parents[1]}`);
    const lines =
      inside.length === 1 && title.startsWith(inside[0].subject)
        ? items(inside[0].body)
        : inside.flatMap(entry);
    sections.push([`### ${title}`, "", ...lines].join("\n"));
  }
  if (brief && sections.length) sections.splice(0, sections.length, ["### Pull requests (each one lists its changes)", "", ...sections].join("\n"));
  if (direct.length) {
    const list = direct.reverse().flatMap((c) => (brief ? [`- ${c.subject} ${short(c.hash)}`] : entry(c)));
    sections.push(["### Straight to main", "", ...list].join("\n"));
  }

  const head = git("rev-parse", ref);
  const stats = prev ? git("diff", "--shortstat", prev, ref) : "";
  const count = git("rev-list", "--count", "--no-merges", ...range);
  const lead = prev ? `Changes since ${prev}` : "Everything up to this release";
  const foot = [
    `${count} commit${count === "1" ? "" : "s"}${stats ? `, ${stats}` : ""}`,
    prev ? `[full diff](${REPO}/compare/${prev}...${head})` : "",
    `built from ${short(head)}`,
  ].filter(Boolean);
  return [
    `## ${lead}`,
    "",
    sections.length ? sections.join("\n\n") : "_No changes: a rebuild of the same commit._",
    "",
    "---",
    "",
    foot.join(" · "),
    "",
  ].join("\n");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const ref = process.argv[2] || "HEAD";
  let prev = process.argv[3];
  if (prev === undefined) {
    // Before ref, so a rebuild of a commit that has a release already lists what that one did.
    try {
      prev = git("describe", "--tags", "--abbrev=0", "--match", "v[0-9]*", `${ref}^`);
    } catch {
      prev = "";
    }
  }
  let body = changelog(ref, prev);
  if (body.length > LIMIT) body = changelog(ref, prev, true);
  process.stdout.write(body);
  if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `body<<CHANGELOG_END\n${body}\nCHANGELOG_END\n`);
}
