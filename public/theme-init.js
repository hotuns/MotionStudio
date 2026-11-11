// Apply the saved theme before the app and its styles load. This same-origin
// script keeps startup compatible with the application's script CSP.
(() => {
  let theme = 'dark';
  try {
    if (localStorage.getItem('motion-studio-theme') === 'light') theme = 'light';
  } catch {
    // Storage may be unavailable; the default theme remains usable.
  }
  const root = document.documentElement;
  const background = theme === 'light' ? '#f0f1f3' : '#101210';
  root.dataset.theme = theme;
  // The CSS variable follows subsequent changes, including floating-window
  // session themes; the fallback covers the interval before CSS is available.
  root.style.backgroundColor = `var(--bg-0, ${background})`;
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', background);
})();
