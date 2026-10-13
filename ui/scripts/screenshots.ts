// README screenshots from a synthetic athlete (no real personal data): `make screenshots`.
//
// Writes the demo athlete (crates/eenkah-testkit/examples/demo.rs) into a temporary data folder,
// imports and trains it with the release binary, serves it on a spare port and captures each
// graph from the dark theme into docs/img/. Needs Chromium or Chrome; set CHROME to its path if
// it is not on PATH.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { chromium, type Page } from "playwright-core";

const ROOT = resolve(import.meta.dirname, "../..");
const OUT = join(ROOT, "docs/img");
const BIN = join(ROOT, "target/release/eenkah");
const PORT = 8097;
const URL = `http://127.0.0.1:${PORT}`;

/** [page, panel title (prefix), file name] — one image per graph. */
const SHOTS: [string, string, string][] = [
  ["/", "Signals against your last 60 days", "today-signals"],
  ["/", "Is my HRV inside my normal range?", "today-hrv-range"],
  ["/", "How did last night compare with your normal?", "today-normal-ranges"],
  ["/", "Has your normal moved?", "today-shifts"],
  ["/model", "Tomorrow morning's HRV", "model-backtest"],
  ["/model", "Tomorrow's resting HR: forecast vs what happened", "model-forecast-vs-actual"],
  ["/trends", "Fitness, fatigue and form", "trends-fitness-fatigue"],
  ["/trends", "Does short sleep show up in HRV?", "trends-sleep-vs-hrv"],
  ["/trends", "When did I train?", "trends-calendar"],
  ["/week", "Training load: this week vs last week", "week-load"],
];

function findChrome(): string {
  if (process.env.CHROME) return process.env.CHROME;
  for (const name of ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable"]) {
    try {
      return execFileSync("which", [name], { encoding: "utf8" }).trim();
    } catch {
      // try the next name
    }
  }
  for (const p of ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "/Applications/Chromium.app/Contents/MacOS/Chromium"]) {
    if (existsSync(p)) return p;
  }
  throw new Error("Chromium or Chrome not found: set CHROME=/path/to/chrome");
}

async function waitForServer(): Promise<void> {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(URL)).ok) return;
    } catch {
      // not listening yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error(`eenkah serve did not answer on ${URL}`);
}

async function open(page: Page, path: string): Promise<void> {
  await page.goto(URL + path, { waitUntil: "networkidle" });
  await page.waitForTimeout(1500); // charts size themselves after the first layout
}

async function main(): Promise<void> {
  if (!existsSync(BIN)) throw new Error(`${BIN} not found: run \`make build\` first`);
  const tmp = mkdtempSync(join(tmpdir(), "eenkah-shots-"));
  const files = join(tmp, "files");
  const env = { ...process.env, EENKAH_HOME: join(tmp, "data") };
  const eenkah = (...args: string[]) => execFileSync(BIN, args, { env, stdio: ["ignore", "ignore", "inherit"] });
  let server: ReturnType<typeof spawn> | null = null;
  try {
    const sick = execFileSync("cargo", ["run", "-q", "--release", "-p", "eenkah-testkit", "--example", "demo", "--", files, "400"], {
      cwd: ROOT,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
    })
      .trim()
      .split(" ");
    eenkah("set", "tz", "Europe/Berlin");
    eenkah("import", files);
    eenkah("mark", sick[0]!, "sick", "--through", sick[1]!);
    eenkah("train");

    server = spawn(BIN, ["serve", "--listen", `127.0.0.1:${PORT}`], { env, stdio: "ignore" });
    await waitForServer();

    const browser = await chromium.launch({ executablePath: findChrome() });
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2, colorScheme: "dark" });

    // hero: the Today page from the nav bar to the bottom of the HRV range chart, at desktop width
    await open(page, "/");
    const hrv = await page.locator(".card").filter({ has: page.locator(".panel-title", { hasText: "Is my HRV inside" }) }).first().boundingBox();
    if (!hrv) throw new Error("HRV range chart not found on the Today page");
    await page.screenshot({ path: join(OUT, "hero-today.png"), clip: { x: 0, y: 0, width: 1440, height: Math.ceil(hrv.y + hrv.height + 20) } });
    console.log("hero-today");

    // single graphs at a narrower width, so full-width charts are not too flat in the README
    await page.setViewportSize({ width: 1100, height: 900 });
    let at = "";
    for (const [path, title, name] of SHOTS) {
      if (path !== at) {
        await open(page, path);
        at = path;
      }
      const card = page.locator(".card").filter({ has: page.locator(".panel-title, h2, h3", { hasText: title }) }).first();
      // cards stretched by their grid row would carry empty space: size them to their content
      await card.evaluate((e: HTMLElement) => {
        if (!e.style.gridRow) {
          e.style.alignSelf = "start";
          e.style.height = "auto";
        }
      });
      await card.scrollIntoViewIfNeeded();
      await card.screenshot({ path: join(OUT, `${name}.png`) });
      console.log(name);
    }
    await browser.close();
  } finally {
    server?.kill();
    rmSync(tmp, { recursive: true, force: true });
  }
}

main().catch((e: unknown) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
