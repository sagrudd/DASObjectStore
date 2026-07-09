#!/usr/bin/env node

import { createServer } from "node:http";
import { mkdir, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { extname, join, normalize, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { chromium } from "playwright";

const execFileAsync = promisify(execFile);
const repoRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const distDir = join(repoRoot, "crates", "dasobjectstore-gui-web", "dist");
const artifactDir = join(repoRoot, "target", "web-screenshots");
const publicBase = "/products/dasobjectstore/";
const apiBase = "/products/dasobjectstore/api";
const apiV1Base = "/products/dasobjectstore/api/v1";

const viewports = [
  { name: "desktop", width: 1440, height: 1000 },
  { name: "mobile", width: 390, height: 844 },
];

const roles = [
  {
    name: "viewer",
    username: "visual-viewer",
    token: "visual-viewer-token",
    administrator: false,
  },
  {
    name: "admin",
    username: "visual-admin",
    token: "visual-admin-token",
    administrator: true,
  },
];

const authenticatedPages = [
  {
    name: "home",
    selector: "button[data-page='home']",
    pageSelector: "section[data-page='home']",
    readySelector: "text=Registered object stores visible to this appliance",
  },
  {
    name: "enclosures",
    selector: "button[data-page='enclosures']",
    pageSelector: "section[data-page='enclosures']",
    readySelector: "[data-enclosure-id='qnap-tl-d800c-visual']",
  },
  {
    name: "objectstores",
    selector: "button[data-page='objectstores']",
    pageSelector: "section[data-page='objectstores']",
    readySelector: "[data-store-id='zymo-fecal-2025-05']",
  },
  {
    name: "activity",
    selector: "button[data-page='activity']",
    pageSelector: "section[data-page='activity']",
    readySelector: "text=Daemon task stream",
  },
  {
    name: "users-groups",
    selector: "button[data-page='users-groups']",
    pageSelector: "section[data-page='users-groups']",
    readySelector: "[data-action='assign_local_user_to_group']",
  },
  {
    name: "bioinformatics",
    selector: "button[data-page='bioinformatics']",
    pageSelector: "section[data-page='bioinformatics']",
    readySelector: "[data-object-type='POD5'][data-state='ready']",
  },
];

async function main() {
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
  await page.locator(".dos-product-footer__version").waitFor();
  await page.getByText("Developed by").waitFor();
  await page.getByRole("link", { name: "Mnemosyne" }).waitFor();

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
    if (!bodyText.includes("Developed by")) {
      issues.push("footer provenance text missing");
    }

    const footer = document.querySelector(".dos-product-footer");
    if (!footer || !visible(footer)) {
      issues.push("footer is not visible");
    } else {
      const footerStyle = window.getComputedStyle(footer);
      if (!footerStyle.fontFamily.toLowerCase().includes("mono")) {
        issues.push(`footer is not using a monospace stack: ${footerStyle.fontFamily}`);
      }
      if (footerStyle.backgroundColor === "rgba(0, 0, 0, 0)") {
        issues.push("footer background is transparent");
      }
    }

    if (authenticated && !document.querySelector(".dos-topbar")) {
      issues.push("authenticated view is missing the top bar");
    }

    const brandLogos = Array.from(document.querySelectorAll(".dos-brand-logo"));
    if (brandLogos.length === 0) {
      issues.push("Mnemosyne brand logo is missing");
    }
    for (const logo of brandLogos) {
      const rect = logo.getBoundingClientRect();
      if (!visible(logo)) {
        issues.push("Mnemosyne brand logo is not visible");
      }
      if (rect.width < 10 || rect.height < 18) {
        issues.push(`Mnemosyne brand logo renders too small: ${rect.width}x${rect.height}`);
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
    case "objectstores":
      await assertObjectStoreWorkflow(page, role);
      break;
    case "users-groups":
      await assertUsersGroupsWorkflow(page, role);
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
  const card = page.locator("[data-action='enclosure_add']");
  await card.waitFor();
  const planButton = card.getByRole("button", { name: "Plan preparation" });

  if (!role.administrator) {
    await expectDisabled(planButton, "non-admin enclosure preparation must be disabled");
    await card.getByText("Admin required").waitFor();
    return;
  }

  await expectEnabled(planButton, "admin enclosure preparation must be enabled");
  await planButton.click();
  await page.locator("[data-workflow='enclosure_add']").waitFor();
  await page.getByText("SSD landing device").waitFor();
  await page.getByLabel("I allow formatting of the selected devices.").check();
  await page.getByLabel("I acknowledge existing data on selected devices may be destroyed.").check();
  await page.getByPlaceholder("confirm prepare das").fill("confirm prepare das");
  await page.getByRole("button", { name: "Submit preparation job" }).click();
  await page.getByText("Job enclosure-prepare-visual").waitFor();
}

async function assertObjectStoreWorkflow(page, role) {
  const createCard = page.locator("[data-action='store_create']");
  const createButton = createCard.getByRole("button", { name: "Configure store" });
  const subobjectCard = page.locator("[data-action='subobject_create']");
  const subobjectButton = subobjectCard.getByRole("button", { name: "Define SubObject" });

  if (!role.administrator) {
    await expectDisabled(createButton, "non-admin ObjectStore creation must be disabled");
    await expectDisabled(subobjectButton, "non-admin SubObject creation must be disabled");
    await createCard.getByText("Admin only").waitFor();
    await subobjectCard.getByText("Admin only").waitFor();
    return;
  }

  await expectEnabled(createButton, "admin ObjectStore creation must be enabled");
  await createButton.click();
  await createCard.getByLabel("Store name").fill("visual-e2e-store");
  await createCard.getByLabel("Enclosure anchor").fill("qnap-tl-d800c-visual");
  await createCard.getByRole("button", { name: "Review daemon plan" }).click();
  await createCard.getByText("dasobjectstore store create visual-e2e-store").waitFor();
  await createCard.getByPlaceholder("confirm create objectstore").fill("confirm create objectstore");
  await createCard.getByRole("button", { name: "Submit daemon job" }).click();
  await createCard.getByText("ObjectStore creation submitted to dasobjectstored.").waitFor();

  await expectEnabled(subobjectButton, "admin SubObject creation must be enabled");
  await subobjectButton.click();
  await subobjectCard.getByLabel("SubObject name").fill("pod5/raw");
  await subobjectCard.getByRole("button", { name: "Review SubObject plan" }).click();
  await subobjectCard.getByText("dasobjectstore subobject create pod5/raw").waitFor();
}

async function assertUsersGroupsWorkflow(page, role) {
  const createCard = page.locator("[data-action='create_local_group']");
  const assignCard = page.locator("[data-action='assign_local_user_to_group']");
  const createPreview = createCard.getByRole("button", { name: "Dry-run preview" });
  const assignPreview = assignCard.getByRole("button", { name: "Dry-run preview" });

  if (!role.administrator) {
    await createCard.getByText("Admin only").waitFor();
    await assignCard.getByText("Admin only").waitFor();
    await expectDisabled(createPreview, "non-admin group creation preview must be disabled");
    await expectDisabled(assignPreview, "non-admin group assignment preview must be disabled");
    return;
  }

  await createCard.getByLabel("Group name").fill("mnemosyne-writers");
  await expectEnabled(createPreview, "admin group creation preview must be enabled");
  await createPreview.click();
  await createCard.getByText("Job local-group-dry-run-visual").waitFor();
  await createCard.getByPlaceholder("confirm local group administration").fill("confirm local group administration");
  await createCard.getByRole("button", { name: "Submit group creation" }).click();
  await createCard.getByText("Job local-group-apply-visual").waitFor();

  await assignCard.getByLabel("Username").fill("stephen");
  await assignCard.getByLabel("Group").selectOption("bioinformatics");
  await expectEnabled(assignPreview, "admin group assignment preview must be enabled");
  await assignPreview.click();
  await assignCard.getByText("Job local-group-assign-dry-run-visual").waitFor();
  await assignCard.getByPlaceholder("confirm local group administration").fill("confirm local group administration");
  await assignCard.getByRole("button", { name: "Submit group assignment" }).click();
  await assignCard.getByText("Job local-group-assign-apply-visual").waitFor();
}

async function assertActivityWorkflow(page) {
  await page.getByText("Administrator jobs", { exact: true }).waitFor();
  await page.getByText("Enclosure preparation", { exact: true }).waitFor();
  await page.getByText("ObjectStore creation", { exact: true }).waitFor();
  await page.getByText("SubObject creation", { exact: true }).waitFor();
  await page.getByText("Create local writer group").waitFor();
  await page.getByText("Ingest zymo_fecal_2025.05").waitFor();
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
  if (pathname === `${apiV1Base}/dashboard/home`) {
    return homeDashboard();
  }
  if (pathname === `${apiV1Base}/dashboard/enclosures`) {
    return enclosuresDashboard(role);
  }
  if (pathname === `${apiV1Base}/dashboard/object-stores`) {
    return objectStoresDashboard(role);
  }
  if (pathname === `${apiV1Base}/workspaces/activity`) {
    return activityWorkspace();
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
      window_days: 7,
      read_tib: "14.2",
      written_tib: "18.8",
      ingest_tib: "12.6",
      avg_read_mib_s: 420,
      avg_write_mib_s: 310,
    },
    ingest: { pressure: "normal", queued_jobs: 2, active_jobs: 1, failed_jobs: 0, warnings: [] },
    destage: { pending_objects: 12, copying_objects: 2, verified_objects: 950, warnings: [] },
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
      blocked_reason: canAdmin
        ? null
        : "Administrator rights are required before preparing DAS media.",
      next_step: canAdmin
        ? "Review detected SSD/HDD devices before submitting the daemon preparation job."
        : "Sign in as a sudo-capable operator to prepare an enclosure.",
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
    groups: [
      {
        group_name: "bioinformatics",
        display_name: "Bioinformatics",
        source: "local",
        current_user_member: true,
      },
    ],
    stores: [
      {
        store_id: "zymo-fecal-2025-05",
        display_name: "zymo_fecal_2025.05",
        store_class: "research",
        object_type: "POD5",
        health: "ready",
        required_copies: 1,
        object_count: 245,
        capacity: capacity("42.0", "2.3", "39.7", 548),
        placement_policy: "fractional free-space placement",
        endpoint_export_mode: "s3",
        writer_group: "bioinformatics",
        public: false,
        writeable: true,
        created_at_utc: "2026-07-08T12:00:00Z",
        last_ingested_at_utc: "2026-07-08T18:50:00Z",
        writer_policy: {
          writer_group: "bioinformatics",
          group_defined: true,
          current_user_member: true,
          writeable_by_current_user: true,
          state: "ready",
          message: "Current user can write through the bioinformatics group.",
        },
        warnings: [],
      },
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

function activityCategory(kind, label, description) {
  return { kind, label, description };
}

function usersGroupsWorkspace(role = roles[1]) {
  const canAdmin = role.administrator;
  return {
    host_mode: "standalone",
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
