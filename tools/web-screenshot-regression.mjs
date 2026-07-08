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
    name: "bioinformatics",
    selector: "button[data-page='bioinformatics']",
    pageSelector: "section[data-page='bioinformatics']",
    readySelector: "text=Bioinformatics workspace is reserved.",
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
      await captureAuthenticatedPages(browser, baseUrl, viewport);
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

async function captureAuthenticatedPages(browser, baseUrl, viewport) {
  const context = await browser.newContext({ viewport });
  await context.addInitScript(() => {
    window.localStorage.setItem("dasobjectstore.username", "visual-operator");
    window.localStorage.setItem("dasobjectstore.session_token", "visual-session-token");
  });
  const page = await context.newPage();
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.locator(".dos-topbar").waitFor();

  for (const pageSpec of authenticatedPages) {
    await page.locator(pageSpec.selector).click();
    await page.locator(pageSpec.pageSelector).waitFor();
    await page.locator(pageSpec.readySelector).waitFor();
    await page.waitForLoadState("networkidle");
    await assertVisualContract(page, { auth: true });
    await page.screenshot({
      path: join(artifactDir, `${viewport.name}-${pageSpec.name}.png`),
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
    response.writeHead(200, { "content-type": "application/json; charset=utf-8" });
    response.end(JSON.stringify(apiResponse(url.pathname, request.method)));
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

function apiResponse(pathname, method) {
  if (pathname === `${apiBase}/session` && method === "POST") {
    return {
      username: "visual-operator",
      valid: true,
      expires_at_unix_seconds: 1_803_988_800,
    };
  }
  if (pathname === `${apiBase}/login` && method === "POST") {
    return {
      username: "visual-operator",
      session_token: "visual-session-token",
      expires_at_unix_seconds: 1_803_988_800,
    };
  }
  if (pathname === `${apiBase}/logout` && method === "POST") {
    return { username: "visual-operator", disconnected: true };
  }
  if (pathname === `${apiV1Base}/dashboard/home`) {
    return homeDashboard();
  }
  if (pathname === `${apiV1Base}/dashboard/enclosures`) {
    return enclosuresDashboard();
  }
  if (pathname === `${apiV1Base}/dashboard/object-stores`) {
    return objectStoresDashboard();
  }
  if (pathname === `${apiV1Base}/workspaces/activity`) {
    return activityWorkspace();
  }
  if (pathname === `${apiV1Base}/workspaces/bioinformatics`) {
    return bioinformaticsWorkspace();
  }
  return {};
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
    object_stores: objectStoresDashboard().stores,
  };
}

function enclosuresDashboard() {
  return {
    schema_version: "dasobjectstore.enclosures_page.v1",
    generated_at_utc: "2026-07-08T19:00:00Z",
    add_enclosure: {
      enabled: false,
      action_kind: "enclosure_add",
      label: "Add enclosure",
      state: "admin_required",
      administrator: false,
      supported_enclosure_detected: true,
      daemon_ready: true,
      confirmation_required: true,
      blocked_reason: "Administrator rights are required before preparing DAS media.",
      next_step: "Sign in as a sudo-capable operator to prepare an enclosure.",
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

function objectStoresDashboard() {
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
      enabled: false,
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
      blocked_reason: "Administrator rights are required to create an ObjectStore.",
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

function bioinformaticsWorkspace() {
  return {
    schema_version: "dasobjectstore.bioinformatics_workspace.v1",
    available: false,
    supported_object_types: ["BAM", "CRAM", "POD5", "FASTQ", "FASTA", "VCF", "GFF", "ENA/SRA"],
    message:
      "Bioinformatics workflow cards are reserved while daemon-backed orchestration is completed.",
  };
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
