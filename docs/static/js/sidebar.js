/**
 * unifly docs: mobile navigation drawer.
 *
 * Below the tablet breakpoint the nav bar hides its section links and shows
 * the hamburger. On pages with a section sidebar the hamburger slides that
 * sidebar in (it repeats the primary links at its top); on pages without
 * one, such as the homepage, it drops the primary links down as a panel.
 */

(() => {
  // Must match `@media (max-width: $bp-lg)` in sass/_responsive.scss.
  const COLLAPSED = window.matchMedia("(max-width: 1024px)");

  const init = () => {
    const toggle = document.getElementById("nav-hamburger");
    if (!toggle) return;

    const sidebar = document.getElementById("site-sidebar");
    const links = document.getElementById("nav-links");
    const drawer = sidebar || links;
    if (!drawer) return;

    const openClass = sidebar ? "site-sidebar--open" : "nav-links--open";
    const overlay = document.getElementById("nav-overlay");

    toggle.setAttribute("aria-controls", drawer.id);

    const isOpen = () => drawer.classList.contains(openClass);

    const syncHidden = () => {
      // The sidebar is a regular column on desktop and off-canvas when
      // collapsed; only hide it from assistive tech while it is off-canvas.
      if (sidebar) {
        sidebar.setAttribute(
          "aria-hidden",
          COLLAPSED.matches && !isOpen() ? "true" : "false",
        );
      }
    };

    const open = () => {
      if (!COLLAPSED.matches) return;
      drawer.classList.add(openClass);
      overlay?.classList.add("nav-overlay--visible");
      document.body.style.overflow = "hidden";
      toggle.setAttribute("aria-expanded", "true");
      syncHidden();
      drawer.querySelector("a")?.focus({ preventScroll: true });
    };

    const close = ({ restoreFocus = false } = {}) => {
      drawer.classList.remove(openClass);
      overlay?.classList.remove("nav-overlay--visible");
      document.body.style.overflow = "";
      toggle.setAttribute("aria-expanded", "false");
      syncHidden();
      if (restoreFocus) toggle.focus();
    };

    toggle.addEventListener("click", () => {
      if (isOpen()) {
        close();
      } else {
        open();
      }
    });

    overlay?.addEventListener("click", () => close());

    // Following a link inside the drawer navigates away; close it so the
    // back button does not return to an open drawer.
    drawer.addEventListener("click", (e) => {
      if (e.target.closest("a") && isOpen()) close();
    });

    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape" && isOpen()) {
        e.preventDefault();
        close({ restoreFocus: true });
      }
    });

    COLLAPSED.addEventListener("change", () => {
      if (!COLLAPSED.matches && isOpen()) close();
      syncHidden();
    });

    toggle.setAttribute("aria-expanded", "false");
    syncHidden();
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
