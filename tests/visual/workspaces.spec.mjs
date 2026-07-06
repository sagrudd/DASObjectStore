import { expect, test } from "@playwright/test";

const workspaces = [
  {
    id: "overview",
    title: "Overview",
    panels: [
      "Capacity",
      "Ingest Pressure",
      "Destage Urgency",
      "Endpoint State",
      "Required Actions"
    ]
  },
  {
    id: "disks",
    title: "Disks",
    panels: [
      "Enclosures",
      "Health",
      "USB and SMART Warnings",
      "Benchmark Drift",
      "Migrate, Drain, Replace, Retire"
    ]
  },
  {
    id: "stores",
    title: "Stores",
    panels: [
      "Policy Create and Modify",
      "Resize",
      "Redundancy",
      "Retention",
      "Export Mode",
      "Capacity Behavior"
    ]
  },
  {
    id: "objects",
    title: "Objects",
    panels: [
      "Inventory",
      "Hashes",
      "Copy Locations",
      "Reproducibility Source",
      "Export and Download",
      "Repair and Redownload"
    ]
  },
  {
    id: "endpoints",
    title: "Endpoints",
    panels: [
      "DAS Pools",
      "External NAS and NFS",
      "S3 Service State",
      "Mneion Export",
      "Binding Readiness"
    ]
  },
  {
    id: "activity",
    title: "Activity",
    panels: [
      "Ingest Queue",
      "Destage Queue",
      "Repair Tasks",
      "Audit and Provenance",
      "Long-Running Operations"
    ]
  }
];

const viewports = [
  { name: "desktop", width: 1280, height: 900 },
  { name: "mobile", width: 390, height: 844 }
];

for (const workspace of workspaces) {
  for (const viewport of viewports) {
    test(`${workspace.id} workspace renders at ${viewport.name} width`, async ({ page }, testInfo) => {
      await page.setViewportSize(viewport);
      await page.setContent(renderWorkspace(workspace), { waitUntil: "domcontentloaded" });

      await expect(page.locator("main")).toHaveAttribute("data-workspace", workspace.id);
      await expect(page.getByRole("heading", { name: workspace.title, level: 1 })).toBeVisible();

      for (const panel of workspace.panels) {
        await expect(page.getByRole("heading", { name: panel, level: 2 })).toBeVisible();
      }

      const screenshot = await page.locator("main").screenshot({
        animations: "disabled",
        path: testInfo.outputPath(`${workspace.id}-${viewport.name}.png`)
      });
      expect(screenshot.byteLength).toBeGreaterThan(10_000);

      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
      expect(overflow).toBe(false);

      const panelBoxes = await page.locator(".workspace-panel").evaluateAll((panels) =>
        panels.map((panel) => {
          const box = panel.getBoundingClientRect();
          return { height: box.height, width: box.width };
        })
      );

      for (const box of panelBoxes) {
        expect(box.height).toBeGreaterThan(70);
        expect(box.width).toBeGreaterThan(120);
      }
    });
  }
}

function renderWorkspace(workspace) {
  const nav = workspaces
    .map((item) => {
      const selected = item.id === workspace.id;
      return `<button class="nav-item" data-selected="${selected}" aria-pressed="${selected}">${item.title}</button>`;
    })
    .join("");
  const panels = workspace.panels
    .map(
      (panel) => `
        <section class="workspace-panel">
          <h2>${panel}</h2>
          <div class="panel-body">
            <span class="status-badge">Ready</span>
            <div class="capacity-track"><div class="capacity-fill"></div></div>
          </div>
        </section>`
    )
    .join("");

  return `
    <!doctype html>
    <html lang="en">
      <head>
        <meta charset="utf-8" />
        <title>DASObjectStore ${workspace.title}</title>
        <style>${styles()}</style>
      </head>
      <body>
        <main data-workspace="${workspace.id}">
          <nav aria-label="Operations workspaces">${nav}</nav>
          <section class="workspace-shell">
            <header class="workspace-header">
              <h1>${workspace.title}</h1>
            </header>
            <div class="workspace-grid">${panels}</div>
          </section>
        </main>
      </body>
    </html>`;
}

function styles() {
  return `
    * { box-sizing: border-box; }
    body {
      margin: 0;
      background: #f6f7f9;
      color: #1d2430;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      letter-spacing: 0;
    }
    main {
      min-height: 100vh;
      display: grid;
      grid-template-columns: 220px minmax(0, 1fr);
    }
    nav {
      background: #101820;
      color: #f6f7f9;
      display: flex;
      flex-direction: column;
      gap: 6px;
      padding: 14px;
    }
    .nav-item {
      border: 0;
      border-radius: 6px;
      background: transparent;
      color: inherit;
      min-height: 36px;
      padding: 0 10px;
      text-align: left;
      font: inherit;
    }
    .nav-item[data-selected="true"] {
      background: #2b6cb0;
    }
    .workspace-shell {
      padding: 22px;
      min-width: 0;
    }
    .workspace-header {
      border-bottom: 1px solid #d8dee7;
      margin-bottom: 18px;
      padding-bottom: 12px;
    }
    h1 {
      margin: 0;
      font-size: 26px;
      font-weight: 700;
    }
    .workspace-grid {
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
      gap: 12px;
    }
    .workspace-panel {
      min-height: 132px;
      border: 1px solid #d8dee7;
      border-radius: 8px;
      background: #ffffff;
      padding: 14px;
    }
    h2 {
      margin: 0 0 12px;
      font-size: 15px;
      font-weight: 700;
    }
    .panel-body {
      display: grid;
      gap: 12px;
    }
    .status-badge {
      width: fit-content;
      border-radius: 999px;
      background: #dff5e5;
      color: #155c2d;
      padding: 3px 8px;
      font-size: 12px;
      font-weight: 700;
    }
    .capacity-track {
      height: 10px;
      border-radius: 999px;
      background: #e6e9ee;
      overflow: hidden;
    }
    .capacity-fill {
      width: 42%;
      height: 100%;
      background: #2b6cb0;
    }
    @media (max-width: 640px) {
      main {
        grid-template-columns: 1fr;
      }
      nav {
        position: sticky;
        top: 0;
        z-index: 1;
        flex-direction: row;
        overflow-x: auto;
      }
      .nav-item {
        flex: 0 0 auto;
        white-space: nowrap;
      }
      .workspace-shell {
        padding: 16px;
      }
      .workspace-grid {
        grid-template-columns: minmax(0, 1fr);
      }
      h1 {
        font-size: 22px;
      }
    }
  `;
}
