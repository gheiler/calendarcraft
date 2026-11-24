// Takes the README screenshots from a served `trunk build` (default http://127.0.0.1:8096/).
// usage: NODE_PATH=<dir with puppeteer-core> node scripts/screenshots.cjs [baseUrl] [outDir]
const puppeteer = require('puppeteer-core');
const base = process.argv[2] || 'http://127.0.0.1:8096/';
const out = process.argv[3] || 'shots';
// Chrome's deviceScaleFactor emulation reports a wrong devicePixelContentBoxSize to
// eframe's resize observer, so phone shots use a second browser started at a real 2x scale.
const MOBILE = { width: 390, height: 844, deviceScaleFactor: 0, isMobile: true, hasTouch: true };
const shots = [
  ['month-light', 'view=month&theme=light'],
  ['month-dark', 'view=month&theme=dark'],
  ['quarter-light', 'view=quarter&theme=light'],
  ['quarter-dark', 'view=quarter&theme=dark'],
  ['year-light', 'view=year&theme=light'],
  ['year-dark', 'view=year&theme=dark'],
  ['week-light', 'view=week&theme=light'],
  ['inspector-light', 'view=week&theme=light&open=ev_4'],
  ['inspector-dark', 'view=month&theme=dark&open=ev_4'],
  ['new-event-light', 'view=month&theme=light&new=1'],
  ['new-event-dark', 'view=week&theme=dark&new=1'],
  ['mobile-light', 'theme=light', MOBILE],
  ['mobile-dark', 'theme=dark&view=month', MOBILE],
];
(async () => {
  const launch = (extra) => puppeteer.launch({
    executablePath: process.env.CHROME || '/usr/bin/google-chrome',
    headless: 'new',
    args: ['--no-sandbox', '--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--lang=en-US', ...extra],
  });
  const desktop = await launch([]);
  const phone = await launch(['--force-device-scale-factor=2']);
  require('fs').mkdirSync(out, { recursive: true });
  for (const [name, q, vp] of shots) {
    const page = await (vp === MOBILE ? phone : desktop).newPage();
    page.on('pageerror', (e) => console.error(name, 'pageerror', e.message));
    page.on('console', (m) => { if (m.type() === 'error') console.error(name, m.text()); });
    await page.setViewport(vp || { width: 1440, height: 900, deviceScaleFactor: 1 });
    await page.goto(`${base}?demo=1&date=2026-11-24&lang=en&${q}`, { waitUntil: 'networkidle0' });
    await new Promise((r) => setTimeout(r, 3500));
    await page.screenshot({ path: `${out}/${name}.png` });
    console.log('saved', `${out}/${name}.png`);
    await page.close();
  }
  await desktop.close();
  await phone.close();
})().catch((e) => { console.error(e); process.exit(1); });
