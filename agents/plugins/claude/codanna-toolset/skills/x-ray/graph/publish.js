// Save a rendered graph under the project's .codanna/visualizations/, serve it
// from the skill dir (where ./graph/vendor lives), and open the browser.
const fs = require('fs');
const path = require('path');
const http = require('http');
const { execSync, spawn } = require('child_process');

const PORT = 3847;

function saveArtifact(html, safeName, workingDir) {
  const artifactDir = path.join(workingDir, '.codanna', 'visualizations');
  if (!fs.existsSync(artifactDir)) fs.mkdirSync(artifactDir, { recursive: true });
  const timestamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
  const artifactFile = path.join(artifactDir, `graph-${safeName}-${timestamp}.html`);
  fs.writeFileSync(artifactFile, html);
  return artifactFile;
}

function openBrowser(url) {
  console.log(`Opening: ${url}`);
  try {
    const opener = process.platform === 'darwin' ? 'open' :
                   process.platform === 'win32' ? 'start' : 'xdg-open';
    execSync(`${opener} "${url}"`, { stdio: 'ignore' });
  } catch (e) {
    // Ignore if can't open browser
  }
}

/** Write graph-view.html beside the vendor dir, start the static server if
 *  needed, open the browser. `open: false` skips server and browser. */
function serveAndOpen(html, skillDir, { open = true } = {}) {
  const url = `http://localhost:${PORT}/graph-view.html?t=${Date.now()}`;
  if (!open) return url;
  const serveFile = path.join(skillDir, 'graph-view.html');
  fs.writeFileSync(serveFile, html);
  const req = http.request({ hostname: 'localhost', port: PORT, timeout: 500 }, () => {
    console.log('Server already running');
    openBrowser(url);
  });
  req.on('error', () => {
    const serverScript = path.join(skillDir, 'graph', 'server.js');
    const server = spawn('node', [serverScript, String(PORT)], { stdio: 'ignore', detached: true });
    server.unref();
    console.log(`Started server on port ${PORT}`);
    console.log(`(Kill with: lsof -ti:${PORT} | xargs kill)`);
    setTimeout(() => openBrowser(url), 300);
  });
  req.end();
  return url;
}

/** Open a self-contained page straight from disk (no server needed). */
function openFile(filePath) {
  openBrowser('file://' + filePath);
}

module.exports = { saveArtifact, serveAndOpen, openFile, PORT };
