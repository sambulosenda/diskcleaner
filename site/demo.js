// Roomy site demo. Everything here is example data; nothing reads the visitor's Mac.
(function () {
  "use strict";

  var DISK_GB = 512;

  // Bar segments, in GB. "used" values add up to 495.6 GB, leaving 16.4 GB free.
  var SEGMENTS = [
    { id: "xcode", label: "Xcode", color: "var(--seg-xcode)", gb: 58 },
    { id: "android", label: "Android", color: "var(--seg-android)", gb: 24 },
    { id: "js", label: "JavaScript", color: "var(--seg-js)", gb: 19 },
    { id: "docker", label: "Docker", color: "var(--seg-docker)", gb: 38 },
    { id: "ai", label: "AI models", color: "var(--seg-ai)", gb: 52 },
    { id: "files", label: "Your files", color: "var(--seg-files)", gb: 160 },
    { id: "system", label: "Apps and macOS", color: "var(--seg-system)", gb: 144.6 }
  ];

  // tier: safe (Likely safe), review (Review first), keep (never offered), skip (skipped this time)
  var ITEMS = [
    { id: "x1", cat: "xcode", seg: "xcode", tier: "safe", gb: 18.4, name: "Xcode DerivedData", why: "Build files Xcode makes again the next time you build.", path: "~/Library/Developer/Xcode/DerivedData" },
    { id: "x2", cat: "xcode", seg: "xcode", tier: "safe", gb: 9.6, name: "Unavailable simulators", why: "Simulators for iOS versions that are no longer installed.", path: "xcrun simctl delete unavailable", cmd: true },
    { id: "x3", cat: "xcode", seg: "xcode", tier: "safe", gb: 6.3, name: "iOS 16 device support", why: "Debug files for an iPhone version you have not tested on in a year.", path: "~/Library/Developer/Xcode/iOS DeviceSupport/16.4" },
    { id: "a1", cat: "android", seg: "android", tier: "review", gb: 6.1, name: "Android 11 system image (API 30)", why: "No project on this Mac uses API 30. Android Studio can download it again.", path: "~/Library/Android/sdk/system-images/android-30" },
    { id: "a2", cat: "android", seg: "android", tier: "safe", gb: 4.8, name: "Gradle caches", why: "Downloaded build files. Gradle fetches them again when a build needs them.", path: "~/.gradle/caches" },
    { id: "j1", cat: "js", seg: "js", tier: "safe", gb: 5.8, name: "node_modules in 6 old projects", why: "Last worked on more than 3 months ago. npm install brings them back.", path: "~/Developer/*/node_modules" },
    { id: "j2", cat: "js", seg: "js", tier: "skip", gb: 0.9, name: "node_modules in roomy-site", why: "Skipped: changed 2 hours ago.", path: "~/Developer/roomy-site/node_modules" },
    { id: "d1", cat: "docker", seg: "docker", tier: "safe", gb: 7.2, name: "Docker build cache", why: "Cached build steps. Docker rebuilds them when needed.", path: "docker builder prune", cmd: true },
    { id: "d2", cat: "docker", seg: "docker", tier: "review", gb: 11.4, name: "Images no container uses", why: "Pulling them again takes time on a slow connection.", path: "docker image prune", cmd: true },
    { id: "d3", cat: "docker", seg: "docker", tier: "keep", gb: 12.0, name: "Docker volumes", why: "These hold your containers' data. Roomy never removes them.", path: "docker volume ls" },
    { id: "ai1", cat: "ai", seg: "ai", tier: "review", gb: 39.6, name: "Ollama model llama3.1:70b", why: "Last used 3 months ago. Takes hours to download again.", path: "~/.ollama/models" },
    { id: "ai2", cat: "ai", seg: "ai", tier: "review", gb: 8.1, name: "LM Studio model qwen2.5-14b", why: "Last used 6 weeks ago.", path: "~/.lmstudio/models" },
    { id: "h1", cat: "brew", seg: "system", tier: "safe", gb: 2.3, name: "Old Homebrew versions", why: "Older copies of tools Homebrew has already updated.", path: "brew cleanup", cmd: true },
    { id: "f1", cat: "files", seg: "files", tier: "review", gb: 14.3, name: "Backup of Sam's old iPhone", why: "Last backed up 14 months ago. Check you have a newer backup first.", path: "~/Library/Application Support/MobileSync/Backup" },
    { id: "f2", cat: "files", seg: "files", tier: "review", gb: 3.9, name: "Installers in Downloads", why: "12 .dmg and .pkg files for apps that are already installed.", path: "~/Downloads" },
    { id: "f3", cat: "files", seg: "files", tier: "review", gb: 1.7, name: "Copy of Screen Recording.mov", why: "The same file is also in Movies. You pick the copy to keep.", path: "~/Desktop/Screen Recording.mov" },
    { id: "f4", cat: "files", seg: "files", tier: "review", gb: 6.8, name: "Messages attachments", why: "Photos and videos from chats, mostly from 2023. Open Messages to keep the ones you want.", path: "~/Library/Messages/Attachments" },
    { id: "c1", cat: "caches", seg: "system", tier: "safe", gb: 3.2, name: "Browser and app caches", why: "Copies of web pages and app data. Apps make them again as you use them.", path: "~/Library/Caches" }
  ];

  var CATS = [
    { id: "all", label: "Overview" },
    { group: "Developer" },
    { id: "xcode", label: "Xcode" },
    { id: "android", label: "Android" },
    { id: "js", label: "JavaScript" },
    { id: "docker", label: "Docker" },
    { id: "ai", label: "AI models" },
    { id: "brew", label: "Homebrew" },
    { group: "Everyday" },
    { id: "files", label: "Your files" },
    { id: "caches", label: "Caches" }
  ];
  // Simple mode shows only everyday things, in plain words, without paths or commands.
  var SIMPLE_CATS = { all: true, files: true, caches: true };
  var STEP_CAPTIONS = {
    scan: "Roomy reads the disk. Scanning never changes a file.",
    read: "Each item says what it is, whether it is likely safe, and how it comes back.",
    choose: "Tick what goes. Nothing is ticked for you, so tick a few boxes in the window.",
    trash: "Everything goes to the Trash. Put back returns it."
  };

  var TIER_LABEL = { safe: "Likely safe", review: "Review first", keep: "Keep", skip: "Skipped" };
  var SCAN_PATHS = [
    "~/Library/Developer/Xcode/DerivedData", "~/Library/Developer/CoreSimulator/Devices",
    "~/Library/Android/sdk/system-images", "~/.gradle/caches", "~/Developer/shop-app/node_modules",
    "~/Library/Containers/com.docker.docker", "~/.ollama/models", "~/.lmstudio/models",
    "/opt/homebrew/Cellar", "~/Library/Application Support/MobileSync", "~/Downloads", "~/Movies"
  ];

  var state = {
    phase: "idle", // idle | scanning | results | done
    cat: "all",
    mode: "developer", // developer | simple
    touched: false, // true once the visitor has used the demo, so autoplay stays out of the way
    checked: {},
    trashed: {}, // id -> true, items moved in the last clean-up
    lastFreed: 0
  };

  var $ = function (id) { return document.getElementById(id); };
  var stage = $("stage");

  function fmt(gb) {
    if (gb < 1) return Math.round(gb * 1000) + " MB";
    return gb.toFixed(1) + " GB";
  }
  function el(tag, attrs, children) {
    var n = document.createElement(tag);
    if (attrs) Object.keys(attrs).forEach(function (k) {
      if (k === "text") n.textContent = attrs[k];
      else if (k === "class") n.className = attrs[k];
      else n.setAttribute(k, attrs[k]);
    });
    (children || []).forEach(function (c) { if (c) n.appendChild(typeof c === "string" ? document.createTextNode(c) : c); });
    return n;
  }
  function trashedGbFor(segId) {
    return ITEMS.reduce(function (s, i) { return s + (state.trashed[i.id] && i.seg === segId ? i.gb : 0); }, 0);
  }
  function freeGb() {
    var used = SEGMENTS.reduce(function (s, g) { return s + g.gb - trashedGbFor(g.id); }, 0);
    return DISK_GB - used;
  }
  function visibleItems() {
    return ITEMS.filter(function (i) { return !state.trashed[i.id] && inMode(i) && (state.cat === "all" || i.cat === state.cat); });
  }
  function inMode(item) { return state.mode === "developer" || SIMPLE_CATS[item.cat]; }
  function chosen() { return ITEMS.filter(function (i) { return state.checked[i.id] && !state.trashed[i.id] && inMode(i); }); }
  function sumGb(list) { return list.reduce(function (s, i) { return s + i.gb; }, 0); }

  // Disk bar: during a scan it fills up as `progress` goes 0 → 1.
  function renderBar(progress) {
    var bar = $("diskbar");
    if (!bar.children.length) {
      SEGMENTS.forEach(function (g) {
        var s = el("span", { title: g.label });
        s.style.background = g.color;
        bar.appendChild(s);
      });
      var legend = $("legend");
      SEGMENTS.forEach(function (g) {
        var i = el("i"); i.style.background = g.color;
        legend.appendChild(el("span", null, [i, g.label]));
      });
    }
    var p = progress == null ? 1 : progress;
    SEGMENTS.forEach(function (g, idx) {
      var gb = (g.gb - trashedGbFor(g.id)) * p;
      bar.children[idx].style.flexBasis = (gb / DISK_GB * 100) + "%";
    });
    $("free-label").textContent = progress == null || progress >= 1 ? fmt(freeGb()) + " free" : "Scanning…";
  }

  function renderSidebar() {
    var sb = $("sidebar");
    sb.textContent = "";
    CATS.forEach(function (c) {
      if (state.mode === "simple" && (c.group || !SIMPLE_CATS[c.id])) return;
      if (c.group) { sb.appendChild(el("div", { class: "group", text: c.group })); return; }
      var items = ITEMS.filter(function (i) { return !state.trashed[i.id] && inMode(i) && (c.id === "all" || i.cat === c.id) && (i.tier === "safe" || i.tier === "review"); });
      var count = state.phase === "results" || state.phase === "done" ? fmt(sumGb(items)) : "";
      var b = el("button", { type: "button", "aria-pressed": String(state.cat === c.id) }, [el("span", { text: c.label }), el("span", { class: "count", text: count })]);
      b.addEventListener("click", function () {
        state.touched = true;
        state.cat = c.id;
        if (state.phase === "done") state.phase = "results";
        render();
      });
      sb.appendChild(b);
    });
  }

  function renderSteps() {
    var order = ["scan", "read", "choose", "trash"];
    var current = "scan";
    if (state.phase === "results") current = chosen().length ? "choose" : "read";
    if (state.phase === "done") current = "trash";
    var ci = order.indexOf(current);
    Array.prototype.forEach.call($("steps").children, function (li) {
      var i = order.indexOf(li.dataset.step);
      li.classList.toggle("done", i < ci);
      if (i === ci) li.setAttribute("aria-current", "step"); else li.removeAttribute("aria-current");
    });
    $("step-caption").textContent = STEP_CAPTIONS[current];
  }

  function renderIdle() {
    stage.textContent = "";
    var btn = el("button", { class: "btn btn-primary", type: "button", id: "scan-btn", text: "Scan this example Mac" });
    btn.addEventListener("click", function () { state.touched = true; startScan(); });
    stage.appendChild(el("div", { class: "scan-cta" }, [
      el("h3", { text: "Only 16.4 GB left. What is using the rest?" }),
      el("p", { text: "This is a made-up Mac belonging to a developer. Scan it to see what Roomy finds and how it explains each item." }),
      btn
    ]));
  }

  function startScan() {
    if (state.phase === "scanning") return;
    state.phase = "scanning";
    renderSteps();
    stage.textContent = "";
    var bar = el("span");
    var pathEl = el("div", { class: "scan-path" });
    stage.appendChild(el("div", { class: "scan-cta" }, [
      el("h3", { text: "Scanning…" }),
      el("div", { class: "scan-progress" }, [bar]),
      pathEl
    ]));
    var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    var total = reduce ? 1 : 26, n = 0;
    renderBar(0);
    var t = setInterval(function () {
      n++;
      var p = n / total;
      bar.style.width = (p * 100) + "%";
      pathEl.textContent = SCAN_PATHS[n % SCAN_PATHS.length];
      renderBar(p);
      if (n >= total) {
        clearInterval(t);
        state.phase = "results";
        render();
      }
    }, 80);
  }

  function rowFor(item) {
    var offerable = item.tier === "safe" || item.tier === "review";
    var lead;
    if (offerable) {
      lead = el("input", { type: "checkbox", id: "demo-" + item.id, "aria-label": "Choose " + item.name });
      lead.checked = !!state.checked[item.id];
      lead.addEventListener("change", function () { state.touched = true; state.checked[item.id] = lead.checked; renderFooter(); renderSteps(); });
    } else {
      lead = el("span", { class: "lock", "aria-hidden": "true", text: "🔒" });
    }
    var pillClass = item.tier === "skip" ? "keep" : item.tier;
    return el("div", { class: "row" + (item.tier === "skip" ? " skipped" : "") + (item.tier === "keep" ? " kept" : "") }, [
      lead,
      el("div", null, [
        offerable ? el("label", { for: "demo-" + item.id, class: "name", text: item.name }) : el("div", { class: "name", text: item.name }),
        el("div", { class: "why", text: item.why }),
        state.mode === "developer" ? el("div", { class: "path", text: (item.cmd ? "Runs: " : "") + item.path }) : null
      ]),
      el("div", { class: "meta" }, [el("span", { class: "size", text: fmt(item.gb) }), el("span", { class: "pill " + pillClass, text: TIER_LABEL[item.tier] })])
    ]);
  }

  function renderResults() {
    stage.textContent = "";
    if (state.phase === "done") {
      var undo = el("button", { class: "btn btn-ghost btn-small", type: "button", text: "Put back" });
      undo.addEventListener("click", putBack);
      stage.appendChild(el("div", { class: "done-banner", role: "status" }, [
        el("span", null, [el("strong", { text: fmt(state.lastFreed) + " freed. " }), "Everything went to the Trash."]),
        undo
      ]));
    }
    var items = visibleItems();
    var order = { safe: 0, review: 1, skip: 2, keep: 3 };
    items.sort(function (a, b) { return order[a.tier] - order[b.tier] || b.gb - a.gb; });
    if (!items.length) {
      stage.appendChild(el("div", { class: "scan-cta" }, [el("p", { text: "Nothing left to look at here." })]));
      return;
    }
    var list = el("div", { class: "list" });
    items.forEach(function (i) { list.appendChild(rowFor(i)); });
    stage.appendChild(list);
  }

  function renderFooter() {
    var bb = $("bottombar");
    bb.hidden = !(state.phase === "results" || state.phase === "done");
    var c = chosen();
    $("chosen").textContent = "";
    $("chosen").appendChild(el("strong", { text: c.length + " chosen" }));
    $("chosen").appendChild(document.createTextNode(c.length ? ", " + fmt(sumGb(c)) : ""));
    $("review-btn").disabled = c.length === 0;
  }

  function openSheet() {
    var c = chosen();
    if (!c.length) return;
    var hasCmd = c.some(function (i) { return i.cmd; });
    var ul = el("ul");
    c.forEach(function (i) { ul.appendChild(el("li", null, [el("span", { text: i.name }), el("span", { text: fmt(i.gb) })])); });
    var cancel = el("button", { class: "btn btn-ghost btn-small", type: "button", text: "Cancel" });
    var go = el("button", { class: "btn btn-primary btn-small", type: "button", text: "Move to Trash" });
    var backdrop = el("div", { class: "sheet-backdrop" }, [
      el("div", { class: "sheet", role: "dialog", "aria-modal": "true", "aria-label": "Review your clean-up" }, [
        el("h3", { text: "Free " + fmt(sumGb(c)) + "?" }),
        ul,
        el("p", { class: "note", text: "Roomy checks each item again just before it acts, and skips anything that has changed." + (hasCmd ? " Tool commands run only after you confirm, and show what they will remove first." : "") }),
        el("div", { class: "sheet-actions" }, [cancel, go])
      ])
    ]);
    function close() { backdrop.remove(); $("review-btn").focus(); }
    cancel.addEventListener("click", close);
    backdrop.addEventListener("keydown", function (e) { if (e.key === "Escape") close(); });
    go.addEventListener("click", function () {
      state.lastFreed = sumGb(c);
      state.trashed = {};
      c.forEach(function (i) { state.trashed[i.id] = true; state.checked[i.id] = false; });
      state.phase = "done";
      backdrop.remove();
      render();
    });
    $("window").appendChild(backdrop);
    go.focus();
  }

  function putBack(quiet) {
    Object.keys(state.trashed).forEach(function (id) { state.checked[id] = false; });
    state.trashed = {};
    state.phase = "results";
    render();
    if (quiet !== true) toast("Put back. Everything is where it was.");
  }

  function render() {
    renderPopover();
    updateTrash();
    renderSteps();
    renderSidebar();
    renderBar();
    if (state.phase === "idle") renderIdle();
    else if (state.phase === "results" || state.phase === "done") renderResults();
    renderFooter();
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
      $("desktop").scrollIntoView({ block: "center" });
      goToStep("read");
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
    if (!hits.length) { res.appendChild(el("li", { text: "No actions match." })); return; }
    hits.forEach(function (a) { res.appendChild(el("li", null, [el("span", { text: a.label }), a.key ? el("kbd", { text: a.key }) : el("kbd", { text: "↩" })])); });
    res.dataset.first = ACTIONS.indexOf(hits[0]);
  }
  function runPalette() {
    var a = ACTIONS[Number($("palette-results").dataset.first)];
    if (!a) return;
    if (a.cat) {
      state.cat = a.cat;
      openWindow(false);
      if (state.phase === "idle") { render(); startScan(); } else { if (state.phase === "done") state.phase = "results"; render(); }
      $("desktop").scrollIntoView({ block: "center" });
      toast(a.label + " in the demo above.");
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

  // Clicking a step jumps the demo to that point.
  function goToStep(step) {
    state.touched = true;
    openWindow(false);
    if (step === "scan") { state.trashed = {}; state.checked = {}; state.cat = "all"; startScan(); return; }
    if (state.phase === "idle" || state.phase === "scanning") { state.phase = "results"; }
    if (step === "read") { if (state.phase === "done") putBack(true); state.checked = {}; render(); return; }
    if (step === "choose") { if (state.phase === "done") putBack(true); render(); pulseBoxes(); return; }
    if (step === "trash") {
      if (state.phase === "done") return;
      render();
      if (!chosen().length) { toast("Tick at least one item first. Nothing is ticked for you."); pulseBoxes(); return; }
      openSheet();
    }
  }

  // Draw the eye to the checkboxes, since the visitor does the ticking.
  function pulseBoxes() {
    var boxes = document.querySelectorAll("#stage .row input[type=checkbox]");
    Array.prototype.forEach.call(boxes, function (b) {
      b.classList.remove("pulse");
      void b.offsetWidth;
      b.classList.add("pulse");
    });
  }

  function setMode(mode) {
    state.touched = true;
    state.mode = mode;
    state.cat = "all";
    Array.prototype.forEach.call(document.querySelectorAll("[data-mode]"), function (b) {
      b.setAttribute("aria-pressed", String(b.dataset.mode === mode));
    });
    if (state.phase === "done") state.phase = "results";
    render();
  }

  // ---- The example desktop in the hero ----

  // Show the Roomy window. The first time, it starts a scan by itself.
  function openWindow(autoScan) {
    var win = $("window");
    var wasHidden = win.hidden;
    win.hidden = false;
    closePopover();
    $("notif").hidden = true;
    if (wasHidden) $("win-close").focus({ preventScroll: true });
    if (autoScan !== false && state.phase === "idle") startScan();
  }
  function closeWindow() {
    $("window").hidden = true;
    $("dock-roomy").focus({ preventScroll: true });
  }

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

  // The menu bar panel, as in Roomy: free space, a gauge, and what is ready to free.
  function renderPopover() {
    var free = freeGb();
    $("pop-free").textContent = fmt(free) + " free";
    $("pop-gauge").style.width = ((DISK_GB - free) / DISK_GB * 100) + "%";
    var list = $("pop-list");
    list.textContent = "";
    var safe = ITEMS.filter(function (i) { return i.tier === "safe" && !state.trashed[i.id]; })
      .sort(function (x, y) { return y.gb - x.gb; });
    var changed = { x1: "changed 3 days ago", x2: "changed 5 months ago", d1: "changed 2 weeks ago" };
    safe.slice(0, 3).forEach(function (i) {
      list.appendChild(el("div", { class: "pop-row" }, [
        el("span", { class: "n", text: i.name }), el("span", { class: "s", text: fmt(i.gb) }),
        el("span", { class: "w", text: "Likely safe" + (changed[i.id] ? " · " + changed[i.id] : "") })
      ]));
    });
    var review = ITEMS.filter(function (i) { return i.tier === "review" && !state.trashed[i.id]; })
      .sort(function (x, y) { return y.gb - x.gb; })[0];
    if (review) {
      list.appendChild(el("div", { class: "pop-row locked" }, [
        el("span", { class: "n", text: "🔒 " + review.name }), el("span", { class: "s", text: fmt(review.gb) }),
        el("span", { class: "w", text: "Review first, in Roomy" })
      ]));
    }
    if (!safe.length) list.appendChild(el("div", { class: "pop-row" }, [el("span", { class: "n", text: "Nothing likely safe left." })]));
  }

  function updateTrash() {
    var n = Object.keys(state.trashed).length;
    var badge = $("trash-badge");
    badge.hidden = n === 0;
    badge.textContent = String(n);
    $("dock-trash").setAttribute("aria-label", n ? "Trash, " + n + " items" : "Trash, empty");
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
    $("pop-review").addEventListener("click", function () {
      state.touched = true;
      openWindow(false);
      if (state.phase === "idle" || state.phase === "scanning") { state.phase = "results"; }
      if (state.phase === "done") state.phase = "results";
      state.cat = "all";
      render();
      pulseBoxes();
    });
    $("pop-scan").addEventListener("click", function () { state.touched = true; openWindow(false); startScan(); });
    $("win-close").addEventListener("click", closeWindow);
    $("dock-roomy").addEventListener("click", function () { state.touched = true; openWindow(); });
    $("dock-trash").addEventListener("click", function () {
      if (Object.keys(state.trashed).length) { openWindow(false); if (state.phase !== "done") { state.phase = "done"; render(); } }
      else toast("The Trash is empty.");
    });
    // Live clock in the menu bar.
    function tick() {
      var d = new Date();
      var day = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][d.getDay()];
      $("mb-clock").textContent = day + " " + String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
    }
    tick();
    setInterval(tick, 30000);
    // The low-space note slides in shortly after the page opens.
    setTimeout(function () { if (!state.touched && $("popover").hidden && $("window").hidden) $("notif").hidden = false; }, 900);
  }

  function start() {
    render();
    Array.prototype.forEach.call($("steps").querySelectorAll("button"), function (b) {
      b.addEventListener("click", function () { goToStep(b.parentNode.dataset.step); });
    });
    Array.prototype.forEach.call(document.querySelectorAll("[data-mode]"), function (b) {
      b.addEventListener("click", function () { setMode(b.dataset.mode); });
    });
    wireDesktop();
    wireFeatures();
    $("review-btn").addEventListener("click", openSheet);
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
