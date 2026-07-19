#!/usr/bin/env node

import { createServer } from "node:http";
import { mkdir, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { extname, join, normalize, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { chromium } from "playwright";
import { authenticatedPages, roles, viewports } from "./web-screenshot-fixtures.mjs";

const execFileAsync = promisify(execFile);
const repoRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const distDir = join(repoRoot, "crates", "dasobjectstore-gui-web", "dist");
const artifactDir = join(repoRoot, "target", "web-screenshots");
const publicBase = "/products/dasobjectstore/";
const apiBase = "/products/dasobjectstore/api";
const apiV1Base = "/products/dasobjectstore/api/v1";
let visualEndpoint = null;
let visualVersion = null;
let visualLiveSequence = 41;

async function main() {
  visualVersion = await workspaceVersion();
  await buildWebDist();
  await mkdir(artifactDir, { recursive: true });
  const server = await startServer();
  const baseUrl = `http://127.0.0.1:${server.address().port}${publicBase}`;
  const browser = await chromium.launch();

  try {
    for (const viewport of viewports) {
      await captureLogin(browser, baseUrl, viewport);
      for (const role of roles) {
        await captureAuthenticatedPages(browser, baseUrl, viewport, role);
      }
    }
  } finally {
    await browser.close();
    await new Promise((resolveClose) => server.close(resolveClose));
  }

  console.log(`web screenshot regression artifacts: ${artifactDir}`);
}

async function workspaceVersion() {
  const manifest = await readFile(join(repoRoot, "Cargo.toml"), "utf8");
  const match = manifest.match(/^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error("Unable to read the Rust workspace version from Cargo.toml");
  }
  return match[1];
}

async function buildWebDist() {
  await execFileAsync("bash", ["packaging/web/prepare-web-dist.sh"], {
    cwd: repoRoot,
    env: { ...process.env, NO_COLOR: "1" },
    maxBuffer: 20 * 1024 * 1024,
  });
  if (!existsSync(join(distDir, "index.html"))) {
    throw new Error(`Trunk build did not produce ${join(distDir, "index.html")}`);
  }
}

async function captureLogin(browser, baseUrl, viewport) {
  const context = await browser.newContext({ viewport });
  const page = await context.newPage();
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.locator(".dos-auth-shell").waitFor();
  await assertVisualContract(page, { auth: false });
  await page.screenshot({
    path: join(artifactDir, `${viewport.name}-login.png`),
    fullPage: true,
  });
  await context.close();
}

async function captureAuthenticatedPages(browser, baseUrl, viewport, role) {
  const context = await browser.newContext({ viewport });
  await context.addInitScript((session) => {
    window.localStorage.setItem("dasobjectstore.username", session.username);
    window.localStorage.setItem("dasobjectstore.session_token", session.token);
  }, role);
  const page = await context.newPage();
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.locator(".dos-topbar").waitFor();

  for (const pageSpec of authenticatedPages) {
    await page.locator(pageSpec.selector).click();
    await page.locator(pageSpec.pageSelector).waitFor();
    await page.locator(pageSpec.readySelector).waitFor();
    await page.waitForLoadState("networkidle");
    await assertVisualContract(page, { auth: true });
    if (viewport.name === "desktop") {
      await assertWorkflowContract(page, pageSpec.name, role);
    }
    await page.screenshot({
      path: join(artifactDir, `${viewport.name}-${role.name}-${pageSpec.name}.png`),
      fullPage: true,
    });
  }

  await context.close();
}

async function assertVisualContract(page, { auth }) {
  await page.locator(".dos-product-footer").waitFor();
  await page.locator(".dos-product-footer__version").waitFor({ state: "attached" });
  await page.getByRole("link", { name: "Mnemosyne Biosciences" }).waitFor();

  if (auth) {
    await page.locator(".dos-topbar").waitFor();
    await page.getByRole("navigation", { name: "Primary" }).waitFor();
  }

  const failures = await page.evaluate(({ auth: authenticated }) => {
    const epsilon = 1;
    const visible = (element) => {
      const style = window.getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return (
        style.display !== "none" &&
        style.visibility !== "hidden" &&
        rect.width > 1 &&
        rect.height > 1
      );
    };
    const rectOf = (element) => {
      const rect = element.getBoundingClientRect();
      return {
        label:
          element.getAttribute("data-page") ||
          element.getAttribute("data-store-id") ||
          element.getAttribute("data-enclosure-id") ||
          element.className ||
          element.tagName,
        left: rect.left,
        right: rect.right,
        top: rect.top,
        bottom: rect.bottom,
        width: rect.width,
        height: rect.height,
      };
    };
    const intersects = (a, b) =>
      a.left < b.right - epsilon &&
      a.right > b.left + epsilon &&
      a.top < b.bottom - epsilon &&
      a.bottom > b.top + epsilon;

    const issues = [];
    const bodyText = document.body.innerText || "";
    if (!bodyText.includes("DASObjectStore")) {
      issues.push("body text does not include DASObjectStore");
    }
    const footer = document.querySelector(".dos-product-footer");
    if (!footer || !visible(footer)) {
      issues.push("footer is not visible");
    } else {
      const footerStyle = window.getComputedStyle(footer);
      if (footerStyle.fontFamily.toLowerCase().includes("mono")) {
        issues.push(`footer must use the report-style sans-serif stack: ${footerStyle.fontFamily}`);
      }
      if (footerStyle.backgroundColor === "rgba(0, 0, 0, 0)") {
        issues.push("footer background is transparent");
      }
    }

    if (authenticated && !document.querySelector(".dos-topbar")) {
      issues.push("authenticated view is missing the top bar");
    }

    const brandLogoSelector = authenticated ? ".dos-brand-logo" : ".dos-auth-wordmark";
    const brandLogos = Array.from(document.querySelectorAll(brandLogoSelector));
    if (brandLogos.length === 0) {
      issues.push(
        authenticated
          ? "Mnemosyne compact brand logo is missing"
          : "Mnemosyne login wordmark is missing",
      );
    }
    for (const logo of brandLogos) {
      const rect = logo.getBoundingClientRect();
      if (!visible(logo)) {
        issues.push("Mnemosyne brand asset is not visible");
      }
      const minWidth = authenticated ? 10 : 160;
      const minHeight = authenticated ? 18 : 100;
      if (rect.width < minWidth || rect.height < minHeight) {
        issues.push(`Mnemosyne brand asset renders too small: ${rect.width}x${rect.height}`);
      }
    }

    const layoutElements = Array.from(
      document.querySelectorAll(".dos-topbar, .dos-page-header, .dos-card, .dos-product-footer"),
    ).filter(visible);
    const rects = layoutElements.map(rectOf);
    for (let i = 0; i < rects.length; i += 1) {
      for (let j = i + 1; j < rects.length; j += 1) {
        if (intersects(rects[i], rects[j])) {
          issues.push(`overlap: ${rects[i].label} intersects ${rects[j].label}`);
        }
      }
    }

    return issues;
  }, { auth });

  if (failures.length > 0) {
    throw new Error(`visual contract failed for ${page.url()}:\n${failures.join("\n")}`);
  }
}

async function assertWorkflowContract(page, pageName, role) {
  switch (pageName) {
    case "enclosures":
      await assertEnclosureWorkflow(page, role);
      break;
    case "live-status":
      await assertLiveStatusWorkflow(page);
      break;
    case "objectstores":
      await assertObjectStoreWorkflow(page, role);
      break;
    case "users-groups":
      await assertUsersGroupsWorkflow(page, role);
      break;
    case "endpoints":
      await assertEndpointsWorkflow(page, role);
      break;
    case "activity":
      await assertActivityWorkflow(page);
      break;
    case "bioinformatics":
      await assertBioinformaticsWorkflow(page);
      break;
  }
}

async function assertEnclosureWorkflow(page, role) {
  const registry = page.locator(".dos-enclosures-table");
  await registry.waitFor();
  const addButton = page.getByRole("button", { name: "Add enclosure", exact: true });

  if (!role.administrator) {
    await expectDisabled(addButton, "non-admin enclosure preparation must be disabled");
    await registry.getByRole("button", { name: "Open" }).first().click();
    const detailPane = page.locator(".dos-task-pane[role='dialog']");
    await detailPane.getByText("Hardware").waitFor();
    await page.waitForTimeout(100);
    await page.screenshot({
      path: join(artifactDir, "desktop-viewer-enclosures-detail.png"),
      fullPage: false,
    });
    await detailPane.getByRole("button", { name: "Close task pane" }).click();
    return;
  }

  await expectEnabled(addButton, "admin enclosure preparation must be enabled");
  await addButton.click();
  const pane = page.locator(".dos-task-pane[role='dialog']");
  await pane.locator("[data-workflow='enclosure_add']").waitFor();
  await pane.getByText("SSD landing device").waitFor();
  await page.screenshot({
    path: join(artifactDir, "desktop-admin-enclosures-prepare.png"),
    fullPage: false,
  });
  await pane.getByLabel("I allow formatting of the selected devices.").check();
  await pane.getByLabel("I acknowledge existing data on selected devices may be destroyed.").check();
  await pane.getByPlaceholder("confirm prepare das").fill("confirm prepare das");
  await pane.getByRole("button", { name: "Submit preparation job" }).click();
  await pane.getByText("Job enclosure-prepare-visual").waitFor();
  await pane.getByRole("button", { name: "Close task pane" }).click();
  await page.locator(".dos-task-pane").waitFor({ state: "detached" });
  await page.waitForTimeout(100);
}

async function assertObjectStoreWorkflow(page, role) {
  const registry = page.locator(".dos-objectstores-table");
  await registry.waitFor();
  const capacitySort = registry.getByRole("button", { name: "Sort by Capacity, descending" });
  await capacitySort.click();
  const capacityHeader = registry.getByRole("columnheader", { name: /Capacity/ });
  if ((await capacityHeader.getAttribute("aria-sort")) !== "descending") {
    throw new Error("Capacity sort must expose descending aria-sort state");
  }
  const descendingRows = await registry.locator("tbody tr[data-store-id]").evaluateAll((rows) =>
    rows.map((row) => row.getAttribute("data-store-id")),
  );
  if (descendingRows.join(",") !== "epic-collection,zymo-fecal-2025-05,cold-archive") {
    throw new Error(`unexpected descending capacity order: ${descendingRows.join(",")}`);
  }
  await registry.getByRole("button", { name: "Sort by Capacity, ascending" }).click();
  const createButton = page.getByRole("button", { name: "Create ObjectStore", exact: true });

  const zymoRow = registry.locator("tr[data-store-id='zymo-fecal-2025-05']");
  const browseButton = zymoRow.getByRole("button", { name: /Browse objects in/ });
  await browseButton.click();
  let browserPane = page.locator(".dos-task-pane[role='dialog']");
  await browserPane.locator(".dos-object-browser:not([data-state='loading'])").waitFor();
  await browserPane.getByText("Browse objects", { exact: true }).waitFor();
  await page.screenshot({
    path: join(artifactDir, `desktop-${role.name}-objectstores-browser.png`),
    fullPage: false,
  });
  if (await browserPane.getByRole("columnheader", { name: "Placement", exact: true }).count()) {
    throw new Error("default ObjectStore file rows must not expose placement diagnostics");
  }
  const objectDetails = browserPane.locator(".dos-object-browser-object-details summary");
  await objectDetails.click();
  await browserPane.locator(".dos-object-browser-object-details__body").waitFor();
  await page.screenshot({
    path: join(artifactDir, `desktop-${role.name}-objectstores-browser-details.png`),
    fullPage: false,
  });
  if (await browserPane.getByLabel("Endpoint").count() !== 0) {
    throw new Error("row-scoped ObjectStore browser must not expose a second endpoint selector");
  }
  if (await page.locator(".dos-objectstores-registry > .dos-object-browser").count() !== 0) {
    throw new Error("ObjectStore browser must not be appended below the registry");
  }
  await browserPane.getByRole("button", { name: "Close task pane" }).click();
  await browserPane.waitFor({ state: "detached" });

  if (!role.administrator) {
    await expectDisabled(createButton, "non-admin ObjectStore creation must be disabled");
    if (await page.locator(".dos-task-pane").count() !== 0) {
      throw new Error("ObjectStores task pane must be closed for viewers");
    }
    return;
  }

  await expectEnabled(createButton, "admin ObjectStore creation must be enabled");
  await createButton.click();
  let pane = page.locator(".dos-task-pane[role='dialog']");
  await pane.waitFor();
  await pane.getByLabel("Store name").fill("visual-e2e-store");
  await pane.getByLabel("Writer group").selectOption("bioinformatics");
  await pane.getByLabel("Enclosure").selectOption("qnap-tl-d800c-visual");
  await pane.getByRole("button", { name: "Review daemon plan" }).click();
  await pane.getByText("dasobjectstore store create visual-e2e-store").waitFor();
  await pane.getByPlaceholder("confirm create objectstore").fill("confirm create objectstore");
  await pane.getByRole("button", { name: "Submit daemon job" }).click();
  await pane.getByText("ObjectStore creation submitted to dasobjectstored.").waitFor();
  await pane.getByRole("button", { name: "Close task pane" }).click();

  await registry.getByRole("button", { name: "Open" }).first().click();
  pane = page.locator(".dos-task-pane[role='dialog']");
  await pane.getByText("Overview").waitFor();
  const subobjectButton = pane.getByRole("button", { name: "Create SubObject" });
  await expectEnabled(subobjectButton, "admin SubObject creation must be enabled");
  await subobjectButton.click();
  await pane.getByLabel("SubObject name").fill("pod5/raw");
  await pane.getByRole("button", { name: "Review SubObject plan" }).click();
  await pane.getByText("dasobjectstore subobject create pod5/raw").waitFor();
  await pane.getByRole("button", { name: "Close task pane" }).click();
}

async function assertUsersGroupsWorkflow(page, role) {
  const inventory = page.locator("[data-section='users-inventory']");
  await inventory.waitFor();
  const addButton = page.getByRole("button", { name: "Add user", exact: true });
  const groupsButton = page.getByRole("button", { name: "Manage groups", exact: true });

  if (!role.administrator) {
    await expectDisabled(addButton, "non-admin Add user must be disabled");
    await expectDisabled(groupsButton, "non-admin group management must be disabled");
    if (await page.locator(".dos-task-pane").count() !== 0) {
      throw new Error("Local Access task pane must be closed for viewers");
    }
    return;
  }

  await expectEnabled(addButton, "admin Add user must be enabled");
  await addButton.click();
  const pane = page.locator(".dos-task-pane[role='dialog']");
  await pane.waitFor();
  for (const step of ["identify-user", "qualification", "groups", "review"]) {
    await pane.locator(`[data-step='${step}']`).waitFor();
  }
  await pane.getByLabel("OS-recognized/local user").selectOption({ label: role.username });
  await pane.getByLabel("I confirm this existing local user is qualified for DASObjectStore access.").check();
  await pane.getByLabel(/Bioinformatics \(bioinformatics\)/).check();
  await pane.getByRole("button", { name: "Review and apply" }).click();
  await pane.getByText(/Job local-group-assign-apply-visual/).waitFor();
  // The focused Rust/Yew component contract covers Escape handling. This
  // visual runner exercises the rendered closed-state through the explicit
  // close control so the following workflow page is not intercepted.
  await pane.getByRole("button", { name: "Close" }).click();
  await expectHidden(pane, "closing the Local Access task pane must hide it");
  await expectEnabled(groupsButton, "admin group management must be enabled");
  await groupsButton.click();
  const groupPane = page.getByRole("dialog");
  await groupPane.getByText("Create access group").waitFor();
  await groupPane.getByRole("button", { name: "Close" }).click();
  await expectHidden(groupPane, "closing the access-group task pane must hide it");
}

async function assertEndpointsWorkflow(page, role) {
  const inventory = page.locator("[data-section='endpoint-inventory']");
  await inventory.waitFor();
  const addButton = page.getByRole("button", { name: "Add connection", exact: true });
  if (!role.administrator) {
    await expectEnabled(addButton, "endpoint inventory remains visible to viewers");
    await addButton.click();
    await page.getByRole("dialog").waitFor();
    await page.keyboard.press("Escape");
    return;
  }

  await addButton.click();
  const pane = page.locator(".dos-task-pane[role='dialog']");
  await pane.waitFor();
  await pane.getByLabel("Connection ID").fill("visual-endpoint");
  await pane.getByLabel("Display name").fill("Visual connection");
  await pane.getByLabel("NAS / NFS gateway URL").fill("https://endpoint.example.test:9443");
  if (await pane.getByLabel("Make an ObjectStore available through this connection.").count()) {
    await pane.getByLabel("Make an ObjectStore available through this connection.").check();
    await pane.locator("[data-section='endpoint-binding'] select").selectOption("zymo-fecal-2025-05");
  }
  if (await pane.getByLabel("Confirmation phrase").count() !== 0) {
    throw new Error("dry-run endpoint review must not show a confirmation phrase");
  }
  await pane.getByLabel("Dry run only").uncheck();
  await pane.getByLabel("Confirmation phrase").fill("record endpoint inventory");
  await pane.getByRole("button", { name: "Record endpoint" }).click();
  await expectHidden(pane, "successful endpoint update must close the task pane");
  await inventory.getByText("Visual connection").waitFor();
  await inventory.getByRole("button", { name: "Open details for Visual connection" }).click();
  const detailPane = page.getByRole("dialog");
  await detailPane.getByText("Technical details").waitFor();
  await detailPane.getByRole("button", { name: "Edit connection" }).click();
  await detailPane.getByLabel("Connection ID").waitFor();
  await detailPane.getByRole("button", { name: "Close task pane" }).click();
  await expectHidden(detailPane, "endpoint edit pane must close before navigating away");
}

async function assertActivityWorkflow(page) {
  await page.locator("[data-panel='reporting']").waitFor();
  await page.getByText("Rebuild performance report").waitFor();
  await page.getByText("Drop benchmarking JSON here").waitFor();
  await page.getByText("Administrator jobs", { exact: true }).waitFor();
  await page.getByText("Enclosure preparation", { exact: true }).waitFor();
  await page.getByText("ObjectStore creation", { exact: true }).waitFor();
  await page.getByText("SubObject creation", { exact: true }).waitFor();
  await page.getByText("Create local writer group").waitFor();
  await page.getByText("Ingest zymo_fecal_2025.05").waitFor();
}

async function assertLiveStatusWorkflow(page) {
  await page.getByText("Host → ObjectStore").waitFor();
  await page.getByText("SSD ingress", { exact: true }).first().waitFor();
  await page.getByText("HDD settlement", { exact: true }).first().waitFor();
  await page.getByText("stephen-NUC12DCMi9", { exact: true }).first().waitFor();
  await page.waitForTimeout(2_200);
  const traces = page.locator(".dos-live-trace__line");
  if (await traces.count() !== 2) {
    throw new Error("live status must render both SSD and HDD throughput traces");
  }
  for (let index = 0; index < 2; index += 1) {
    const points = (await traces.nth(index).getAttribute("points") ?? "").trim().split(/\s+/);
    if (points.length < 2) {
      throw new Error("live status traces must retain more than one sequenced sample");
    }
  }
  await page.getByText("HDD copy 1", { exact: true }).waitFor();
  await page.locator(".dos-live-path").first().click();
  await page.getByRole("complementary", { name: "Transfer detail" }).waitFor();
}

async function assertBioinformaticsWorkflow(page) {
  await page.locator("[data-object-type='POD5']").first().waitFor();
  await page.getByText("Sequencing run provenance").waitFor();
  await page
    .locator("[data-source-kind='ObjectStore']")
    .getByText("ObjectStore, SubObject, object-type, and Mneion source records")
    .waitFor();
}

async function expectEnabled(locator, message) {
  if (!(await locator.isEnabled())) {
    throw new Error(message);
  }
}

async function expectDisabled(locator, message) {
  if (await locator.isEnabled()) {
    throw new Error(message);
  }
}

async function expectHidden(locator, message) {
  try {
    await locator.waitFor({ state: "hidden", timeout: 10_000 });
  } catch (error) {
    throw new Error(message, { cause: error });
  }
}

function startServer() {
  const server = createServer(async (request, response) => {
    try {
      await handleRequest(request, response);
    } catch (error) {
      response.writeHead(500, { "content-type": "text/plain; charset=utf-8" });
      response.end(String(error?.stack || error));
    }
  });

  return new Promise((resolveListen) => {
    server.listen(0, "127.0.0.1", () => resolveListen(server));
  });
}

async function handleRequest(request, response) {
  const url = new URL(request.url, "http://127.0.0.1");
  if (url.pathname.startsWith(apiBase)) {
    const body = await readJsonBody(request);
    response.writeHead(200, { "content-type": "application/json; charset=utf-8" });
    response.end(JSON.stringify(apiResponse(url.pathname, request.method, request, body)));
    return;
  }

  let assetPath = url.pathname;
  if (assetPath === publicBase || assetPath === publicBase.slice(0, -1)) {
    assetPath = `${publicBase}index.html`;
  }
  if (!assetPath.startsWith(publicBase)) {
    response.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    response.end("not found");
    return;
  }

  const relativeAsset = assetPath.slice(publicBase.length);
  const filePath = normalize(join(distDir, relativeAsset));
  if (!relative(distDir, filePath).startsWith("..") && existsSync(filePath)) {
    response.writeHead(200, { "content-type": contentType(filePath) });
    response.end(await readFile(filePath));
    return;
  }

  response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  response.end(await readFile(join(distDir, "index.html")));
}

async function readJsonBody(request) {
  if (!["POST", "PUT", "PATCH"].includes(request.method || "")) {
    return {};
  }
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(chunk);
  }
  const raw = Buffer.concat(chunks).toString("utf8").trim();
  if (!raw) {
    return {};
  }
  try {
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

function contentType(filePath) {
  switch (extname(filePath)) {
    case ".css":
      return "text/css; charset=utf-8";
    case ".html":
      return "text/html; charset=utf-8";
    case ".js":
      return "text/javascript; charset=utf-8";
    case ".wasm":
      return "application/wasm";
    case ".png":
      return "image/png";
    case ".svg":
      return "image/svg+xml";
    default:
      return "application/octet-stream";
  }
}

function apiResponse(pathname, method, request, body = {}) {
  const role = roleFromRequest(request);
  if (pathname === `${apiBase}/session` && method === "POST") {
    return {
      username: role.username,
      valid: true,
      expires_at_unix_seconds: 1_803_988_800,
    };
  }
  if (pathname === `${apiBase}/login` && method === "POST") {
    return {
      username: roles[1].username,
      session_token: roles[1].token,
      expires_at_unix_seconds: 1_803_988_800,
    };
  }
  if (pathname === `${apiBase}/logout` && method === "POST") {
    return { username: role.username, disconnected: true };
  }
  if (pathname === `${apiV1Base}/health` && method === "GET") {
    return {
      service: "dasobjectstore-gui-web",
      status: "ready",
      version: visualVersion,
      instance_id: "visual-instance",
    };
  }
  if (pathname === `${apiV1Base}/dashboard/home`) {
    return homeDashboard();
  }
  if (pathname === `${apiV1Base}/dashboard/enclosures`) {
    return enclosuresDashboard(role);
  }
  if (pathname === `${apiV1Base}/dashboard/object-stores`) {
    return objectStoresDashboard(role);
  }
  if (pathname.startsWith(`${apiV1Base}/object-stores/`) && pathname.endsWith("/browser")) {
    return objectBrowserResponse(pathname);
  }
  if (pathname === `${apiV1Base}/workspaces/activity`) {
    return activityWorkspace();
  }
  if (pathname === `${apiV1Base}/workspaces/live-status`) {
    return liveStatusWorkspace();
  }
  if (pathname === `${apiV1Base}/workspaces/endpoints` && method === "GET") {
    return endpointsWorkspace();
  }
  if (pathname === `${apiV1Base}/workspaces/users-groups`) {
    return usersGroupsWorkspace(role);
  }
  if (pathname === `${apiV1Base}/workspaces/bioinformatics`) {
    return bioinformaticsWorkspace();
  }
  if (pathname === `${apiV1Base}/actions/plan` && method === "POST") {
    return actionPlanResponse(body);
  }
  if (pathname === `${apiV1Base}/workspaces/enclosures/prepare` && method === "POST") {
    return enclosurePrepareResponse(role);
  }
  if (pathname === `${apiV1Base}/workspaces/object-stores/create` && method === "POST") {
    return objectStoreCreateResponse(role);
  }
  if (pathname === `${apiV1Base}/workspaces/users-groups/local-groups` && method === "POST") {
    return localGroupAdminResponse("create_local_group", body.group_name || "mnemosyne-writers", null, body);
  }
  if (pathname === `${apiV1Base}/workspaces/users-groups/local-groups/members` && method === "POST") {
    return localGroupAdminResponse("assign_local_user_to_group", body.group_name || "bioinformatics", body.username || "stephen", body);
  }
  if (pathname === `${apiV1Base}/workspaces/endpoints/upsert` && method === "POST") {
    return endpointInventoryUpsertResponse(body);
  }
  if (pathname.startsWith(`${apiV1Base}/workspaces/admin/jobs/`)) {
    return adminJobStatusResponse(pathname);
  }
  return {};
}

function roleFromRequest(request) {
  const token = request.headers["x-dasobjectstore-session-token"] || "";
  return roles.find((role) => role.token === token) || roles[1];
}

function capacity(total, used, free, usedPercentBasisPoints) {
  return {
    total_tib: total,
    used_tib: used,
    free_tib: free,
    used_percent_basis_points: usedPercentBasisPoints,
  };
}

function objectBrowserResponse(pathname) {
  const segments = pathname.split("/").filter(Boolean);
  const endpoint = decodeURIComponent(segments.at(-2));
  return {
    endpoint,
    prefix: "",
    breadcrumbs: [],
    folders: [
      {
        name: "pod5",
        prefix: "pod5/",
        object_count: 128,
        total_size_bytes: 68719476736,
        readiness: "available",
      },
      {
        name: "reports",
        prefix: "reports/",
        object_count: 12,
        total_size_bytes: 33554432,
        readiness: "available",
      },
    ],
    files: [
      {
        object_id: "visual-object-001",
        name: "manifest.json",
        path: "manifest.json",
        object_type: "application/json",
        size_bytes: 18432,
        modified_at_utc: "2026-07-17T12:30:00Z",
        checksum: {
          algorithm: "sha256",
          value: "d1b6c6d8e9024a627f4ecdb580e897c78b87e87c8b943175528208a9a12b7aa1",
          verified_at_utc: "2026-07-17T12:31:00Z",
        },
        readiness: "available",
        lifecycle_state: "settled",
        copy_count: 2,
        placements: [
          {
            disk_id: "qnap-1057",
            disk_label: "QNAP bay 1",
            location: "hdd_settled",
            state: "verified",
            size_bytes: 18432,
            checksum: null,
            verified_at_utc: "2026-07-17T12:31:00Z",
          },
          {
            disk_id: "qnap-1058",
            disk_label: "QNAP bay 2",
            location: "hdd_settled",
            state: "verified",
            size_bytes: 18432,
            checksum: null,
            verified_at_utc: "2026-07-17T12:31:00Z",
          },
        ],
        download_source: "hdd_settled",
      },
    ],
    next_cursor: null,
    total_entries: 3,
  };
}

function driveCount(total, mounted, healthy = mounted, watch = 0) {
  return {
    total,
    mounted,
    healthy,
    watch,
    suspect: 0,
    failed: 0,
  };
}

function enclosureCard() {
  return {
    enclosure_id: "qnap-tl-d800c-visual",
    display_name: "QNAP TL-D800C",
    mount_path: "/srv/dasobjectstore",
    connection: { bus: "USB", protocol: "USB 3.2", link_speed: "10 Gb/s" },
    health: "ready",
    drive_count: driveCount(8, 8, 7, 1),
    capacity: capacity("126.0", "42.0", "84.0", 3333),
    last_seen_at_utc: "2026-07-08T19:00:00Z",
    warnings: [{ code: "smart_watch", message: "One HDD reports SMART watch state." }],
  };
}

function homeDashboard() {
  return {
    schema_version: "dasobjectstore.web_redesign.v1",
    generated_at_utc: "2026-07-08T19:00:00Z",
    health: {
      state: "ready",
      label: "Operational",
      warning_count: 1,
      critical_count: 0,
      action_count: 1,
      last_checked_at_utc: "2026-07-08T19:00:00Z",
    },
    drives: driveCount(8, 8, 7, 1),
    capacity: capacity("126.0", "42.0", "84.0", 3333),
    mounted_enclosures: [enclosureCard()],
    throughput_7d: {
      window_days: 0,
      read_tib: "14.2",
      written_tib: "18.8",
      ingest_tib: "12.6",
      avg_read_mib_s: 420,
      avg_write_mib_s: 310,
      source: "daemon_disk_io",
      message: null,
      daily: [
        { date: "18:00", read_tib: "0.01", written_tib: "0.02", ingest_tib: "0.02" },
        { date: "18:05", read_tib: "0.01", written_tib: "0.04", ingest_tib: "0.04" },
        { date: "18:10", read_tib: "0.02", written_tib: "0.06", ingest_tib: "0.06" },
        { date: "18:15", read_tib: "0.01", written_tib: "0.05", ingest_tib: "0.05" },
      ],
    },
    ingest: { pressure: "normal", queued_jobs: 2, active_jobs: 1, failed_jobs: 0, warnings: [] },
    destage: { pending_objects: 12, copying_objects: 2, verified_objects: 950, warnings: [] },
    object_service: {
      active: true,
      remote_ready: true,
      bind_address: "127.0.0.1",
      port: 3900,
      local_url: "http://127.0.0.1:3900",
      remote_url: null,
      service_state: "ready",
      message: null,
    },
    memory_stress: {
      state: "normal",
      pressure_percent: 42,
      swap_used_percent: 0,
      page_cache_tib: "0.8",
      warning: null,
    },
    smart_warnings: {
      warning_count: 1,
      affected_drive_count: 1,
      warnings: [
        {
          drive_id: "qnap-1059",
          severity: "watch",
          attribute: "reallocated_sector_ct",
          message: "SMART watch threshold is active.",
        },
      ],
    },
    object_stores: objectStoresDashboard(roles[1]).stores,
  };
}

function enclosuresDashboard(role = roles[1]) {
  const canAdmin = role.administrator;
  return {
    schema_version: "dasobjectstore.enclosures_page.v1",
    generated_at_utc: "2026-07-08T19:00:00Z",
    add_enclosure: {
      enabled: canAdmin,
      action_kind: "enclosure_add",
      label: "Add enclosure",
      state: canAdmin ? "ready" : "admin_required",
      administrator: canAdmin,
      supported_enclosure_detected: true,
      daemon_ready: true,
      confirmation_required: true,
      blocked_reason: canAdmin ? null : "Requires sudo-derived administrator authority.",
      next_step: "Review the selected SSD/HDD format plan before daemon execution.",
    },
    enclosures: [enclosureCard()],
    selected_enclosure_id: "qnap-tl-d800c-visual",
    details: {
      enclosure_id: "qnap-tl-d800c-visual",
      vendor: "QNAP",
      model: "TL-D800C",
      serial: "VISUAL-QNAP-001",
      firmware: "1.0.0",
      slots: [
        driveSlot(0, "nvme-landing", "ssd", "3.6", "ready"),
        driveSlot(1, "qnap-1057", "hdd", "18.0", "ready"),
        driveSlot(2, "qnap-1058", "hdd", "18.0", "ready"),
        driveSlot(3, "qnap-1059", "hdd", "18.0", "watch"),
      ],
    },
    warnings: [],
  };
}

function driveSlot(slotNumber, driveId, role, sizeTib, health) {
  return {
    slot_number: slotNumber,
    drive_id: driveId,
    role,
    mount_path: slotNumber === 0 ? "/srv/dasobjectstore/ssd" : `/srv/dasobjectstore/hdd/${driveId}`,
    device_path: slotNumber === 0 ? "/dev/nvme0n1" : `/dev/sd${String.fromCharCode(96 + slotNumber)}`,
    filesystem: "xfs",
    size_tib: sizeTib,
    health,
    mounted: true,
    smart_warning_count: health === "watch" ? 1 : 0,
    actions_available: ["inspect"],
  };
}

function objectStoresDashboard(role = roles[1]) {
  const canAdmin = role.administrator;
  return {
    schema_version: "dasobjectstore.objectstores_page.v1",
    generated_at_utc: "2026-07-08T19:00:00Z",
    groups_file_path: "/opt/dasobjectstore/groups.json",
    mounted_enclosures: [enclosureCard()],
    groups: [
      {
        group_name: "bioinformatics",
        display_name: "Bioinformatics",
        source: "local",
        current_user_member: true,
      },
    ],
    stores: [
      visualObjectStore({
        storeId: "zymo-fecal-2025-05",
        displayName: "zymo_fecal_2025.05",
        objectCount: 245,
        usedTib: "2.3",
        lastIngested: "2026-07-08T18:50:00Z",
      }),
      visualObjectStore({
        storeId: "epic-collection",
        displayName: "epic_collection",
        objectCount: 9,
        usedTib: "12.5",
        lastIngested: "2026-07-12T09:30:00Z",
      }),
      visualObjectStore({
        storeId: "cold-archive",
        displayName: "cold_archive",
        objectCount: 100,
        usedTib: null,
        lastIngested: null,
      }),
    ],
    selected_store_id: "zymo-fecal-2025-05",
    create_object_store: {
      enabled: canAdmin,
      action_kind: "store_create",
      label: "Create ObjectStore",
      required_fields: [
        { name: "store_name", label: "Store name" },
        { name: "writer_group", label: "Writer group" },
      ],
      optional_fields: [{ name: "object_type", label: "Object type" }],
      defaults: { store_class: "research", required_copies: 1, endpoint_export_mode: "s3" },
      store_class_options: [
        { value: "research", label: "Research", description: "General research data." },
      ],
      copy_count_options: [1, 2, 3],
      confirmation_required: true,
      blocked_reason: canAdmin
        ? null
        : "Administrator rights are required to create an ObjectStore.",
    },
    warnings: [],
  };
}

function visualObjectStore({ storeId, displayName, objectCount, usedTib, lastIngested }) {
  return {
    store_id: storeId,
    display_name: displayName,
    store_class: "research",
    object_type: "POD5",
    health: "ready",
    required_copies: 1,
    object_count: objectCount,
    capacity: usedTib === null ? null : capacity("42.0", usedTib, "29.5", 2976),
    placement_policy: "fractional free-space placement",
    endpoint_export_mode: "s3",
    writer_group: "bioinformatics",
    public: false,
    writeable: true,
    created_at_utc: "2026-07-08T12:00:00Z",
    last_ingested_at_utc: lastIngested,
    writer_policy: {
      writer_group: "bioinformatics",
      group_defined: true,
      current_user_member: true,
      writeable_by_current_user: true,
      state: "ready",
      message: "Current user can write through the bioinformatics group.",
    },
    warnings: [],
  };
}

function activityWorkspace() {
  return {
    ingest: { pressure: "normal", queued_jobs: 2, active_jobs: 1, failed_jobs: 0, warnings: [] },
    destage: { pending_objects: 12, copying_objects: 2, verified_objects: 950, warnings: [] },
    categories: [
      activityCategory(
        "system_administration",
        "Administrator jobs",
        "Privileged appliance and local group administration submitted to the daemon.",
      ),
      activityCategory(
        "enclosure_preparation",
        "Enclosure preparation",
        "Supported DAS detection and media preparation jobs.",
      ),
      activityCategory(
        "object_store_creation",
        "ObjectStore creation",
        "Daemon-owned ObjectStore creation and policy materialization.",
      ),
      activityCategory(
        "sub_object_creation",
        "SubObject creation",
        "Folder-level and nested object routing registrations.",
      ),
      activityCategory("ingest", "Ingest", "SSD-first upload jobs and queue pressure."),
      activityCategory("destage", "Destage", "SSD-to-HDD settlement and verification."),
      activityCategory("repair", "Repair", "Repair and redundancy restoration work."),
      activityCategory(
        "endpoint_validation",
        "Endpoint validation",
        "Object-service, S3, NAS/NFS, and Mnemosyne endpoint checks.",
      ),
    ],
    tasks: [
      {
        task_id: "admin-job-visual-1",
        kind: "system_administration",
        state: "running",
        label: "Create local writer group",
        updated_at_utc: "2026-07-08T19:05:00Z",
        warnings: [],
      },
      {
        task_id: "ingest-visual-1",
        kind: "ingest",
        state: "queued",
        label: "Ingest zymo_fecal_2025.05",
        updated_at_utc: "2026-07-08T19:06:00Z",
        warnings: [],
      },
    ],
    warnings: [],
  };
}

function liveStatusWorkspace() {
  visualLiveSequence += 1;
  const rateOffset = (visualLiveSequence % 5) * 16 * 1024 * 1024;
  return {
    schema_version: 1,
    availability: "available",
    sequence: visualLiveSequence,
    generated_at_utc: "2026-07-19T06:40:12Z",
    suggested_refresh_millis: 1000,
    aggregate: {
      connected_hosts: 1,
      active_stores: 1,
      active_ingests: 1,
      source_read_bytes_per_second: 734003200,
      ssd_write_bytes_per_second: 681574400 + rateOffset,
      hdd_write_bytes_per_second: 503316480 + rateOffset,
      active_hdd_transfers: 2,
    },
    hosts: [{
      display_name: "stephen-NUC12DCMi9",
      actors: ["stephen"],
      active_ingests: 1,
      object_stores: ["colo829_2024.03"],
    }],
    store_writers: [{
      store_id: "colo829_2024.03",
      hosts: ["stephen"],
      active_ingests: 1,
    }],
    ssd_ingests: [{
      job_id: "ingest-visual-live",
      store_id: "colo829_2024.03",
      host: "stephen",
      state: "ssd_ingest",
      pipeline_stage: "ssd_stage",
      current_item: "sample_0042.pod5",
      bytes_done: 68719476736,
      bytes_total: 137438953472,
      files_done: 128,
      files_total: 256,
      bytes_per_second: 681574400,
      updated_at_utc: "2026-07-19T06:40:12Z",
    }],
    hdd_transfers: [{
      job_id: "ingest-visual-live",
      store_id: "colo829_2024.03",
      disk_id: "qnap-1057",
      copy_number: 1,
      current_item: "sample_0041.pod5",
      bytes_done: 51539607552,
      bytes_total: 68719476736,
      bytes_per_second: 251658240,
      phase: "writing",
    }, {
      job_id: "ingest-visual-live",
      store_id: "colo829_2024.03",
      disk_id: "qnap-1059",
      copy_number: 2,
      current_item: "sample_0041.pod5",
      bytes_done: 49392123904,
      bytes_total: 68719476736,
      bytes_per_second: 251658240,
      phase: "fsync",
    }],
    recent: [],
    warnings: [],
  };
}

function activityCategory(kind, label, description) {
  return { kind, label, description };
}

function endpointsWorkspace() {
  const endpoint = visualEndpoint || {
    endpoint_id: "nas-staging",
    display_name: "NAS staging",
    kind: "dasobjectstore_nfs",
    manager_product_id: "dasobjectstore",
    object_service_url: "https://nas.example.test:9443",
    validation: {
      state: "validated",
      checked_at_utc: "2026-07-09T00:00:00Z",
      message: "fixture",
    },
    active_bindings: [{
      binding_id: "binding-1",
      governance_domain: "local",
      store_id: "zymo-fecal-2025-05",
      readiness: "ready",
    }],
    warnings: [],
  };
  return {
    inventory: {
      schema_version: "dasobjectstore.endpoint_inventory.v1",
      endpoint_count: 1,
      degraded_endpoint_count: 0,
      binding_count: 1,
      endpoints: [endpoint],
      warnings: [],
    },
  };
}

function endpointInventoryUpsertResponse(body) {
  visualEndpoint = {
    endpoint_id: body.endpoint_id || "visual-endpoint",
    display_name: body.display_name || "Visual endpoint",
    kind: body.kind || "dasobjectstore_nfs",
    manager_product_id: body.manager_product_id || "dasobjectstore",
    object_service_url: body.object_service_url || "https://endpoint.example.test:9443",
    validation: {
      state: body.validation?.state || "pending_validation",
      checked_at_utc: body.validation?.checked_at_utc || null,
      message: body.validation?.message || null,
    },
    active_bindings: body.active_bindings || [],
    warnings: [],
  };
  return {
    accepted: {
      job_id: "endpoint-validation-visual",
      kind: "endpoint_validation",
      accepted_at_utc: "2026-07-13T00:00:00Z",
      dry_run: Boolean(body.dry_run),
    },
    endpoint_id: body.endpoint_id || "visual-endpoint",
    display_name: body.display_name || "Visual endpoint",
    kind: body.kind || "dasobjectstore_nfs",
    validation_state: body.validation?.state || "pending_validation",
    registry_path: "/opt/dasobjectstore/endpoints.json",
    administrator_actor: "visual-admin",
    client_request_id: body.client_request_id || null,
  };
}

function usersGroupsWorkspace(role = roles[1]) {
  const canAdmin = role.administrator;
  return {
    host_mode: "standalone",
    authentication_framework: "hybrid",
    device_token_requirement: "not_required",
    current_user: {
      username: role.username,
      groups: canAdmin ? ["sudo", "bioinformatics"] : ["bioinformatics"],
      sudo_administrator: canAdmin,
    },
    users: [
      {
        username: "stephen",
        registered: true,
        created_at_unix_seconds: 1_783_411_200,
        registered_at_unix_seconds: 1_783_411_200,
        active_session_count: 1,
      },
      {
        username: role.username,
        registered: true,
        created_at_unix_seconds: 1_783_411_200,
        registered_at_unix_seconds: 1_783_411_200,
        active_session_count: 1,
        qualification_state: canAdmin ? "qualified" : "registered",
        groups: canAdmin ? ["sudo", "bioinformatics"] : [],
        sudo_administrator: canAdmin,
      },
    ],
    groups: [
      {
        group_name: "sudo",
        current_user_member: canAdmin,
        sudo_administrator_group: true,
      },
      {
        group_name: "bioinformatics",
        current_user_member: true,
        sudo_administrator_group: false,
      },
    ],
    groups_file_path: "/opt/dasobjectstore/groups.json",
    writer_groups: [
      {
        group_name: "bioinformatics",
        display_name: "Bioinformatics",
        source: "local",
        current_user_member: true,
      },
    ],
    operations: [
      localGroupOperation(
        "create_local_group",
        "Create local writer/admin group",
        canAdmin,
      ),
      localGroupOperation("assign_local_user_to_group", "Assign local user to group", canAdmin),
    ],
    capabilities: {
      product_local_user_registration: true,
      os_local_user_management: false,
      os_local_group_management: canAdmin,
      administrator_actions_enabled: canAdmin,
    },
    selected_username: role.username,
    selected_group_name: "bioinformatics",
    warnings: [],
  };
}

function localGroupOperation(kind, label, enabled) {
  return {
    kind,
    label,
    requires_sudo_administrator: true,
    enabled,
    blocked_reason: enabled ? null : "Requires sudo-derived authority.",
  };
}

function bioinformaticsWorkspace() {
  return {
    schema_version: "dasobjectstore.bioinformatics_workspace.v1",
    available: true,
    supported_object_types: [
      "BAM",
      "CRAM",
      "POD5",
      "FASTQ",
      "FASTQ.GZ",
      "FASTA",
      "VCF",
      "BCF",
      "GFF",
      "GTF",
      "ENA/SRA",
    ],
    readiness_cards: [
      readinessCard("POD5", "Nanopore POD5", "Sequencing signal", "ready", "Basecalling", "Dorado/Remora"),
      readinessCard("FASTQ", "FASTQ reads", "Reads", "ready", "Alignment/QC", "Minimap2/FastQC"),
      readinessCard("BAM", "BAM alignment", "Alignment", "watch", "Variant calling", "Samtools/BCFtools"),
    ],
    derivation_sources: [
      {
        source_kind: "ObjectStore",
        source_id: "zymo-fecal-2025-05",
        display_name: "ObjectStore, SubObject, object-type, and Mneion source records",
        object_type: "POD5",
        parent_id: null,
        endpoint_export_mode: "s3",
        mneion_binding_state: "bound",
        governance_domain: "zymo-fecal",
        workflow_roles: ["basecalling", "metagenomics"],
        evidence: ["ObjectStore object_type POD5", "Mneion governance binding zymo-fecal"],
      },
      {
        source_kind: "SubObject",
        source_id: "zymo-fecal-2025-05/raw",
        display_name: "SubObject lineage and object-type policy",
        object_type: "POD5",
        parent_id: "zymo-fecal-2025-05",
        endpoint_export_mode: "s3",
        mneion_binding_state: "inherited",
        governance_domain: "zymo-fecal",
        workflow_roles: ["basecalling"],
        evidence: ["SubObject parent relationship", "Inherited object type"],
      },
    ],
    sequencing_runs: [
      contextCard(
        "Sequencing run provenance",
        "ready",
        "Run metadata is linked to the imported POD5 folder.",
        "Flowcell, kit, run identifier, and acquisition timestamp are available to orchestration.",
      ),
    ],
    object_lineage: [
      contextCard(
        "Object lineage",
        "ready",
        "Parent ObjectStore and raw SubObject relationship is known.",
        "Derived FASTQ and BAM outputs can retain lineage to raw signal.",
      ),
    ],
    workflow_handoffs: [
      contextCard(
        "Basecalling readiness",
        "ready",
        "POD5 files can be handed to basecalling workflows.",
        "The workflow receives object type, S3 route, and governance-domain metadata.",
      ),
    ],
    governance_bindings: [
      contextCard(
        "Mnemosyne governance binding",
        "ready",
        "The ObjectStore is associated with a Mneion governance domain.",
        "Audit and downstream project visibility can be resolved by the API layer.",
      ),
    ],
    message:
      "Bioinformatics readiness is derived from ObjectStore/SubObject metadata and Mneion bindings supplied by the API.",
  };
}

function readinessCard(objectType, label, category, state, primaryWorkflow, handoff) {
  return {
    object_type: objectType,
    label,
    category,
    state,
    primary_workflow: primaryWorkflow,
    handoff,
    required_metadata: ["object type", "settled placement", "governance binding"],
  };
}

function contextCard(label, state, summary, detail) {
  return {
    label,
    state,
    summary,
    detail,
    evidence: ["API fixture", "workflow contract"],
  };
}

function actionPlanResponse(body) {
  if (body.action === "subobject_create") {
    const name = body.subobject_name || "pod5/raw";
    return {
      action: "subobject_create",
      execution: "daemon",
      argv: ["dasobjectstore", "subobject", "create", name],
      mutates_pool: true,
      writes_recovery_metadata: true,
      confirmation_required: true,
    };
  }
  const storeId = body.store_id || "visual-e2e-store";
  return {
    action: body.action || "store_create",
    execution: "daemon",
    argv: ["dasobjectstore", "store", "create", storeId],
    mutates_pool: true,
    writes_recovery_metadata: true,
    confirmation_required: true,
  };
}

function enclosurePrepareResponse(role) {
  return {
    accepted: {
      job_id: "enclosure-prepare-visual",
      kind: "enclosure_preparation",
      accepted_at_utc: "2026-07-08T19:10:00Z",
      dry_run: false,
    },
    ssd_device: "/dev/nvme0n1",
    hdd_devices: [
      { disk_id: "qnap-1057", device_path: "/dev/sda" },
      { disk_id: "qnap-1058", device_path: "/dev/sdb" },
      { disk_id: "qnap-1059", device_path: "/dev/sdc" },
    ],
    mount_root: "/srv/dasobjectstore",
    filesystem: "ext4",
    owner: role.username,
    administrator_actor: role.username,
    client_request_id: null,
  };
}

function objectStoreCreateResponse(role) {
  return {
    accepted: {
      job_id: "objectstore-create-visual",
      kind: "object_store_creation",
      accepted_at_utc: "2026-07-08T19:11:00Z",
      dry_run: false,
    },
    store_id: "visual-e2e-store",
    store_class: "research",
    required_copies: 1,
    bucket: "visual-e2e-store",
    writer_group: "bioinformatics",
    ssd_root: "/srv/dasobjectstore/ssd",
    object_type: "naive",
    enclosure_id: "qnap-tl-d800c-visual",
    public: false,
    writeable: true,
    capacity_behavior: "balanced",
    retention: "standard",
    endpoint_export_mode: "s3_bucket",
    administrator_actor: role.username,
    client_request_id: null,
  };
}

function localGroupAdminResponse(operation, groupName, username, body) {
  const dryRun = Boolean(body.dry_run);
  const jobPrefix = operation === "create_local_group" ? "local-group" : "local-group-assign";
  return {
    accepted: {
      job_id: `${jobPrefix}-${dryRun ? "dry-run" : "apply"}-visual`,
      kind: "system_administration",
      accepted_at_utc: "2026-07-08T19:12:00Z",
      dry_run: dryRun,
    },
    operation,
    group_name: groupName,
    username,
    client_request_id: null,
  };
}

function adminJobStatusResponse(pathname) {
  const jobId = pathname.split("/").filter(Boolean).at(-1);
  return {
    job: {
      job_id: jobId,
      kind: jobId.includes("enclosure") ? "enclosure_preparation" : "system_administration",
      state: "running",
      progress: {
        stage: "validating-plan",
        work_bytes_done: 0,
        work_bytes_total: 0,
        work_units_done: 1,
        work_units_total: 4,
        message: "Daemon accepted the administrator workflow and is validating media.",
      },
      percent_complete: 25,
      submitted_at_utc: "2026-07-08T19:10:00Z",
      updated_at_utc: "2026-07-08T19:10:02Z",
      actor: "visual-admin",
      failure_message: null,
    },
  };
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
