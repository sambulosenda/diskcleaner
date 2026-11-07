// Roomy site demo. Everything here is example data; nothing reads the visitor's Mac.
// The hero is an example Mac running Roomy. The window copies the real app's screens on main:
// the sidebar sections, the Overview (one answer, a bar, Ready to clean, Worth a look), the item
// list with tier labels, the bar under it once something is ticked, and the review sheet.
document.documentElement.classList.add("js");
(function () {
  "use strict";

  var DISK_GB = 512;
  var FREE_GB = 16.4;

  // Categories and colours follow CoreTypesUI.swift (system colours).
  var CATS = {
    cache: { label: "Caches", color: "var(--c-blue)", icon: "grid" },
    build: { label: "Build files", color: "var(--c-indigo)", icon: "hammer" },
    developerTools: { label: "Developer tools", color: "var(--c-purple)", icon: "wrench" },
    docker: { label: "Docker", color: "var(--c-cyan)", icon: "box" },
    appData: { label: "App data", color: "var(--c-teal)", icon: "folderGear" },
    downloads: { label: "Downloads and installers", color: "var(--c-yellow)", icon: "arrowDown" },
    userData: { label: "Your files", color: "var(--c-orange)", icon: "person" },
    backups: { label: "Backups", color: "var(--c-pink)", icon: "extDrive" },
    system: { label: "System Data", color: "var(--c-gray)", icon: "gear" }
  };

  // tier: safe (Likely safe), review (Review first), keep (Keep, never offered).
  // screens: which sidebar entries list the item. place: the name shown in Developer mode only.
  var ITEMS = [
    { id: "x1", cat: "build", screens: ["projects", "xcode"], tier: "safe", gb: 18.4, title: "Xcode build files", place: "DerivedData", explain: "Build files Xcode makes again the next time you build.", path: "~/Library/Developer/Xcode/DerivedData", changed: "3 days ago" },
    { id: "x2", cat: "developerTools", screens: ["xcode", "tools"], tier: "safe", gb: 9.6, title: "Unavailable simulators", explain: "Simulators for iOS versions that are no longer installed.", path: "xcrun simctl delete unavailable", cmd: true, changed: "5 months ago" },
    { id: "x3", cat: "developerTools", screens: ["xcode"], tier: "safe", gb: 6.3, title: "iOS device support", place: "16.4", explain: "Debug files for an iPhone version you haven't tested on in a year.", path: "~/Library/Developer/Xcode/iOS DeviceSupport/16.4", changed: "1 year ago" },
    { id: "a1", cat: "developerTools", screens: ["android"], tier: "review", gb: 6.1, title: "Android system image", place: "API 30", explain: "No project on this Mac uses API 30. Android Studio can download it again.", path: "~/Library/Android/sdk/system-images/android-30" },
    { id: "a2", cat: "cache", screens: ["android", "appscaches"], tier: "safe", gb: 4.8, title: "Gradle caches", explain: "Downloaded build files. Gradle fetches them again when a build needs them.", path: "~/.gradle/caches", changed: "2 weeks ago" },
    { id: "j1", cat: "build", screens: ["projects", "javascript"], tier: "safe", gb: 5.8, title: "node_modules", place: "6 old projects", explain: "Last worked on more than 3 months ago. npm install brings them back.", path: "~/Developer/*/node_modules", changed: "4 months ago" },
    { id: "j2", cat: "build", screens: ["projects", "javascript"], tier: "keep", gb: 0.9, title: "node_modules", place: "roomy-site", explain: "Skipped: you changed this project 2 hours ago.", path: "~/Developer/roomy-site/node_modules" },
    { id: "d1", cat: "docker", screens: ["docker", "tools"], tier: "safe", gb: 7.2, title: "Docker build cache", explain: "Cached build steps. Docker rebuilds them when needed.", path: "docker builder prune", cmd: true, changed: "2 weeks ago" },
    { id: "d2", cat: "docker", screens: ["docker"], tier: "review", gb: 11.4, title: "Images no container uses", explain: "Pulling them again takes time on a slow connection.", path: "docker image prune", cmd: true },
    { id: "d3", cat: "docker", screens: ["docker"], tier: "keep", gb: 12.0, title: "Docker volumes", explain: "These hold your containers' data. Roomy never removes them.", path: "docker volume ls" },
    { id: "ai1", cat: "appData", screens: ["appscaches", "largefiles"], tier: "review", gb: 39.6, title: "Ollama model", place: "llama3.1:70b", explain: "Last used 3 months ago. Takes hours to download again.", path: "~/.ollama/models" },
    { id: "ai2", cat: "appData", screens: ["appscaches"], tier: "review", gb: 8.1, title: "LM Studio model", place: "qwen2.5-14b", explain: "Last used 6 weeks ago.", path: "~/.lmstudio/models" },
    { id: "h1", cat: "cache", screens: ["appscaches", "tools"], tier: "safe", gb: 2.3, title: "Old Homebrew versions", explain: "Older copies of tools Homebrew has already updated.", path: "brew cleanup", cmd: true, changed: "1 month ago" },
    { id: "c1", cat: "cache", screens: ["appscaches"], tier: "safe", gb: 3.2, title: "Browser and app caches", explain: "Copies of web pages and app data. Apps make them again as you use them.", path: "~/Library/Caches", changed: "today" },
    { id: "f1", cat: "backups", screens: ["backups", "largefiles"], tier: "review", gb: 14.3, title: "iPhone backup", place: "Sam's old iPhone", explain: "Last backed up 14 months ago. Check you have a newer backup first.", path: "~/Library/Application Support/MobileSync/Backup" },
    { id: "f2", cat: "downloads", screens: ["downloads"], tier: "review", gb: 3.9, title: "Installers in Downloads", place: "12 files", explain: ".dmg and .pkg files for apps that are already installed.", path: "~/Downloads" },
    { id: "f3", cat: "userData", screens: ["duplicates", "largefiles"], tier: "review", gb: 1.7, title: "Copy of Screen Recording.mov", explain: "The same file is also in Movies. You pick the copy to keep.", path: "~/Desktop/Screen Recording.mov" },
    { id: "f4", cat: "userData", screens: ["libraries"], tier: "review", gb: 6.8, title: "Messages attachments", explain: "Photos and videos from chats, mostly from 2023. Open Messages to keep the ones you want.", path: "~/Library/Messages/Attachments" }
  ];

  // Used space outside the scanned items, so the bar adds up to the disk.
  var OTHER_GB = 140.0; // other files in the home folder
  var OUTSIDE_GB = DISK_GB - FREE_GB - OTHER_GB - ITEMS.reduce(function (s, i) { return s + i.gb; }, 0);

  // The real sidebar (SidebarItem.swift): main items, Developer (Developer mode only), Clean up.
  var SIDEBAR = [
    { items: [
      { key: "overview", title: "Overview", icon: "drive" },
      { key: "backups", title: "Backups", icon: "extDrive" },
      { key: "libraries", title: "Photos, Mail and Music", icon: "photo" },
      { key: "downloads", title: "Downloads and installers", icon: "arrowDown" },
      { key: "largefiles", title: "Large files", icon: "doc" },
      { key: "duplicates", title: "Duplicates", icon: "docs" },
      { key: "appscaches", title: "Apps and caches", icon: "grid" },
      { key: "systemdata", title: "System Data", icon: "gear" }
    ] },
    { header: "Developer", dev: true, items: [
      { key: "projects", title: "Projects", icon: "folder" },
      { key: "xcode", title: "Xcode", icon: "hammer" },
      { key: "android", title: "Android", icon: "phone" },
      { key: "javascript", title: "JavaScript", icon: "braces" },
      { key: "docker", title: "Docker", icon: "box" },
      { key: "tools", title: "Tool clean-ups", icon: "wrench" }
    ] },
    { header: "Clean up", items: [
      { key: "apps", title: "Remove apps", icon: "xbin" },
      { key: "cleanup", title: "Cleanup", icon: "sparkles" },
      { key: "history", title: "History", icon: "clock" }
    ] }
  ];
  var DEV_SCREENS = { projects: 1, xcode: 1, android: 1, javascript: 1, docker: 1, tools: 1 };

  var TIER = {
    safe: { label: "Likely safe", icon: "tierSafe", tint: "var(--sys-green)" },
    review: { label: "Review first", icon: "tierReview", tint: "var(--sys-orange)" },
    keep: { label: "Keep", icon: "tierKeep", tint: "var(--sys-gray)" }
  };

  // Line icons drawn in the spirit of SF Symbols (24 x 24, stroked with currentColor).
  var ICONS = {
    drive: '<rect x="3" y="7" width="18" height="10" rx="2.5"/><circle cx="17" cy="12" r="1" fill="currentColor"/>',
    extDrive: '<rect x="3" y="8" width="18" height="9" rx="2.5"/><path d="M7 13.5h6"/>',
    photo: '<rect x="3" y="5" width="18" height="14" rx="2.5"/><circle cx="9" cy="10" r="1.6"/><path d="M21 16l-5-5-9 8"/>',
    arrowDown: '<circle cx="12" cy="12" r="9"/><path d="M12 7.5v9M8 12.5l4 4 4-4"/>',
    doc: '<path d="M7 3h7l4 4v14H7z"/><path d="M14 3v4h4"/>',
    docs: '<rect x="8" y="7" width="11" height="14" rx="1.5"/><path d="M5 17V5a1.5 1.5 0 0 1 1.5-1.5H15"/>',
    grid: '<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>',
    gear: '<circle cx="12" cy="12" r="3"/><circle cx="12" cy="12" r="7.5" stroke-dasharray="3.2 2.7"/>',
    folder: '<path d="M3 7.5A2 2 0 0 1 5 5.5h4l2 2h8a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    folderGear: '<path d="M3 7.5A2 2 0 0 1 5 5.5h4l2 2h8a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><circle cx="14" cy="13.5" r="2"/>',
    hammer: '<path d="M14 4l6 6-2.5 2.5-6-6z"/><path d="M12.5 8.5L4 17l3 3 8.5-8.5"/>',
    phone: '<rect x="7" y="2.5" width="10" height="19" rx="2.5"/><path d="M11 18.5h2"/>',
    braces: '<path d="M9 4c-2 0-3 1-3 3v2c0 1.6-.8 3-2 3 1.2 0 2 1.4 2 3v2c0 2 1 3 3 3M15 4c2 0 3 1 3 3v2c0 1.6.8 3 2 3-1.2 0-2 1.4-2 3v2c0 2-1 3-3 3"/>',
    box: '<path d="M3.5 8L12 3.5 20.5 8v8L12 20.5 3.5 16z"/><path d="M3.5 8L12 12.5 20.5 8M12 12.5v8"/>',
    wrench: '<path d="M15 4.5a4.5 4.5 0 0 0 4.5 6L10 20l-3-3 9.5-9.5A4.5 4.5 0 0 1 15 4.5z"/>',
    xbin: '<rect x="5" y="7" width="14" height="13" rx="2"/><path d="M3 7h18M9 4h6M10 11.5l4 4M14 11.5l-4 4"/>',
    sparkles: '<path d="M11 3l1.8 5.2L18 10l-5.2 1.8L11 17l-1.8-5.2L4 10l5.2-1.8zM18 15l.8 2.2L21 18l-2.2.8L18 21l-.8-2.2L15 18l2.2-.8z"/>',
    clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3.5 2"/>',
    person: '<rect x="3.5" y="3.5" width="17" height="17" rx="3"/><circle cx="12" cy="10" r="3"/><path d="M6.5 19c1-3 3-4.5 5.5-4.5s4.5 1.5 5.5 4.5"/>',
    search: '<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>',
    refresh: '<path d="M19.5 12A7.5 7.5 0 1 1 17.3 6.7"/><path d="M19.5 4v5h-5"/>',
    chevron: '<path d="M9.5 6l6 6-6 6"/>',
    chevronDown: '<path d="M7 10l5 5 5-5"/>',
    ellipsis: '<circle cx="6" cy="12" r="1.3" fill="currentColor"/><circle cx="12" cy="12" r="1.3" fill="currentColor"/><circle cx="18" cy="12" r="1.3" fill="currentColor"/>',
    sidebar: '<rect x="3" y="5" width="18" height="14" rx="2.5"/><path d="M9.5 5v14"/>',
    warn: '<path d="M12 4l9 16H3z" fill="currentColor" stroke="none"/><path d="M12 10v4M12 17v.5" stroke="var(--win-bg)"/>',
    tierSafe: '<circle cx="12" cy="12" r="9" fill="currentColor" stroke="none"/><path d="M8 12.5l2.7 2.7L16.2 9.5" stroke="#fff"/>',
    tierReview: '<circle cx="12" cy="12" r="9" fill="currentColor" stroke="none"/><path d="M6.8 12s2-3.3 5.2-3.3 5.2 3.3 5.2 3.3-2 3.3-5.2 3.3S6.8 12 6.8 12z" stroke="#fff" stroke-width="1.4"/><circle cx="12" cy="12" r="1.3" fill="#fff" stroke="none"/>',
    tierKeep: '<circle cx="12" cy="12" r="9" fill="currentColor" stroke="none"/><rect x="8.8" y="11" width="6.4" height="5" rx="1" fill="#fff" stroke="none"/><path d="M10 11V9.6a2 2 0 0 1 4 0V11" stroke="#fff" stroke-width="1.4"/>'
  };

  var state = {
    open: false,
    phase: "idle", // idle | scanning | ready
    screen: "overview",
    tierFilter: null, // null (Everything) | safe | review
    mode: "developer",
    checked: {},
    trashed: {},
    selected: null,
    sheet: null, // null | preview | done
    lastMoved: [],
    history: [],
    touched: false,
    popChecked: {}
  };

  var $ = function (id) { return document.getElementById(id); };

  // ---- small helpers ----

  /** Sizes as the app shows them: decimal GB with one decimal, MB under 1 GB. */
  function fmt(gb) {
    if (gb < 1) return Math.round(gb * 1000) + " MB";
    return gb.toFixed(1) + " GB";
  }
  /** Builds an element: attributes (text, class, html, on*) and children. */
  function el(tag, attrs, children) {
    var n = document.createElement(tag);
    if (attrs) Object.keys(attrs).forEach(function (k) {
      var v = attrs[k];
      if (v === undefined || v === null || v === false) return;
      if (k === "text") n.textContent = v;
      else if (k === "class") n.className = v;
      else if (k === "html") n.innerHTML = v;
      else if (k.slice(0, 2) === "on") n.addEventListener(k.slice(2), v);
      else n.setAttribute(k, v === true ? "" : v);
    });
    (children || []).forEach(function (c) { if (c) n.appendChild(typeof c === "string" ? document.createTextNode(c) : c); });
    return n;
  }
  /** An inline icon from ICONS. */
  function icon(name, cls, color) {
    var span = el("span", { class: "ico" + (cls ? " " + cls : ""), "aria-hidden": "true" });
    span.innerHTML = '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">' + ICONS[name] + "</svg>";
    if (color) span.style.color = color;
    return span;
  }
  function sum(list) { return list.reduce(function (s, i) { return s + i.gb; }, 0); }
  function live() { return ITEMS.filter(function (i) { return !state.trashed[i.id]; }); }
  function inMode(i) { return state.mode === "developer" || !i.screens.every(function (s) { return DEV_SCREENS[s]; }); }
  function freeGb() { return FREE_GB + sum(ITEMS.filter(function (i) { return state.trashed[i.id]; })); }
  function safeGb() { return sum(live().filter(function (i) { return i.tier === "safe" && inMode(i); })); }
  function reviewGb() { return sum(live().filter(function (i) { return i.tier === "review" && inMode(i); })); }
  function chosen() { return live().filter(function (i) { return state.checked[i.id] && inMode(i); }); }
  function screenInfo(key) {
    for (var s = 0; s < SIDEBAR.length; s++) for (var j = 0; j < SIDEBAR[s].items.length; j++) if (SIDEBAR[s].items[j].key === key) return SIDEBAR[s].items[j];
    return SIDEBAR[0].items[0];
  }

  // ---- the window ----

  function renderWindow() {
    // Rebuilding replaces every control, so put keyboard focus back on the same one afterwards.
    var active = document.activeElement;
    var focusId = active && active.id && $("win").contains(active) ? active.id : null;
    renderSidebar();
    var info = screenInfo(state.screen);
    $("win-title").textContent = info.title;
    $("win-mode").textContent = state.mode === "developer" ? "Developer" : "Simple";
    var content = $("win-content");
    content.textContent = "";
    content.className = "win-content";
    if (state.phase === "idle" || state.phase === "scanning") content.appendChild(scanningView());
    else if (state.screen === "overview") content.appendChild(overviewView());
    else if (state.screen === "systemdata") content.appendChild(systemDataView());
    else if (state.screen === "history") content.appendChild(historyView());
    else if (state.screen === "apps") content.appendChild(appsView());
    else { content.className = "win-content is-list"; content.appendChild(listView()); }
    if (focusId && $(focusId)) $(focusId).focus({ preventScroll: true });
    renderSheet();
    renderPopover();
    updateTrash();
  }

  function renderSidebar() {
    var nav = $("win-sidebar");
    nav.textContent = "";
    SIDEBAR.forEach(function (section) {
      if (section.dev && state.mode !== "developer") return;
      if (section.header) nav.appendChild(el("div", { class: "sb-head", text: section.header }));
      section.items.forEach(function (it) {
        nav.appendChild(el("button", {
          type: "button", class: "sb-item", id: "sb-" + it.key, "aria-current": state.screen === it.key ? "page" : null,
          onclick: function () { state.touched = true; showScreen(it.key); }
        }, [icon(it.icon), el("span", { text: it.title })]));
      });
    });
  }

  function showScreen(key, opts) {
    state.screen = key;
    state.tierFilter = opts && opts.tier !== undefined ? opts.tier : null;
    state.selected = null;
    if (state.phase === "idle") { openWindow(); return; }
    renderWindow();
  }

  function scanningView() {
    var wrap = el("div", { class: "scan-view" }, [
      el("div", { class: "spinner", "aria-hidden": "true" }),
      el("h2", { text: "Scanning this Mac…" }),
      el("p", { class: "sec", text: "Roomy only reads sizes. Nothing is removed." }),
      el("div", { class: "pbar" }, [el("span", { id: "scan-fill" })]),
      el("p", { class: "mono sec", id: "scan-path", text: "~/Library" })
    ]);
    return wrap;
  }

  var SCAN_PATHS = ["~/Library/Developer/Xcode/DerivedData", "~/Library/Developer/CoreSimulator", "~/Library/Android/sdk", "~/.gradle/caches",
    "~/Developer/shop-app/node_modules", "~/Library/Containers/com.docker.docker", "~/.ollama/models", "/opt/homebrew/Cellar",
    "~/Library/Application Support/MobileSync", "~/Downloads", "~/Movies", "~/Library/Caches"];

  function startScan() {
    if (state.phase === "scanning") return;
    state.phase = "scanning";
    state.sheet = null;
    renderWindow();
    var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    var total = reduce ? 1 : 24, n = 0;
    var t = setInterval(function () {
      n++;
      var fill = $("scan-fill"), path = $("scan-path");
      if (fill) fill.style.width = (n / total * 100) + "%";
      if (path) path.textContent = SCAN_PATHS[n % SCAN_PATHS.length];
      if (n >= total) { clearInterval(t); state.phase = "ready"; renderWindow(); }
    }, 80);
  }

  // The Overview: "You can likely free X", the bar, Ready to clean, Worth a look, footer.
  function overviewView() {
    var safe = safeGb(), review = reviewGb(), free = freeGb();
    var col = el("div", { class: "ov" });
    col.appendChild(el("div", { class: "ov-head" }, [
      el("h1", { text: safe > 0 ? "You can likely free " + fmt(safe) : "Nothing is clearly safe to remove right now" }),
      el("p", { class: "sec lg", text: fmt(free) + " free now" + (safe > 0 ? " · " + fmt(Math.min(DISK_GB, free + safe)) + " after cleaning" : "") })
    ]));
    col.appendChild(diskBar());

    if (safe > 0) {
      var tiles = el("div", { class: "rc-tiles" });
      var byCat = {};
      live().filter(function (i) { return i.tier === "safe" && inMode(i); }).forEach(function (i) { byCat[i.cat] = (byCat[i.cat] || 0) + i.gb; });
      Object.keys(byCat).sort(function (a, b) { return byCat[b] - byCat[a]; }).forEach(function (c) {
        tiles.appendChild(el("div", { class: "rc-tile" }, [icon(CATS[c].icon, "cat", CATS[c].color), el("span", { text: CATS[c].label }), el("span", { class: "sec num", text: fmt(byCat[c]) })]));
      });
      col.appendChild(el("section", { class: "rc" }, [
        el("div", { class: "rc-top" }, [
          el("div", null, [
            el("h3", { text: "Ready to clean · " + fmt(safe) }),
            el("p", { class: "sec", text: "These come back by themselves when they're needed. You choose what goes, and it moves to the Trash." })
          ]),
          el("button", { type: "button", class: "mbtn primary lg", id: "rc-review", text: "Review and clean…", onclick: function () { showScreen("cleanup", { tier: "safe" }); } })
        ]),
        tiles
      ]));
    }

    var groups = live().filter(function (i) { return i.tier === "review" && inMode(i); }).sort(function (a, b) { return b.gb - a.gb; }).slice(0, 4);
    var list = el("div", { class: "wal-list" });
    groups.forEach(function (i) {
      list.appendChild(el("button", { type: "button", class: "wal-row", title: i.explain, onclick: function () { showScreen("cleanup", { tier: "review" }); state.selected = i.id; renderWindow(); } }, [
        icon(CATS[i.cat].icon, "cat", CATS[i.cat].color),
        el("span", { class: "wal-t" }, [i.title, state.mode === "developer" && i.place ? el("span", { class: "sec", text: " · " + i.place }) : null]),
        el("span", { class: "sec num", text: fmt(i.gb) }),
        icon("chevron", "chev")
      ]));
    });
    col.appendChild(el("section", { class: "wal" }, [
      el("div", { class: "wal-head" }, [
        el("h3", { text: "Worth a look" }),
        review > 0 ? el("span", { class: "sec", text: "+" + fmt(review) + " to check first" }) : null,
        el("button", { type: "button", class: "link", text: "Show all", onclick: function () { showScreen("cleanup", { tier: "review" }); } })
      ]),
      groups.length ? list : el("p", { class: "sec", text: "Nothing else here takes much space." })
    ]));
    col.appendChild(el("div", { class: "ov-foot sec" }, [
      el("span", { text: "Scanned today at " + clockText(false) }),
      el("span", { text: "Roomy could open every folder." })
    ]));
    return col;
  }

  // The disk as one thin bar by category, the three biggest parts named underneath.
  function diskBar() {
    var parts = [];
    var byCat = {};
    live().forEach(function (i) { byCat[i.cat] = (byCat[i.cat] || 0) + i.gb; });
    Object.keys(byCat).forEach(function (c) { parts.push({ title: CATS[c].label, gb: byCat[c], color: CATS[c].color }); });
    parts.push({ title: "Other files", gb: OTHER_GB, color: "var(--c-brown)" });
    parts.push({ title: "Apps and macOS", gb: OUTSIDE_GB, color: "var(--win-quiet)" });
    var strip = el("div", { class: "strip", role: "img", "aria-label": "Disk usage by category" });
    parts.forEach(function (p) {
      var s = el("span", { title: p.title + " " + fmt(p.gb) });
      s.style.flexBasis = (p.gb / DISK_GB * 100) + "%";
      s.style.background = p.color;
      strip.appendChild(s);
    });
    var top = parts.slice().sort(function (a, b) { return b.gb - a.gb; }).slice(0, 3);
    var labels = el("div", { class: "strip-labels sec" });
    top.forEach(function (p) {
      var dot = el("i"); dot.style.background = p.color;
      labels.appendChild(el("span", null, [dot, p.title + " " + fmt(p.gb)]));
    });
    labels.appendChild(el("button", { type: "button", class: "link", text: "Show breakdown", onclick: function () { showScreen("systemdata"); } }));
    return el("div", { class: "ovbar" }, [strip, labels]);
  }

  // The item list: Show / Sort controls, rows, the bar once something is ticked, and a detail panel.
  function listView() {
    var key = state.screen;
    var items = live().filter(function (i) { return inMode(i) && (key === "cleanup" || i.screens.indexOf(key) !== -1); });
    if (state.tierFilter) items = items.filter(function (i) { return i.tier === state.tierFilter; });
    items.sort(function (a, b) { return b.gb - a.gb; });

    var showSel = el("select", { id: "win-show", "aria-label": "Show", onchange: function (e) { state.tierFilter = e.target.value || null; renderWindow(); } }, [
      el("option", { value: "", text: "Everything" }), el("option", { value: "safe", text: "Likely safe" }), el("option", { value: "review", text: "Review first" })
    ]);
    showSel.value = state.tierFilter || "";
    var controls = el("div", { class: "lc" }, [
      el("label", { class: "sec", for: "win-show", text: "Show:" }), showSel,
      el("label", { class: "sec", for: "win-sort", text: "Sort by:" }),
      el("select", { id: "win-sort", "aria-label": "Sort by" }, [el("option", { text: "Largest first" })])
    ]);

    var rows = el("div", { class: "rows" });
    if (!items.length) rows.appendChild(el("p", { class: "sec empty", text: key === "cleanup" ? "Nothing left to look at." : "Nothing here takes much space on this Mac." }));
    items.forEach(function (i) {
      var lead;
      if (i.tier === "keep") lead = el("span", { class: "cb-space" });
      else {
        lead = el("input", { type: "checkbox", class: "mcb", id: "cb-" + i.id, "aria-label": "Choose " + i.title + (i.place ? ", " + i.place : ""),
          onclick: function (e) { e.stopPropagation(); },
          onchange: function (e) { state.touched = true; state.checked[i.id] = e.target.checked; state.selected = i.id; renderWindow(); } });
        lead.checked = !!state.checked[i.id];
      }
      rows.appendChild(el("div", { class: "irow" + (state.selected === i.id ? " sel" : ""), onclick: function () { state.selected = i.id; renderWindow(); } }, [
        lead,
        icon(CATS[i.cat].icon, "cat", CATS[i.cat].color),
        el("div", { class: "row-main" }, [
          el("div", { class: "row-title" }, [el("span", { text: i.title }), state.mode === "developer" && i.place ? el("span", { class: "place", text: i.place }) : null]),
          el("div", { class: "sec row-ex", text: i.explain })
        ]),
        el("span", { class: "num row-size", text: fmt(i.gb) }),
        tierLabel(i.tier)
      ]));
    });

    var left = el("div", { class: "list-left" }, [controls, rows]);
    var c = chosen();
    if (c.length) {
      left.appendChild(el("div", { class: "cbar" }, [
        el("span", { class: "num", text: c.length + (c.length === 1 ? " item" : " items") + " chosen, " + fmt(sum(c)) }),
        el("button", { type: "button", class: "mbtn primary", id: "win-review", text: "Review…", onclick: function () { state.sheet = "preview"; renderWindow(); } })
      ]));
    }
    left.appendChild(el("div", { class: "unmeasured sec", text: "Roomy measured every folder in this list." }));
    return el("div", { class: "list-wrap" }, [left, detailPanel(items)]);
  }

  function tierLabel(tier) {
    var t = TIER[tier];
    var pill = el("span", { class: "tier" }, [icon(t.icon, "tier-ico", t.tint), el("span", { text: t.label })]);
    pill.style.setProperty("--tint", t.tint);
    return pill;
  }

  function detailPanel(items) {
    var item = ITEMS.filter(function (i) { return i.id === state.selected; })[0];
    if (!item) return el("aside", { class: "detail empty" }, [el("p", { class: "sec", text: items.length ? "Select a row to see what it is and what removing it would mean." : "" })]);
    var what = item.tier === "keep" ? "Roomy keeps this. It isn't offered for cleanup." :
      item.tier === "safe" ? "Comes back by itself when it's needed. Moving it to the Trash is likely safe." :
      "Look at this first. It may take time to get back, or hold something you want.";
    return el("aside", { class: "detail" }, [
      icon(CATS[item.cat].icon, "cat big", CATS[item.cat].color),
      el("h3", { text: item.title }),
      item.place && state.mode === "developer" ? el("span", { class: "place", text: item.place }) : null,
      el("p", { class: "num big-size", text: fmt(item.gb) }),
      tierLabel(item.tier),
      el("p", { text: item.explain }),
      el("p", { class: "sec", text: what }),
      state.mode === "developer" ? el("p", { class: "mono sec small", text: (item.cmd ? "Runs: " : "") + item.path }) : null,
      item.changed ? el("p", { class: "sec small", text: "Changed " + item.changed }) : null
    ]);
  }

  function systemDataView() {
    var rows = [["Local Time Machine snapshots", 22.1, "Copies Time Machine keeps on this disk while the backup drive is away. macOS removes them when space runs low."],
      ["iPhone backups", 14.3, "Full backups of an iPhone or iPad."], ["App caches", 8.7, "Data apps keep to load faster."], ["Logs", 1.2, "Records apps and macOS write while they run."]];
    var list = el("div", { class: "wal-list" });
    rows.forEach(function (r) {
      list.appendChild(el("div", { class: "wal-row static" }, [icon("gear", "cat", "var(--c-gray)"), el("span", { class: "wal-t" }, [r[0], el("span", { class: "sec", text: " · " + r[2] })]), el("span", { class: "sec num", text: fmt(r[1]) })]));
    });
    return el("div", { class: "ov" }, [
      el("div", { class: "ov-head" }, [el("h1", { text: "System Data, explained" }), el("p", { class: "sec lg", text: "What macOS puts in one grey bar, in parts. Read only." })]),
      list
    ]);
  }

  function historyView() {
    var col = el("div", { class: "ov" }, [el("div", { class: "ov-head" }, [el("h1", { text: "History" }), el("p", { class: "sec lg", text: "Past clean-ups, newest first." })])]);
    if (!state.history.length) { col.appendChild(el("p", { class: "sec", text: "No clean-ups yet. Tick something on any list and press Review." })); return col; }
    var list = el("div", { class: "wal-list" });
    state.history.slice().reverse().forEach(function (h, idx) {
      list.appendChild(el("div", { class: "wal-row static" }, [icon("clock", "cat", "var(--win-secondary)"),
        el("span", { class: "wal-t" }, [h.when, el("span", { class: "sec", text: " · " + h.count + (h.count === 1 ? " item" : " items") + " to the Trash" })]),
        el("span", { class: "sec num", text: fmt(h.gb) }),
        idx === 0 && state.lastMoved.length ? el("button", { type: "button", class: "mbtn", text: "Put back", onclick: putBack }) : null]));
    });
    col.appendChild(list);
    return col;
  }

  function appsView() {
    return el("div", { class: "ov" }, [
      el("div", { class: "ov-head" }, [el("h1", { text: "Remove apps" }), el("p", { class: "sec lg", text: "Each app with the caches, support files and preferences it leaves behind." })]),
      el("p", { class: "sec", text: "Try it in the leftovers card further down this page." })
    ]);
  }

  // ---- the review sheet: a dry run, then Move to Trash, then Put back ----

  function renderSheet() {
    var host = $("win-sheet");
    host.textContent = "";
    host.hidden = !state.sheet;
    if (!state.sheet) return;
    var box;
    if (state.sheet === "preview") {
      var c = chosen();
      var ul = el("ul", { class: "sheet-list" });
      c.forEach(function (i) { ul.appendChild(el("li", null, [icon(CATS[i.cat].icon, "cat", CATS[i.cat].color), el("span", { text: i.title + (i.place && state.mode === "developer" ? " · " + i.place : "") }), el("span", { class: "sec num", text: fmt(i.gb) })])); });
      var hasCmd = c.some(function (i) { return i.cmd; });
      box = el("div", { class: "msheet", role: "dialog", "aria-modal": "true", "aria-label": "Review your clean-up" }, [
        el("h2", { text: "Move " + fmt(sum(c)) + " to the Trash?" }),
        el("p", { class: "sec", text: "Roomy checked each item again, just as a clean-up would. Nothing has been moved yet." + (hasCmd ? " Tool commands run only after you confirm." : "") }),
        ul,
        el("div", { class: "sheet-actions" }, [
          el("button", { type: "button", class: "mbtn", id: "sheet-cancel", text: "Cancel", onclick: closeSheet }),
          el("button", { type: "button", class: "mbtn primary", id: "sheet-go", text: "Move to Trash", onclick: moveToTrash })
        ])
      ]);
    } else {
      box = el("div", { class: "msheet", role: "dialog", "aria-modal": "true", "aria-label": "Clean-up finished" }, [
        el("h2", { text: fmt(sum(state.lastMoved)) + " moved to the Trash" }),
        el("p", { class: "sec", text: "Everything is in the Trash. Put back returns it to where it was." }),
        el("div", { class: "sheet-actions" }, [
          el("button", { type: "button", class: "mbtn", id: "sheet-putback", text: "Put back", onclick: putBack }),
          el("button", { type: "button", class: "mbtn primary", id: "sheet-done", text: "Done", onclick: closeSheet })
        ])
      ]);
    }
    host.appendChild(box);
    var focusables = box.querySelectorAll("button");
    if (focusables.length) focusables[focusables.length - 1].focus({ preventScroll: true });
  }

  function closeSheet() { state.sheet = null; renderWindow(); }

  function moveToTrash() {
    var c = chosen();
    if (!c.length) return closeSheet();
    c.forEach(function (i) { state.trashed[i.id] = true; state.checked[i.id] = false; });
    state.lastMoved = c;
    if (state.selected && state.trashed[state.selected]) state.selected = null;
    state.history.push({ when: "Today at " + clockText(false), count: c.length, gb: sum(c) });
    state.sheet = "done";
    renderWindow();
  }

  function putBack() {
    state.lastMoved.forEach(function (i) { delete state.trashed[i.id]; });
    state.lastMoved = [];
    state.sheet = null;
    renderWindow();
    toast("Put back. Everything is where it was.");
  }

  // Keyboard inside the sheet: Tab stays in it, Escape closes it.
  document.addEventListener("keydown", function (e) {
    if (!state.sheet) return;
    var box = $("win-sheet");
    var btns = box.querySelectorAll("button");
    if (e.key === "Escape") { e.preventDefault(); closeSheet(); }
    else if (e.key === "Tab" && btns.length) {
      e.preventDefault();
      var i = Array.prototype.indexOf.call(btns, document.activeElement);
      btns[(i + (e.shiftKey ? btns.length - 1 : 1)) % btns.length].focus();
    }
  });

  // ---- the desktop: menu bar panel, notification, Dock ----

  function openWindow(autoScan) {
    var win = $("win");
    var wasHidden = win.hidden;
    win.hidden = false;
    state.open = true;
    closePopover();
    $("notif").hidden = true;
    $("hint").hidden = true;
    $("dock-roomy").classList.add("running");
    if (state.phase === "idle" && autoScan !== false) startScan();
    else renderWindow();
    if (wasHidden) win.classList.add("opening");
    setTimeout(function () { win.classList.remove("opening"); }, 350);
  }
  function closeWindow() { state.sheet = null; renderSheet(); $("win").hidden = true; state.open = false; }

  function openPopover() {
    $("popover").hidden = false;
    $("mb-roomy").setAttribute("aria-expanded", "true");
    $("hint").hidden = true;
    $("notif").hidden = true;
    renderPopover();
  }
  function closePopover() {
    $("popover").hidden = true;
    $("mb-roomy").setAttribute("aria-expanded", "false");
  }

  // The menu bar panel (MenuBarPanel.swift): free space, "X ready", the gauge, Ready to free rows
  // you can tick, locked Review first rows, and "Review N items…" / "Open Roomy".
  function renderPopover() {
    var pop = $("popover");
    if (pop.hidden) return;
    pop.textContent = "";
    var free = freeGb(), safe = safeGb(), review = reviewGb();
    var used = DISK_GB - free;
    var gauge = el("div", { class: "pg" }, [
      seg((used - safe - review) / DISK_GB, "var(--sys-gray)"), seg(review / DISK_GB, "var(--sys-orange)"),
      seg(safe / DISK_GB, "var(--sys-green)"), seg(free / DISK_GB, "transparent")
    ]);
    function seg(f, color) { var s = el("span"); s.style.flexBasis = (f * 100) + "%"; s.style.background = color; return s; }
    pop.appendChild(el("div", { class: "pp-top" }, [
      el("div", { class: "pp-free" }, [el("span", { class: "num big", text: fmt(free) }), el("span", { class: "sec", text: "free" })]),
      safe > 0 ? el("span", { class: "pp-ready num", text: fmt(safe) + " ready" }) : null
    ]));
    if (free < 20) pop.appendChild(el("div", { class: "pp-warn" }, [icon("warn"), el("span", { text: "Your Mac is almost full." })]));
    pop.appendChild(gauge);
    pop.appendChild(el("div", { class: "pp-key sec" }, [
      key("var(--sys-green)", "Likely safe"), key("var(--sys-orange)", "Review first"), key("var(--sys-gray)", "Other")
    ]));
    function key(c, t) { var d = el("i"); d.style.background = c; return el("span", null, [d, t]); }
    if (review > 0) pop.appendChild(el("p", { class: "sec small", text: fmt(review) + " more to look at first" }));
    pop.appendChild(el("div", { class: "pp-head", text: "Ready to free" }));
    var rows = el("div", { class: "pp-rows" });
    live().filter(function (i) { return i.tier === "safe" && inMode(i); }).sort(function (a, b) { return b.gb - a.gb; }).slice(0, 4).forEach(function (i) {
      var cb = el("input", { type: "checkbox", class: "mcb", id: "pp-" + i.id, onchange: function (e) { state.popChecked[i.id] = e.target.checked; renderPopover(); } });
      cb.checked = !!state.popChecked[i.id];
      rows.appendChild(el("label", { class: "pp-row", for: "pp-" + i.id }, [cb, el("span", { class: "pp-t" }, [el("span", { text: i.title }), el("span", { class: "sec small", text: "Likely safe" + (i.changed ? " · changed " + i.changed : "") })]), el("span", { class: "num sec", text: fmt(i.gb) })]));
    });
    live().filter(function (i) { return i.tier === "review" && inMode(i); }).sort(function (a, b) { return b.gb - a.gb; }).slice(0, 1).forEach(function (i) {
      rows.appendChild(el("div", { class: "pp-row locked" }, [icon("tierKeep", "lock"), el("span", { class: "pp-t" }, [el("span", { text: i.title }), el("span", { class: "sec small", text: "Review first, in Roomy" })]), el("span", { class: "num sec", text: fmt(i.gb) })]));
    });
    pop.appendChild(rows);
    var ticked = live().filter(function (i) { return state.popChecked[i.id] && inMode(i); });
    pop.appendChild(el("button", { type: "button", class: "mbtn primary wide", id: "pop-review",
      text: ticked.length === 0 ? "Open Roomy" : ticked.length === 1 ? "Review 1 item…" : "Review " + ticked.length + " items…",
      onclick: function () {
        state.touched = true;
        var t = live().filter(function (i) { return state.popChecked[i.id] && inMode(i); });
        state.popChecked = {};
        if (!t.length) { openWindow(); return; }
        t.forEach(function (i) { state.checked[i.id] = true; });
        if (state.phase !== "ready") state.phase = "ready";
        state.screen = "cleanup"; state.tierFilter = null; state.selected = null;
        openWindow(false);
        state.sheet = "preview";
        renderWindow();
      } }));
    pop.appendChild(el("div", { class: "pp-foot sec small" }, [el("span", { text: state.phase === "ready" ? "Scanned just now" : "Scanned 2 hours ago" }), icon("ellipsis")]));
  }

  function updateTrash() {
    var n = Object.keys(state.trashed).length;
    var badge = $("trash-badge");
    badge.hidden = n === 0;
    badge.textContent = String(n);
    $("dock-trash").classList.toggle("full", n > 0);
    $("dock-trash").setAttribute("aria-label", n ? "Trash, " + n + " items" : "Trash, empty");
  }

  function clockText(withDay) {
    var d = new Date();
    var time = String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
    if (!withDay) return time;
    var day = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][d.getDay()];
    var mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][d.getMonth()];
    return day + " " + d.getDate() + " " + mon + "  " + time;
  }

  function wireDesktop() {
    var mb = $("mb-roomy");
    mb.addEventListener("click", function (e) {
      e.stopPropagation();
      state.touched = true;
      if ($("popover").hidden) openPopover(); else closePopover();
    });
    $("notif").addEventListener("click", function (e) { e.stopPropagation(); state.touched = true; openPopover(); });
    $("popover").addEventListener("click", function (e) { e.stopPropagation(); });
    document.addEventListener("click", function () { if (!$("popover").hidden) closePopover(); });
    document.addEventListener("keydown", function (e) { if (e.key === "Escape" && !$("popover").hidden) { closePopover(); mb.focus(); } });
    $("win-close").addEventListener("click", closeWindow);
    $("win-scan").addEventListener("click", function () { state.touched = true; startScan(); });
    $("win-mode").addEventListener("click", function () {
      state.mode = state.mode === "developer" ? "simple" : "developer";
      if (state.mode === "simple" && DEV_SCREENS[state.screen]) state.screen = "overview";
      renderWindow();
      toast(state.mode === "simple" ? "Simple mode: everyday things in plain words." : "Developer mode: toolchains and paths.");
    });
    $("dock-roomy").addEventListener("click", function () { state.touched = true; openWindow(); });
    $("dock-trash").addEventListener("click", function () {
      if (state.lastMoved.length) { openWindow(false); state.screen = "history"; renderWindow(); }
      else toast("The Trash is empty.");
    });
    function tick() { $("mb-clock").textContent = clockText(true); }
    tick();
    setInterval(tick, 30000);
  }

  var toastTimer;
  function toast(msg) {
    var t = $("toast");
    t.textContent = msg;
    t.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { t.hidden = true; }, 2600);
  }

  // ---- Feature cards ----

  function wireFeatures() {
    wireLowSpace();
    wireExplainers("guards");
    wireExplainers("sysdata");
    wireUninstaller();
    var review = $("panel-review");
    if (review) review.addEventListener("click", function () {
      state.touched = true;
      $("desktop").scrollIntoView({ block: "center" });
      if (state.phase === "idle") openWindow(); else { openWindow(false); showScreen("cleanup", { tier: "safe" }); }
    });
  }

  // Menu bar panel: drag free space down and the low-space note appears below 10 GB.
  function wireLowSpace() {
    var slider = $("free-slider"), out = $("free-out"), gauge = $("panel-gauge"), note = $("lowspace-note");
    if (!slider) return;
    function update() {
      var free = Number(slider.value);
      out.textContent = free.toFixed(1) + " GB free";
      gauge.style.width = ((DISK_GB - free) / DISK_GB * 100) + "%";
      note.hidden = free >= 10;
    }
    slider.addEventListener("input", update);
    update();
  }

  // Rows that open a short explanation when clicked.
  function wireExplainers(id) {
    var box = $(id);
    if (!box) return;
    Array.prototype.forEach.call(box.querySelectorAll("button[data-explain]"), function (b) {
      b.addEventListener("click", function () {
        var open = b.getAttribute("aria-expanded") === "true";
        Array.prototype.forEach.call(box.querySelectorAll("button[data-explain]"), function (o) { o.setAttribute("aria-expanded", "false"); });
        var text = $(id + "-text");
        if (open) { text.textContent = text.dataset.idle; return; }
        b.setAttribute("aria-expanded", "true");
        text.textContent = b.dataset.explain;
      });
    });
  }

  var APPS = {
    Figma: [["Figma.app", 0.52, "/Applications"], ["Caches", 2.4, "~/Library/Caches/com.figma.Desktop"], ["Support files", 0.31, "~/Library/Application Support/Figma"], ["Preferences", 0.00005, "~/Library/Preferences"]],
    Zoom: [["zoom.us.app", 0.41, "/Applications"], ["Caches", 0.62, "~/Library/Caches/us.zoom.xos"], ["Recordings folder", 0, "Not offered: it holds your recordings"], ["Logs", 0.09, "~/Library/Logs/zoom.us"]],
    Slack: [["Slack.app", 0.48, "/Applications"], ["Caches", 1.9, "~/Library/Application Support/Slack/Cache"], ["Support files", 0.74, "~/Library/Application Support/Slack"], ["Saved state", 0.002, "~/Library/Saved Application State"]]
  };
  function wireUninstaller() {
    var box = $("uninstall");
    if (!box) return;
    var list = $("uninstall-list"), btn = $("uninstall-btn"), current = "Figma", removed = false;
    function draw() {
      list.textContent = "";
      var total = 0;
      APPS[current].forEach(function (f) {
        var isApp = /\.app$/.test(f[0]);
        var offered = f[1] > 0 && !isApp;
        if (offered) total += f[1];
        var right = isApp ? "stays in Applications" : (f[1] > 0 ? fmt(f[1]) : "kept");
        list.appendChild(el("div", { class: "mini-row" + (removed && offered ? " gone" : "") }, [
          el("span", { class: "l" }, [el("span", { text: f[0] })]),
          el("span", { class: "r", text: removed && offered ? "in the Trash" : right })
        ]));
      });
      btn.textContent = removed ? "Put back" : "Move " + fmt(total) + " to the Trash";
    }
    Array.prototype.forEach.call(box.querySelectorAll("[data-app]"), function (chip) {
      chip.addEventListener("click", function () {
        current = chip.dataset.app; removed = false;
        Array.prototype.forEach.call(box.querySelectorAll("[data-app]"), function (c) { c.setAttribute("aria-pressed", String(c === chip)); });
        draw();
      });
    });
    btn.addEventListener("click", function () { removed = !removed; draw(); });
    draw();
  }

  // ⌘K palette demo
  var ACTIONS = [
    { label: "Scan now", key: "⌘R" },
    { label: "Review Likely safe items", key: "" },
    { label: "Show Docker", cat: "docker" },
    { label: "Show Xcode", cat: "xcode" },
    { label: "Show Android", cat: "android" },
    { label: "Show JavaScript projects", cat: "js" },
    { label: "Show AI models", cat: "ai" },
    { label: "Uninstall an app…", key: "" },
    { label: "Find duplicates", key: "" },
    { label: "Show large files", key: "" },
    { label: "Show History", key: "⌘Y" },
    { label: "Settings", key: "⌘," }
  ];
  function renderPalette() {
    var q = $("palette-input").value.trim().toLowerCase();
    var res = $("palette-results");
    res.textContent = "";
    var hits = ACTIONS.filter(function (a) { return !q || a.label.toLowerCase().indexOf(q) !== -1; }).slice(0, 5);
    if (!hits.length) { delete res.dataset.first; res.appendChild(el("li", { text: "No actions match." })); return; }
    hits.forEach(function (a) { res.appendChild(el("li", null, [el("span", { text: a.label }), a.key ? el("kbd", { text: a.key }) : el("kbd", { text: "↩" })])); });
    res.dataset.first = ACTIONS.indexOf(hits[0]);
  }
  function runPalette() {
    var first = $("palette-results").dataset.first;
    var a = first === undefined ? null : ACTIONS[Number(first)];
    if (!a) return;
    var screens = { docker: "docker", xcode: "xcode", android: "android", js: "javascript", ai: "appscaches" };
    state.touched = true;
    $("desktop").scrollIntoView({ block: "center" });
    if (a.label === "Scan now") {
      openWindow(false);
      startScan();
      toast("Scanning the example Mac above.");
    } else if (a.label === "Review Likely safe items") {
      if (state.phase === "idle") openWindow();
      else { openWindow(false); showScreen("cleanup", { tier: "safe" }); }
      toast("Tick the ones you want to go. Nothing is ticked for you.");
    } else if (a.cat) {
      if (a.cat === "ai" || state.mode === "developer" || !DEV_SCREENS[screens[a.cat]]) state.screen = screens[a.cat];
      if (state.phase === "idle") openWindow(); else { openWindow(false); showScreen(state.screen); }
      toast(a.label + " in the example Mac above.");
    } else {
      toast("In the app, this runs “" + a.label + "”.");
    }
  }

  // Duplicate picker: at most one copy can go, so one copy is always kept.
  function wireDupes() {
    var a = $("dupe-a"), b = $("dupe-b"), note = $("dupe-note");
    var names = { "dupe-a": "Desktop/Screen Recording.mov", "dupe-b": "Movies/Screen Recording.mov" };
    function update(changed) {
      var other = changed === a ? b : a;
      if (changed && changed.checked) other.checked = false;
      if (a.checked) note.textContent = "Keeping " + names["dupe-b"] + ".";
      else if (b.checked) note.textContent = "Keeping " + names["dupe-a"] + ".";
      else note.textContent = "Tick the copy you don't need. One copy is always kept.";
    }
    a.addEventListener("change", function () { update(a); });
    b.addEventListener("change", function () { update(b); });
    update(null);
  }



  // ---- The page around the Mac: reveals, the Mac tilting up, a short guided tour,
  // the statement lighting up, the five-step story and the card spotlight. ----

  var reduceMotion = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  function clamp01(v) { return v < 0 ? 0 : v > 1 ? 1 : v; }

  function wireReveals() {
    var els = document.querySelectorAll(".reveal");
    if (!("IntersectionObserver" in window) || reduceMotion) {
      Array.prototype.forEach.call(els, function (e) { e.classList.add("in"); });
      return;
    }
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) { if (en.isIntersecting) { en.target.classList.add("in"); io.unobserve(en.target); } });
    }, { rootMargin: "0px 0px -8% 0px" });
    Array.prototype.forEach.call(els, function (e) { io.observe(e); });
  }

  // Split the statement into words so they can light up one by one while scrolling.
  function splitWords(root) {
    Array.prototype.slice.call(root.childNodes).forEach(function (n) {
      if (n.nodeType === 3) {
        var frag = document.createDocumentFragment();
        n.textContent.split(/(\s+)/).forEach(function (part) {
          if (!part) return;
          if (/^\s+$/.test(part)) frag.appendChild(document.createTextNode(part));
          else frag.appendChild(el("span", { class: "w", text: part }));
        });
        root.replaceChild(frag, n);
      } else if (n.nodeType === 1) splitWords(n);
    });
  }

  function wireScrollEffects() {
    var mac = $("mac"), quote = document.querySelector(".big-quote");
    if (reduceMotion) return;
    var words = [];
    if (quote) { splitWords(quote); words = quote.querySelectorAll(".w"); }
    var ticking = false;
    function frame() {
      ticking = false;
      var vh = window.innerHeight;
      if (mac) {
        var r = mac.getBoundingClientRect();
        var p = clamp01((vh - r.top) / (vh * 0.7));
        if (p >= 0.999) mac.classList.add("flat");
        else {
          mac.classList.remove("flat");
          mac.style.setProperty("--tilt", (1 - p) * 24 + "deg");
          mac.style.setProperty("--sc", String(0.88 + 0.12 * p));
        }
      }
      if (words.length) {
        var q = quote.getBoundingClientRect();
        var prog = clamp01((vh * 0.8 - q.top) / (q.height + vh * 0.3));
        var lit = Math.round(prog * words.length);
        for (var i = 0; i < words.length; i++) words[i].style.setProperty("--o", i < lit ? "1" : "0.16");
      }
    }
    function onScroll() { if (!ticking) { ticking = true; requestAnimationFrame(frame); } }
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    frame();
  }

  // When the Mac first comes into view, play a short tour unless the visitor has already
  // touched it: the low-space note, then the menu bar panel, then the Roomy window.
  function wireTour() {
    var desk = $("desktop");
    function touch() { state.touched = true; }
    desk.addEventListener("pointerdown", touch, true);
    desk.addEventListener("keydown", touch, true);
    if (!("IntersectionObserver" in window)) return;
    var started = false;
    var io = new IntersectionObserver(function (entries) {
      if (started || !entries[0].isIntersecting) return;
      started = true; io.disconnect();
      var steps = [
        [600, function () { if (!state.open && $("popover").hidden) { $("notif").hidden = false; $("hint").hidden = true; } }],
        [2600, function () { if (!state.open) openPopover(); }],
        [5200, function () { if (!state.open) openWindow(); }]
      ];
      steps.forEach(function (s) { setTimeout(function () { if (!state.touched) s[1](); }, s[0]); });
    }, { threshold: 0.55 });
    io.observe(desk);
  }

  var STORY = [
    { free: 16.4, status: "Reading your disk. Nothing is changed." },
    { free: 16.4, status: "Every item labelled, in plain words." },
    { free: 16.4, status: "You ticked what you want gone: 57.6 GB." },
    { free: 16.4, status: "Checked again just now. Nothing changed." },
    { free: 74.0, status: "57.6 GB in the Trash. Put back any time." }
  ];
  function wireStory() {
    var disk = $("disk"), chapters = document.querySelectorAll(".chapter");
    if (!disk || !chapters.length) return;
    var current = -1, shown = 16.4, anim;
    function countTo(target) {
      cancelAnimationFrame(anim);
      var from = shown, t0 = performance.now(), dur = reduceMotion ? 0 : 900;
      (function stepFn(now) {
        var k = dur ? clamp01((now - t0) / dur) : 1;
        shown = from + (target - from) * (1 - Math.pow(1 - k, 3));
        $("disk-free").textContent = shown.toFixed(1) + " GB";
        if (k < 1) anim = requestAnimationFrame(stepFn);
      })(t0);
    }
    function go(i) {
      if (i === current) return;
      current = i;
      disk.dataset.step = String(i);
      Array.prototype.forEach.call(chapters, function (c) { c.classList.toggle("on", Number(c.dataset.step) === i); });
      $("disk-status").textContent = STORY[i].status;
      countTo(STORY[i].free);
    }
    go(0);
    if (!("IntersectionObserver" in window)) return;
    var io = new IntersectionObserver(function (entries) {
      entries.forEach(function (en) { if (en.isIntersecting) go(Number(en.target.dataset.step)); });
    }, { rootMargin: "-48% 0px -48% 0px" });
    Array.prototype.forEach.call(chapters, function (c) { io.observe(c); });
  }

  function wireSpotlight() {
    Array.prototype.forEach.call(document.querySelectorAll(".b-card, .feature"), function (card) {
      card.addEventListener("pointermove", function (e) {
        var r = card.getBoundingClientRect();
        card.style.setProperty("--mx", (e.clientX - r.left) + "px");
        card.style.setProperty("--my", (e.clientY - r.top) + "px");
      });
    });
  }

  // The floating header tucks away while scrolling down and comes back on the way up,
  // so it never sits over the example Mac's own menu bar.
  function wireHeader() {
    var header = document.querySelector(".site-header"), lastY = window.scrollY;
    window.addEventListener("scroll", function () {
      var y = window.scrollY;
      if (Math.abs(y - lastY) < 6) return;
      header.classList.toggle("tucked", y > lastY && y > 300 && !header.contains(document.activeElement));
      lastY = y;
    }, { passive: true });
  }

  function wireSite() {
    wireHeader();
    wireReveals();
    wireScrollEffects();
    wireTour();
    wireStory();
    wireSpotlight();
  }

  function start() {
    wireDesktop();
    wireSite();
    wireFeatures();
    var pi = $("palette-input");
    pi.addEventListener("input", renderPalette);
    pi.addEventListener("keydown", function (e) { if (e.key === "Enter") { e.preventDefault(); runPalette(); } });
    renderPalette();
    wireDupes();
    Array.prototype.forEach.call(document.querySelectorAll("[data-toast]"), function (btn) {
      btn.addEventListener("click", function () { toast(btn.dataset.toast); });
    });
    document.addEventListener("keydown", function (e) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); pi.focus(); }
    });
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
