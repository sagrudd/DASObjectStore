/** Stable fixture matrix shared by the screenshot runner and fixture checks. */
export const viewports = [
  { name: "desktop", width: 1440, height: 1000 },
  { name: "mobile", width: 390, height: 844 },
];

export const roles = [
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

export const authenticatedPages = [
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
    readySelector: "[data-panel='reporting']",
  },
  {
    name: "endpoints",
    selector: "button[data-page='endpoints']",
    pageSelector: "section[data-page='endpoints']",
    readySelector: "[data-section='endpoint-inventory']",
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
