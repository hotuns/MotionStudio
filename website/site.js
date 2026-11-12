const buttons = document.querySelectorAll('[data-angle]');
buttons.forEach(button => button.addEventListener('click', () => {
  const angle = Number(button.dataset.angle);
  buttons.forEach(item => item.setAttribute('aria-pressed', String(item === button)));
  document.querySelector('#heading').style.transform = `rotate(${angle}deg)`;
  document.querySelector('#angle').replaceChildren(document.createTextNode(String(Math.abs(angle))), Object.assign(document.createElement('span'), {textContent:'°'}));
  const radians = angle * Math.PI / 180;
  document.querySelector('#turn-arc').setAttribute('d', `M200 35 A165 165 0 0 ${angle > 0 ? 1 : 0} ${200 + 165 * Math.sin(radians)} ${200 - 165 * Math.cos(radians)}`);
}));

// Keep the release page as a usable fallback if GitHub's API is unavailable.
fetch('https://api.github.com/repos/hotuns/MotionStudio/releases/latest', {signal:AbortSignal.timeout(6000)})
  .then(response => {if(!response.ok) throw new Error('Release unavailable'); return response.json();})
  .then(release => {
    if(release.draft || release.prerelease || !/^v\d+\.\d+\.\d+$/.test(release.tag_name)) return;
    const asset = release.assets?.find(item => /^MotionStudio-\d+\.\d+\.\d+-x64-setup\.exe$/.test(item.name));
    if(!asset) return;
    const url = new URL(asset.browser_download_url);
    if(url.origin !== 'https://github.com' || !url.pathname.startsWith('/hotuns/MotionStudio/releases/download/')) return;
    document.querySelector('#download-link').href = url.href;
    document.querySelector('#release-info').textContent = `${release.tag_name} · Windows x64 · ${(asset.size / 1024 / 1024).toFixed(1)} MB`;
  }).catch(() => {});
