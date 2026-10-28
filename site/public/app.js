// Progressive enhancement only: without this script the page still reads in
// full, and every command can be selected and copied by hand.

const announcer = document.querySelector("[data-announcer]");
const announce = (message) => {
  if (!announcer) return;
  announcer.textContent = "";
  requestAnimationFrame(() => (announcer.textContent = message));
};

// ---------- Copy buttons ----------

for (const root of document.querySelectorAll("[data-copy-root]")) {
  const button = root.querySelector(".copy");
  const code = root.querySelector("[data-copy-text]");
  let timer;
  button?.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(code.textContent.trim());
    } catch {
      // Clipboard blocked: select the text so ⌘C works.
      const range = document.createRange();
      range.selectNodeContents(code);
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
