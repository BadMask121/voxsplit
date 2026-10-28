// Progressive enhancement only: without this script every panel is visible
// and every command can still be selected and copied by hand.

const announcer = document.querySelector("[data-announcer]");
const announce = (message) => {
  if (!announcer) return;
  announcer.textContent = "";
  requestAnimationFrame(() => (announcer.textContent = message));
};

// ---------- Copy buttons ----------

for (const root of document.querySelectorAll("[data-copy-root]")) {
  const button = root.querySelector(".copy");
  const text = root.querySelector("[data-copy-text]")?.textContent.trim();
  let timer;
  button?.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard blocked (e.g. insecure context): select it so ⌘C works.
      const range = document.createRange();
      range.selectNodeContents(root.querySelector("[data-copy-text]"));
      getSelection().removeAllRanges();
      getSelection().addRange(range);
      return;
    }
    button.setAttribute("data-copied", "");
    announce("Copied");
    clearTimeout(timer);
    timer = setTimeout(() => button.removeAttribute("data-copied"), 1600);
  });
}

// Commands that don't fit scroll sideways; mark them so the cut edge fades.
const overflowObserver = new ResizeObserver((entries) => {
  for (const { target } of entries) markOverflow(target);
});
function markOverflow(code) {
  const root = code.closest("[data-copy-root]");
  root.toggleAttribute("data-overflow", code.scrollWidth > code.clientWidth + 1);
  root.toggleAttribute("data-end", code.scrollLeft + code.clientWidth >= code.scrollWidth - 1);
}
for (const code of document.querySelectorAll("[data-copy-root] code")) {
  overflowObserver.observe(code);
  code.addEventListener("scroll", () => markOverflow(code), { passive: true });
}

// ---------- Setup tabs ----------

const tablist = document.querySelector(".tabs");
if (tablist) {
  const tabs = [...tablist.querySelectorAll('[role="tab"]')];
  const pill = tablist.querySelector(".tab-pill");

  const place = (tab) => {
    pill.style.setProperty("--pill-x", `${tab.offsetLeft}px`);
    pill.style.setProperty("--pill-w", `${tab.offsetWidth}px`);
  };

  // Keyboard switches are instant: animation would make them feel slow.
  const select = (tab, { focus = false, instant = false, remember = true } = {}) => {
    for (const t of tabs) {
      const on = t === tab;
      t.setAttribute("aria-selected", String(on));
      t.tabIndex = on ? 0 : -1;
      document.getElementById(t.getAttribute("aria-controls")).hidden = !on;
    }
    tablist.toggleAttribute("data-instant", instant);
    place(tab);
    if (focus) tab.focus();
    // A shareable link to the open tab, once the reader picks one.
    if (remember) history.replaceState(null, "", `#${tab.id.replace("tab-", "setup-")}`);
  };

  tabs.forEach((tab, i) => {
    tab.addEventListener("click", () => select(tab));
    tab.addEventListener("keydown", (event) => {
      const step = { ArrowRight: 1, ArrowLeft: -1 }[event.key];
      if (step) {
        event.preventDefault();
        select(tabs[(i + step + tabs.length) % tabs.length], { focus: true, instant: true });
      } else if (event.key === "Home" || event.key === "End") {
        event.preventDefault();
        select(event.key === "Home" ? tabs[0] : tabs.at(-1), { focus: true, instant: true });
      }
    });
  });

  // Deep links (#setup-cli) open the matching tab.
  const fromHash = tabs.find((t) => location.hash === `#${t.id.replace("tab-", "setup-")}`);
  const initial = fromHash ?? tabs[0];
  for (const t of tabs) document.getElementById(t.getAttribute("aria-controls")).hidden = t !== initial;
  tablist.setAttribute("data-instant", "");
  select(initial, { instant: true, remember: false });
  if (fromHash) document.getElementById("setup").scrollIntoView();
  requestAnimationFrame(() => tablist.setAttribute("data-ready", ""));

  // Widths change with the font loading and on resize; keep the pill on its tab.
  new ResizeObserver(() => {
    tablist.setAttribute("data-instant", "");
    place(tabs.find((t) => t.getAttribute("aria-selected") === "true"));
  }).observe(tablist);
}
